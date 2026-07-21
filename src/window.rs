use std::path::PathBuf;
use std::time::Duration;

use cosmic::app::Core;
use cosmic::iced::{
    advanced::text::{Ellipsize, EllipsizeHeightLimit, Wrapping},
    platform_specific::shell::commands::popup::{destroy_popup, get_popup},
    stream::channel,
    window,
    window::Id,
    Background,
    Color,
    ContentFit,
    Length,
    Limits,
    Subscription,
};
use cosmic::widget::{button, button::Catalog, icon, mouse_area, text, Column, Row};
use cosmic::{Action, Element, Task};
use mpris::PlayerFinder;

use crate::album_color::dominant_album_color;
use crate::fl;
use crate::metadata::{now_playing_from_player, now_playing_snapshot, NowPlayingData, PlayerInfo};
use crate::player::{playback_state_from_player, with_player};

const ID: &str = "com.github.DiegoMMR.CosmicExtAppletNowPlaying";

#[derive(Default)]
pub struct Window {
    core: Core,
    popup: Option<Id>,
    now_playing_text: String,
    now_playing_title: String,
    now_playing_artist: String,
    playback_state: PlaybackState,
    album_art_path: Option<PathBuf>,
    album_color: Option<Color>,
    has_active_media: bool,
    players: Vec<PlayerInfo>,
    selected_player: String,
    show_player_list: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
    #[default]
    Unknown,
}

#[derive(Clone, Debug)]
pub enum Message {
    TogglePopup,
    TogglePlayerList,
    SelectPlayer(String),
    PopupClosed(Id),
    PlayersChanged(Vec<PlayerInfo>),
    NowPlayingChanged(NowPlayingData),
    PreviousTrack,
    TogglePlayPause,
    NextTrack,
}

impl cosmic::Application for Window {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Action<Self::Message>>) {
        let initial = now_playing_snapshot();

        let window = Window {
            core,
            now_playing_text: initial.text,
            now_playing_title: initial.title,
            now_playing_artist: initial.artist,
            playback_state: initial.state,
            album_color: album_color_from_path(initial.album_art_path.as_deref()),
            album_art_path: initial.album_art_path,
            has_active_media: initial.has_active_media,
            ..Default::default()
        };

        (window, Task::none())
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn update(&mut self, message: Message) -> Task<Action<Self::Message>> {
        match message {
            Message::TogglePopup => {
                return if let Some(popup_id) = self.popup.take() {
                    self.show_player_list = false;
                    destroy_popup(popup_id)
                } else {
                    let new_id = Id::unique();
                    self.popup.replace(new_id);

                    let mut popup_settings = self.core.applet.get_popup_settings(
                        self.core.main_window_id().unwrap(),
                        new_id,
                        None,
                        None,
                        None,
                    );

                    popup_settings.positioner.size_limits = Limits::NONE
                        .max_width(370.0)
                        .min_width(200.0)
                        .min_height(200.0)
                        .max_height(1080.0);

                    get_popup(popup_settings)
                };
            }
            Message::TogglePlayerList => {
                self.show_player_list = !self.show_player_list;
                if self.popup.is_none() {
                    return self.update(Message::TogglePopup);
                }
            }
            Message::SelectPlayer(identity) => {
                self.selected_player = identity;
                self.show_player_list = false;
            }
            Message::PopupClosed(popup_id) => {
                if self.popup.as_ref() == Some(&popup_id) {
                    self.popup = None;
                }
            }
            Message::NowPlayingChanged(data) => {
                self.now_playing_text = data.text;
                self.now_playing_title = data.title;
                self.now_playing_artist = data.artist;
                self.playback_state = data.state;
                self.album_color = album_color_from_path(data.album_art_path.as_deref());
                self.album_art_path = data.album_art_path;
                self.has_active_media = data.has_active_media;
            }
            Message::PlayersChanged(players) => {
                if self.selected_player.is_empty()
                    || !players.iter().any(|p| p.identity == self.selected_player)
                {
                    self.selected_player = players
                        .iter()
                        .find(|p| p.state == PlaybackState::Playing)
                        .or_else(|| players.first())
                        .map(|p| p.identity.clone())
                        .unwrap_or_default();
                }
                self.players = players;
            }
            Message::PreviousTrack => {
                with_player(&self.selected_player, |player| {
                    let _ = player.previous();
                });
            }
            Message::TogglePlayPause => {
                with_player(&self.selected_player, |player| {
                    let _ = player.play_pause();
                });
            }
            Message::NextTrack => {
                with_player(&self.selected_player, |player| {
                    let _ = player.next();
                });
            }
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::run_with(self.selected_player.clone(), |selected_player| {
            let selected_player = selected_player.clone();
            channel(
                64,
                move |mut output: cosmic::iced::futures::channel::mpsc::Sender<Message>| async move {
                    std::thread::spawn(move || {
                        let mut last_sent = String::new();
                        let mut last_state = PlaybackState::Unknown;
                        let mut last_art: Option<PathBuf> = None;
                        let mut last_players = Vec::new();

                        loop {
                            let finder = match PlayerFinder::new() {
                                Ok(finder) => finder,
                                Err(_) => {
                                    std::thread::sleep(Duration::from_millis(1000));
                                    continue;
                                }
                            };

                            let players = finder.find_all().unwrap_or_default();
                            let infos: Vec<PlayerInfo> = players
                                .iter()
                                .map(|player| PlayerInfo {
                                    identity: player.identity().to_owned(),
                                    track: now_playing_from_player(player).text,
                                    state: playback_state_from_player(player),
                                })
                                .collect();
                            if infos != last_players {
                                last_players = infos.clone();
                                let _ = output.try_send(Message::PlayersChanged(infos));
                            }

                            let player = if selected_player.is_empty() {
                                finder.find_active().ok()
                            } else {
                                finder.find_by_name(&selected_player).ok()
                            }
                            .or_else(|| finder.find_active().ok());

                            let current = player
                                .as_ref()
                                .map(now_playing_from_player)
                                .unwrap_or(NowPlayingData {
                                    text: fl!("nothing-playing"),
                                    title: fl!("nothing-playing"),
                                    artist: String::new(),
                                    state: PlaybackState::Stopped,
                                    album_art_path: None,
                                    has_active_media: false,
                                });
                            let current_state = current.state;
                            let current_art = current.album_art_path.clone();
                            if current.text != last_sent
                                || current_state != last_state
                                || current_art != last_art
                            {
                                last_sent = current.text.clone();
                                last_state = current_state;
                                last_art = current_art.clone();
                                while output
                                    .try_send(Message::NowPlayingChanged(current.clone()))
                                    .is_err()
                                {
                                    std::thread::sleep(Duration::from_millis(10));
                                }
                            }

                            std::thread::sleep(Duration::from_millis(500));
                        }
                    });
                },
            )
        })
    }

    fn view(&self) -> Element<'_, Message> {
        if !self.has_active_media() {
            return self.core.applet.autosize_window(text("")).into();
        }

        let size = self.core.applet.suggested_size(true);
        let pad = self.core.applet.suggested_padding(true);
        let transport_icon = match self.playback_state {
            PlaybackState::Playing => "media-playback-pause-symbolic",
            PlaybackState::Paused | PlaybackState::Stopped | PlaybackState::Unknown => {
                "media-playback-start-symbolic"
            }
        };

        let row_content = Row::new()
            .spacing(pad.0)
            .padding([0, pad.0])
            .align_y(cosmic::iced::alignment::Vertical::Center)
            .push(
                mouse_area(icon::from_name(transport_icon).size(size.0))
                    .on_press(Message::TogglePlayPause),
            )
            .push(
                text(self.now_playing_text.as_str())
                    .size(size.0.saturating_sub(1))
                    .width(Length::Fixed(260.0))
                    .wrapping(Wrapping::None)
                    .ellipsize(Ellipsize::End(EllipsizeHeightLimit::Lines(1))),
            );

        let album_color = self.album_color;
        let content = button::custom(row_content)
            .width(Length::Shrink)
            .height(Length::Shrink)
            .class(cosmic::theme::Button::Custom {
                active: Box::new(move |focused, theme| {
                    let base = theme.active(focused, false, &cosmic::theme::Button::AppletIcon);
                    style_with_optional_album_color(base, album_color)
                }),
                disabled: Box::new(move |theme| {
                    let base = theme.disabled(&cosmic::theme::Button::AppletIcon);
                    style_with_optional_album_color(base, album_color)
                }),
                hovered: Box::new(move |focused, theme| {
                    let base = theme.hovered(focused, false, &cosmic::theme::Button::AppletIcon);
                    style_with_optional_album_color(base, album_color.map(|c| shift_color(c, 0.07)))
                }),
                pressed: Box::new(move |focused, theme| {
                    let base = theme.pressed(focused, false, &cosmic::theme::Button::AppletIcon);
                    style_with_optional_album_color(base, album_color.map(|c| shift_color(c, -0.08)))
                }),
            })
            .on_press(Message::TogglePopup);

        self.core.applet.autosize_window(content).into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Message> {
        if !self.has_active_media() {
            return self.core.applet.popup_container(text("")).into();
        }

        if self.show_player_list {
            let size = self.core.applet.suggested_size(true);
            let list = self.players.iter().enumerate().fold(
                Column::new().spacing(4).padding(12),
                |list, (index, player)| {
                    let selected = player.identity == self.selected_player;
                    let marker: Element<'_, Message> = if selected {
                        icon::from_name("emblem-ok-symbolic")
                            .size(size.0.saturating_sub(2))
                            .into()
                    } else {
                        text("").width(Length::Fixed(f32::from(size.0))).into()
                    };
                    let track = if player.track == fl!("nothing-playing") {
                        String::new()
                    } else {
                        player.track.clone()
                    };
                    let list = if index > 0 {
                        list.push(
                            cosmic::widget::divider::horizontal::light()
                                .height(1.0)
                                .width(Length::Fill),
                        )
                    } else {
                        list
                    };

                    list.push(
                        button::custom(
                            Row::new()
                                .spacing(8)
                                .align_y(cosmic::iced::alignment::Vertical::Center)
                                .push(marker)
                                .push(
                                    Column::new()
                                        .spacing(1)
                                        .push(text(player.identity.clone()))
                                        .push(text(track).size(size.0.saturating_sub(3))),
                                ),
                        )
                        .width(Length::Fill)
                        .selected(selected)
                        .class(cosmic::theme::Button::ListItem([4.0; 4]))
                        .on_press(Message::SelectPlayer(player.identity.clone())),
                    )
                },
            );
            return self.core.applet.popup_container(list).into();
        }

        let size = self.core.applet.suggested_size(true);
        let pad = self.core.applet.suggested_padding(true);
        let transport_icon = match self.playback_state {
            PlaybackState::Playing => "media-playback-pause-symbolic",
            PlaybackState::Paused | PlaybackState::Stopped | PlaybackState::Unknown => {
                "media-playback-start-symbolic"
            }
        };
        let album_height = size.0.saturating_mul(4);
        let album_width = album_height.saturating_mul(16) / 9;

        let album_widget = self
            .album_art_path
            .as_ref()
            .map(|path| {
                icon::icon(icon::from_path(path.clone()))
                    .height(Length::Fixed(f32::from(album_height)))
                    .width(Length::Fixed(f32::from(album_width)))
                    .content_fit(ContentFit::Contain)
            })
            .unwrap_or_else(|| {
                icon::from_name("audio-x-generic-symbolic")
                    .size(album_height)
                    .icon()
                    .height(Length::Fixed(f32::from(album_height)))
                    .width(Length::Fixed(f32::from(album_width)))
                    .content_fit(ContentFit::Contain)
            });

        let controls = Row::new()
            .spacing(pad.0)
            .align_y(cosmic::iced::alignment::Vertical::Center)
            .push(
                button::icon(icon::from_name("media-skip-backward-symbolic").size(size.0 + 4))
                    .on_press(Message::PreviousTrack),
            )
            .push(
                button::icon(icon::from_name(transport_icon).size(size.0 + 4))
                    .on_press(Message::TogglePlayPause),
            )
            .push(
                button::icon(icon::from_name("media-skip-forward-symbolic").size(size.0 + 4))
                    .on_press(Message::NextTrack),
            );

        let media_info = Column::new()
            .spacing(2)
            .padding([0, 16])
            .align_x(cosmic::iced::Alignment::Center)
            .push(
                text(self.now_playing_title.as_str())
                    .size(size.0.saturating_sub(1))
                    .width(Length::Fill)
                    .align_x(cosmic::iced::alignment::Horizontal::Center)
                    .wrapping(Wrapping::WordOrGlyph),
            )
            .push(
                text(self.now_playing_artist.as_str())
                    .size(size.0.saturating_sub(3))
                    .width(Length::Fill)
                    .align_x(cosmic::iced::alignment::Horizontal::Center)
                    .wrapping(Wrapping::WordOrGlyph),
            );

        let player_selector: Element<'_, Message> = if self.players.len() > 1 {
            button::icon(icon::from_name("view-list-symbolic").size(size.0))
                .on_press(Message::TogglePlayerList)
                .into()
        } else {
            text("").into()
        };

        let content_list = Column::new()
            .padding(12)
            .spacing(12)
            .align_x(cosmic::iced::Alignment::Center)
            .push(player_selector)
            .push(album_widget)
            .push(media_info)
            .push(controls);

        self.core.applet.popup_container(content_list).into()
    }
}

impl Window {
    fn has_active_media(&self) -> bool {
        self.has_active_media
    }
}

fn album_color_from_path(path: Option<&std::path::Path>) -> Option<Color> {
    Some(dominant_album_color(path).unwrap_or(Color::WHITE))
}

fn style_with_optional_album_color(mut base: button::Style, color: Option<Color>) -> button::Style {
    if let Some(album) = color {
        let theme_base = match base.background {
            Some(Background::Color(c)) => c,
            _ => Color::from_rgb8(36, 38, 42),
        };

        let mixed = blend_color(theme_base, album, 0.36);
        let background = with_alpha(mixed, 0.64);
        let border = with_alpha(shift_color(album, -0.05), 0.82);

        let foreground = contrast_text_color(background);
        base.background = Some(Background::Color(background));
        base.border_width = base.border_width.max(1.0);
        base.border_color = border;
        base.text_color = Some(foreground);
        base.icon_color = Some(foreground);
    }
    base
}

fn with_alpha(mut color: Color, alpha: f32) -> Color {
    color.a = alpha.clamp(0.0, 1.0);
    color
}

fn blend_color(a: Color, b: Color, ratio: f32) -> Color {
    let t = ratio.clamp(0.0, 1.0);
    let inv = 1.0 - t;
    Color {
        r: (a.r * inv) + (b.r * t),
        g: (a.g * inv) + (b.g * t),
        b: (a.b * inv) + (b.b * t),
        a: (a.a * inv) + (b.a * t),
    }
}

fn contrast_text_color(background: Color) -> Color {
    let luminance = 0.2126 * background.r + 0.7152 * background.g + 0.0722 * background.b;
    if luminance > 0.58 {
        Color::from_rgb8(17, 17, 17)
    } else {
        Color::WHITE
    }
}

fn shift_color(color: Color, amount: f32) -> Color {
    let adjust = |channel: f32| (channel + amount).clamp(0.0, 1.0);
    Color {
        r: adjust(color.r),
        g: adjust(color.g),
        b: adjust(color.b),
        a: color.a,
    }
}

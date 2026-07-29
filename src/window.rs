use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use cosmic::app::Core;
use cosmic::iced::{
    advanced::text::{Ellipsize, EllipsizeHeightLimit, Wrapping},
    stream::channel,
    window,
    window::Id,
    ContentFit, Length, Subscription,
};
use cosmic::widget::{button, container, icon, image, slider, text, Column, Row};
use cosmic::{Action, Element, Task};
use mpris::{Event as MprisEvent, PlayerFinder};

use crate::fl;
use crate::metadata::{now_playing_from_player, now_playing_snapshot, NowPlayingData};
use crate::player::{cycle_loop_status, find_selected_or_active, select_player, with_player};

const ID: &str = "com.github.DiegoMMR.CosmicExtAppletNowPlaying";
static POPUP_OPEN: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
pub struct Window {
    core: Core,
    popup: Option<Id>,
    now_playing_text: String,
    now_playing_title: String,
    now_playing_artist: String,
    now_playing_album: String,
    player_bus_name: String,
    sources: Vec<(String, String)>,
    duration_seconds: Option<u64>,
    position_seconds: Option<u64>,
    can_seek: bool,
    can_go_previous: bool,
    can_go_next: bool,
    can_shuffle: bool,
    shuffle: bool,
    can_loop: bool,
    playback_state: PlaybackState,
    album_art_path: Option<PathBuf>,
    has_active_media: bool,
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
    PopupClosed(Id),
    NowPlayingChanged(NowPlayingData),
    PreviousTrack,
    TogglePlayPause,
    NextTrack,
    SelectSource(String),
    ToggleShuffle,
    CycleLoop,
    Seek(i64),
    SeekTo(u64),
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

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        // Applets must explicitly opt into COSMIC's applet style. Without
        // this, Iced falls back to its default blue controls instead of the
        // user's configured COSMIC accent and surface colours.
        Some(cosmic::applet::style())
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Action<Self::Message>>) {
        let initial = now_playing_snapshot();

        let window = Window {
            core,
            now_playing_text: initial.text,
            now_playing_title: initial.title,
            now_playing_artist: initial.artist,
            now_playing_album: initial.album,
            player_bus_name: initial.player_bus_name,
            sources: initial.sources,
            duration_seconds: initial.duration_seconds,
            position_seconds: initial.position_seconds,
            can_seek: initial.capabilities.seek,
            can_go_previous: initial.capabilities.previous,
            can_go_next: initial.capabilities.next,
            can_shuffle: initial.capabilities.shuffle,
            shuffle: initial.shuffle,
            can_loop: initial.capabilities.loop_mode,
            playback_state: initial.state,
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
                    POPUP_OPEN.store(false, Ordering::Relaxed);
                    cosmic::surface::surface_task(cosmic::surface::action::destroy_popup(popup_id))
                } else {
                    cosmic::surface::surface_task(cosmic::surface::action::app_popup(
                        |_| Default::default(),
                        |app: &mut Self| {
                            POPUP_OPEN.store(true, Ordering::Relaxed);
                            let new_id = Id::unique();
                            app.popup.replace(new_id);
                            app.core.applet.get_popup_settings(
                                app.core.main_window_id().unwrap(),
                                new_id,
                                None,
                                None,
                                None,
                            )
                        },
                        None,
                    ))
                };
            }
            Message::PopupClosed(popup_id) => {
                if self.popup.as_ref() == Some(&popup_id) {
                    self.popup = None;
                    POPUP_OPEN.store(false, Ordering::Relaxed);
                }
            }
            Message::NowPlayingChanged(data) => {
                self.now_playing_text = data.text;
                self.now_playing_title = data.title;
                self.now_playing_artist = data.artist;
                self.now_playing_album = data.album;
                self.player_bus_name = data.player_bus_name;
                self.sources = data.sources;
                self.duration_seconds = data.duration_seconds;
                self.position_seconds = data.position_seconds;
                self.can_seek = data.capabilities.seek;
                self.can_go_previous = data.capabilities.previous;
                self.can_go_next = data.capabilities.next;
                self.can_shuffle = data.capabilities.shuffle;
                self.shuffle = data.shuffle;
                self.can_loop = data.capabilities.loop_mode;
                self.playback_state = data.state;
                self.album_art_path = data.album_art_path;
                self.has_active_media = data.has_active_media;
            }
            Message::PreviousTrack => {
                with_player(&self.player_bus_name, |player| {
                    let _ = player.previous();
                });
            }
            Message::TogglePlayPause => {
                with_player(&self.player_bus_name, |player| {
                    let _ = player.play_pause();
                });
            }
            Message::NextTrack => {
                with_player(&self.player_bus_name, |player| {
                    let _ = player.next();
                });
            }
            Message::SelectSource(bus_name) => {
                select_player(bus_name.clone());
                // Selection is applied immediately; the periodic MPRIS refresh
                // will reconcile metadata and capabilities on the next tick.
                with_player(&bus_name, |player| {
                    let data = now_playing_from_player(player);
                    self.now_playing_text = data.text;
                    self.now_playing_title = data.title;
                    self.now_playing_artist = data.artist;
                    self.now_playing_album = data.album;
                    self.player_bus_name = data.player_bus_name;
                    self.album_art_path = data.album_art_path;
                });
            }
            Message::ToggleShuffle => with_player(&self.player_bus_name, |player| {
                if let Ok(shuffle) = player.get_shuffle() {
                    let _ = player.set_shuffle(!shuffle);
                }
            }),
            Message::CycleLoop => with_player(&self.player_bus_name, cycle_loop_status),
            Message::Seek(offset) => with_player(&self.player_bus_name, |player| {
                let _ = player.seek(offset);
            }),
            Message::SeekTo(seconds) => {
                let current = self.position_seconds.unwrap_or(0);
                let offset = i64::try_from(seconds.saturating_sub(current)).unwrap_or(i64::MAX);
                let offset = if seconds < current { -offset } else { offset };
                with_player(&self.player_bus_name, |player| {
                    let _ = player.seek(offset.saturating_mul(1_000_000));
                });
            }
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::batch(vec![
            Subscription::run(|| {
                channel(
                    64,
                    |mut output: cosmic::iced::futures::channel::mpsc::Sender<Message>| async move {
                        std::thread::spawn(move || {
                            let mut last_sent = String::new();
                            let mut last_state = PlaybackState::Unknown;
                            let mut last_art: Option<PathBuf> = None;

                            loop {
                                let finder = match PlayerFinder::new() {
                                    Ok(finder) => finder,
                                    Err(_) => {
                                        std::thread::sleep(Duration::from_millis(1000));
                                        continue;
                                    }
                                };

                                let player = match find_selected_or_active(&finder) {
                                    Some(player) => player,
                                    None => {
                                        if last_sent != fl!("nothing-playing")
                                            || last_state != PlaybackState::Stopped
                                        {
                                            last_sent = fl!("nothing-playing");
                                            last_state = PlaybackState::Stopped;
                                            last_art = None;
                                            while output
                                                .try_send(Message::NowPlayingChanged(
                                                    NowPlayingData {
                                                        text: last_sent.clone(),
                                                        title: fl!("nothing-playing"),
                                                        artist: String::new(),
                                                        album: String::new(),
                                                        player_bus_name: String::new(),
                                                        sources: Vec::new(),
                                                        duration_seconds: None,
                                                        position_seconds: None,
                                                        capabilities:
                                                            crate::metadata::PlaybackCapabilities {
                                                                seek: false,
                                                                previous: false,
                                                                next: false,
                                                                shuffle: false,
                                                                loop_mode: false,
                                                            },
                                                        shuffle: false,
                                                        state: last_state,
                                                        album_art_path: None,
                                                        has_active_media: false,
                                                    },
                                                ))
                                                .is_err()
                                            {
                                                std::thread::sleep(Duration::from_millis(10));
                                            }
                                        }

                                        std::thread::sleep(Duration::from_millis(1000));
                                        continue;
                                    }
                                };

                                let current = now_playing_from_player(&player);
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

                                let mut events = match player.events() {
                                    Ok(events) => events,
                                    Err(_) => {
                                        std::thread::sleep(Duration::from_millis(300));
                                        continue;
                                    }
                                };

                                for event in &mut events {
                                    match event {
                                        Ok(MprisEvent::TrackChanged(_metadata)) => {
                                            let data = now_playing_from_player(&player);
                                            let text = data.text.clone();
                                            let state = data.state;
                                            let art = data.album_art_path.clone();

                                            if text != last_sent
                                                || state != last_state
                                                || art != last_art
                                            {
                                                last_sent = text.clone();
                                                last_state = state;
                                                last_art = art.clone();
                                                while output
                                                    .try_send(Message::NowPlayingChanged(
                                                        data.clone(),
                                                    ))
                                                    .is_err()
                                                {
                                                    std::thread::sleep(Duration::from_millis(10));
                                                }
                                            }
                                        }
                                        Ok(MprisEvent::Playing)
                                        | Ok(MprisEvent::Paused)
                                        | Ok(MprisEvent::Stopped) => {
                                            let data = now_playing_from_player(&player);
                                            let text = data.text.clone();
                                            let state = data.state;
                                            let art = data.album_art_path.clone();

                                            if text != last_sent
                                                || state != last_state
                                                || art != last_art
                                            {
                                                last_sent = text;
                                                last_state = state;
                                                last_art = art.clone();
                                                while output
                                                    .try_send(Message::NowPlayingChanged(
                                                        data.clone(),
                                                    ))
                                                    .is_err()
                                                {
                                                    std::thread::sleep(Duration::from_millis(10));
                                                }
                                            }
                                        }
                                        Ok(MprisEvent::PlayerShutDown) | Err(_) => break,
                                        _ => {}
                                    }
                                }

                                std::thread::sleep(Duration::from_millis(200));
                            }
                        });
                    },
                )
            }),
            Subscription::run(|| {
                channel(
                    8,
                    |mut output: cosmic::iced::futures::channel::mpsc::Sender<Message>| async move {
                        std::thread::spawn(move || loop {
                            let data = now_playing_snapshot();
                            // Position changes only matter while the card is visible.
                            // This keeps the panel idle between metadata changes.
                            if POPUP_OPEN.load(Ordering::Relaxed) {
                                let _ = output.try_send(Message::NowPlayingChanged(data));
                            }
                            std::thread::sleep(Duration::from_secs(1));
                        });
                    },
                )
            }),
        ])
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

        let panel_previous = self.core.applet.icon_button("media-skip-backward-symbolic");
        let panel_previous = if self.can_go_previous {
            panel_previous.on_press(Message::PreviousTrack)
        } else {
            panel_previous
        };
        let panel_next = self.core.applet.icon_button("media-skip-forward-symbolic");
        let panel_next = if self.can_go_next {
            panel_next.on_press(Message::NextTrack)
        } else {
            panel_next
        };
        let panel_play = self
            .core
            .applet
            .icon_button(transport_icon)
            .on_press(Message::TogglePlayPause);
        let track_label = button::custom(
            text(self.now_playing_text.as_str())
                .size(size.0.saturating_sub(1))
                .width(Length::Fixed(200.0))
                .wrapping(Wrapping::None)
                .ellipsize(Ellipsize::End(EllipsizeHeightLimit::Lines(1))),
        )
        .class(cosmic::theme::Button::AppletIcon)
        .on_press(Message::TogglePopup);
        let row_content = Row::new()
            .spacing(pad.0)
            .padding([0, pad.0])
            .align_y(cosmic::iced::alignment::Vertical::Center)
            .push(panel_previous)
            .push(panel_play)
            .push(panel_next)
            .push(track_label);

        // Each panel action is its own applet button; nesting them inside one
        // catch-all popup button would swallow the transport clicks.
        self.core.applet.autosize_window(row_content).into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Message> {
        if !self.has_active_media() {
            return self.core.applet.popup_container(text("")).into();
        }

        let size = self.core.applet.suggested_size(true);
        let pad = self.core.applet.suggested_padding(true);
        let transport_icon = match self.playback_state {
            PlaybackState::Playing => "media-playback-pause-symbolic",
            PlaybackState::Paused | PlaybackState::Stopped | PlaybackState::Unknown => {
                "media-playback-start-symbolic"
            }
        };
        // MediaShell uses the popup's full content width for cover art.  The
        // previous 16:9 box both distorted the visual hierarchy and left a
        // large, awkward amount of unused popup width.
        const POPUP_CONTENT_WIDTH: f32 = 328.0;

        let album_widget: Element<'_, Message> = if let Some(path) = self.album_art_path.as_ref() {
            image(image::Handle::from_path(path.clone()))
                .height(Length::Fixed(POPUP_CONTENT_WIDTH))
                .width(Length::Fixed(POPUP_CONTENT_WIDTH))
                // Preserve unusually tall/wide artwork inside the square card.
                // `Cover` can enlarge it beyond the image bounds on some
                // fractional-scale renderers.
                .content_fit(ContentFit::Contain)
                .border_radius(12.0)
                .into()
        } else {
            icon::from_name("audio-x-generic-symbolic")
                .size(72)
                .icon()
                .height(Length::Fixed(POPUP_CONTENT_WIDTH))
                .width(Length::Fixed(POPUP_CONTENT_WIDTH))
                .content_fit(ContentFit::Contain)
                .into()
        };

        let previous =
            button::icon(icon::from_name("media-skip-backward-symbolic").size(size.0 + 4));
        let previous = if self.can_go_previous {
            previous.on_press(Message::PreviousTrack)
        } else {
            previous
        };
        let next = button::icon(icon::from_name("media-skip-forward-symbolic").size(size.0 + 4));
        let next = if self.can_go_next {
            next.on_press(Message::NextTrack)
        } else {
            next
        };
        let controls = Row::new()
            .spacing(pad.0.saturating_add(6))
            .align_y(cosmic::iced::alignment::Vertical::Center)
            .push(if self.can_seek {
                button::icon(icon::from_name("media-seek-backward-symbolic").size(size.0))
                    .on_press(Message::Seek(-10_000_000))
            } else {
                button::icon(icon::from_name("media-seek-backward-symbolic").size(size.0))
            })
            .push(previous)
            .push(
                button::icon(icon::from_name(transport_icon).size(size.0 + 8))
                    .class(cosmic::theme::Button::Suggested)
                    .on_press(Message::TogglePlayPause),
            )
            .push(next)
            .push(if self.can_seek {
                button::icon(icon::from_name("media-seek-forward-symbolic").size(size.0))
                    .on_press(Message::Seek(10_000_000))
            } else {
                button::icon(icon::from_name("media-seek-forward-symbolic").size(size.0))
            });

        let media_info = Column::new()
            .spacing(4)
            .align_x(cosmic::iced::Alignment::Start)
            .push(
                text(self.now_playing_title.as_str())
                    .size(size.0.saturating_add(5))
                    .width(Length::Fixed(POPUP_CONTENT_WIDTH))
                    .wrapping(Wrapping::None)
                    .ellipsize(Ellipsize::End(EllipsizeHeightLimit::Lines(1))),
            )
            .push(
                text(self.now_playing_artist.as_str())
                    .size(size.0)
                    .width(Length::Fixed(POPUP_CONTENT_WIDTH))
                    .wrapping(Wrapping::None)
                    .ellipsize(Ellipsize::End(EllipsizeHeightLimit::Lines(1))),
            )
            .push(
                text(self.now_playing_album.as_str())
                    .size(size.0.saturating_sub(2))
                    .width(Length::Fixed(POPUP_CONTENT_WIDTH))
                    .wrapping(Wrapping::None)
                    .ellipsize(Ellipsize::End(EllipsizeHeightLimit::Lines(1))),
            );

        let position = self.position_seconds.unwrap_or(0);
        let duration = self.duration_seconds.unwrap_or(0);
        let progress = format!(
            "{}  /  {}",
            format_duration(position),
            format_duration(duration)
        );
        let progress_bar = if self.can_seek && duration > 0 {
            slider(
                0.0..=duration as f32,
                position.min(duration) as f32,
                |value| Message::SeekTo(value.round() as u64),
            )
            .width(Length::Fixed(220.0))
        } else {
            slider(0.0..=1.0, 0.0, |_| Message::SeekTo(0)).width(Length::Fixed(220.0))
        };
        let progress_row = Row::new()
            .spacing(8)
            .align_y(cosmic::iced::alignment::Vertical::Center)
            .push(progress_bar)
            .push(
                text(progress)
                    .width(Length::Fixed(88.0))
                    .wrapping(Wrapping::None)
                    .align_x(cosmic::iced::alignment::Horizontal::Right),
            );

        let mode_controls = Row::new()
            .spacing(8)
            .push(if self.can_shuffle {
                button::icon(
                    icon::from_name(if self.shuffle {
                        "media-playlist-shuffle-symbolic"
                    } else {
                        "media-playlist-shuffle-symbolic"
                    })
                    .size(size.0),
                )
                .on_press(Message::ToggleShuffle)
            } else {
                button::icon(icon::from_name("media-playlist-shuffle-symbolic").size(size.0))
            })
            .push(if self.can_loop {
                button::icon(icon::from_name("media-playlist-repeat-symbolic").size(size.0))
                    .on_press(Message::CycleLoop)
            } else {
                button::icon(icon::from_name("media-playlist-repeat-symbolic").size(size.0))
            });

        let source_picker =
            self.sources
                .iter()
                .fold(Row::new().spacing(4), |row, (bus_name, name)| {
                    let label = if bus_name == &self.player_bus_name {
                        format!("• {name}")
                    } else {
                        name.clone()
                    };
                    row.push(
                        button::standard(label)
                            .class(if bus_name == &self.player_bus_name {
                                cosmic::theme::Button::Suggested
                            } else {
                                cosmic::theme::Button::Standard
                            })
                            .on_press(Message::SelectSource(bus_name.clone())),
                    )
                });

        let content_list = Column::new()
            .padding(16)
            .spacing(12)
            .align_x(cosmic::iced::Alignment::Center)
            // A fixed header prevents the cover image from being laid out over
            // the source controls on fractional display scales.
            .push(
                container(source_picker)
                    .height(Length::Fixed(36.0))
                    .align_y(cosmic::iced::alignment::Vertical::Center),
            )
            .push(album_widget)
            .push(media_info)
            .push(progress_row)
            .push(controls)
            .push(mode_controls);

        // Match COSMIC's own applets: the applet popup owns the themed card
        // surface, while our content is only its child. This keeps its
        // background, border, radius and text colours under COSMIC control.
        self.core
            .applet
            .popup_container(container(content_list))
            .into()
    }
}

fn format_duration(seconds: u64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

impl Window {
    fn has_active_media(&self) -> bool {
        self.has_active_media
    }
}

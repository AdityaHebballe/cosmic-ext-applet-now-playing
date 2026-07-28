use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use cosmic::iced::futures::channel::mpsc;
use cosmic::iced::{Subscription, stream::channel};
use mpris::{PlaybackStatus, PlayerFinder};
use url::Url;

use crate::fl;
use crate::model::{MediaSnapshot, NowPlayingData, PlaybackState, PlayerInfo};

const POLL_INTERVAL: Duration = Duration::from_millis(500);
const RECONNECT_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug)]
pub enum MediaCommand {
    Previous,
    TogglePlayPause,
    Next,
}

#[must_use]
pub fn initial_snapshot() -> MediaSnapshot {
    let finder = match PlayerFinder::new() {
        Ok(finder) => finder,
        Err(error) => {
            eprintln!("unable to connect to MPRIS: {error}");
            return MediaSnapshot::default();
        }
    };

    match scan(&finder) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            eprintln!("unable to read initial MPRIS state: {error}");
            MediaSnapshot::default()
        }
    }
}

pub fn subscription() -> Subscription<MediaSnapshot> {
    Subscription::run(monitor)
}

pub fn run_command(selected_player: Option<&str>, command: MediaCommand) -> Result<(), String> {
    let selected_player =
        selected_player.ok_or_else(|| "no media player is selected".to_owned())?;
    let finder =
        PlayerFinder::new().map_err(|error| format!("unable to connect to MPRIS: {error}"))?;
    let players = finder
        .find_all()
        .map_err(|error| format!("unable to list MPRIS players: {error}"))?;
    let player = players
        .iter()
        .find(|player| player.bus_name() == selected_player)
        .ok_or_else(|| format!("selected MPRIS player disappeared: {selected_player}"))?;

    let result = match command {
        MediaCommand::Previous => player.previous(),
        MediaCommand::TogglePlayPause => player.play_pause(),
        MediaCommand::Next => player.next(),
    };

    result.map_err(|error| {
        format!(
            "MPRIS command {command:?} failed for {}: {error}",
            player.identity()
        )
    })
}

fn monitor() -> impl cosmic::iced::futures::Stream<Item = MediaSnapshot> {
    channel(8, |mut output: mpsc::Sender<MediaSnapshot>| async move {
        thread::spawn(move || monitor_loop(&mut output));
    })
}

fn monitor_loop(output: &mut mpsc::Sender<MediaSnapshot>) {
    let mut finder = None;
    let mut last_sent = None;

    loop {
        if output.is_closed() {
            break;
        }

        if finder.is_none() {
            match PlayerFinder::new() {
                Ok(new_finder) => finder = Some(new_finder),
                Err(error) => {
                    eprintln!("unable to connect to MPRIS: {error}");
                    thread::sleep(RECONNECT_INTERVAL);
                    continue;
                }
            }
        }

        let Some(active_finder) = finder.as_ref() else {
            continue;
        };
        let snapshot = match scan(active_finder) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                eprintln!("unable to poll MPRIS players: {error}");
                finder = None;
                thread::sleep(RECONNECT_INTERVAL);
                continue;
            }
        };

        if last_sent.as_ref() != Some(&snapshot) {
            match output.try_send(snapshot.clone()) {
                Ok(()) => last_sent = Some(snapshot),
                Err(error) if error.is_disconnected() => break,
                Err(_) => {}
            }
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn scan(finder: &PlayerFinder) -> Result<MediaSnapshot, mpris::FindingError> {
    let players = finder
        .find_all()?
        .iter()
        .map(player_info)
        .collect::<Vec<_>>();
    Ok(MediaSnapshot { players })
}

fn player_info(player: &mpris::Player) -> PlayerInfo {
    PlayerInfo {
        id: player.bus_name().to_owned(),
        identity: player.identity().to_owned(),
        now_playing: now_playing(player),
    }
}

fn now_playing(player: &mpris::Player) -> NowPlayingData {
    let state = playback_state(player);
    let Ok(metadata) = player.get_metadata() else {
        return NowPlayingData {
            state,
            ..NowPlayingData::nothing_playing()
        };
    };

    if metadata.is_empty() {
        return NowPlayingData {
            state,
            ..NowPlayingData::nothing_playing()
        };
    }

    let title = metadata
        .title()
        .map_or_else(|| fl!("unknown-title"), ToOwned::to_owned);
    let artist = metadata
        .artists()
        .and_then(|artists| artists.first().copied())
        .map_or_else(|| fl!("unknown-artist"), ToOwned::to_owned);

    NowPlayingData {
        text: format!("{title} - {artist}"),
        title,
        artist,
        state,
        album_art_path: metadata.art_url().and_then(file_url_to_path),
        has_usable_metadata: true,
    }
}

fn playback_state(player: &mpris::Player) -> PlaybackState {
    match player.get_playback_status() {
        Ok(PlaybackStatus::Playing) => PlaybackState::Playing,
        Ok(PlaybackStatus::Paused) => PlaybackState::Paused,
        Ok(PlaybackStatus::Stopped) => PlaybackState::Stopped,
        Err(_) => PlaybackState::Unknown,
    }
}

fn file_url_to_path(value: &str) -> Option<PathBuf> {
    if !has_valid_percent_encoding(value) {
        return None;
    }

    Url::parse(value).ok()?.to_file_path().ok()
}

fn has_valid_percent_encoding(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::file_url_to_path;
    use std::path::PathBuf;

    #[test]
    fn parses_local_file_urls() {
        assert_eq!(
            file_url_to_path("file:///home/user/Music/Album%20Art.png"),
            Some(PathBuf::from("/home/user/Music/Album Art.png"))
        );
        assert_eq!(
            file_url_to_path("file://localhost/home/user/M%C3%BAsica.png"),
            Some(PathBuf::from("/home/user/Música.png"))
        );
    }

    #[test]
    fn parses_unescaped_unicode_file_urls() {
        assert_eq!(
            file_url_to_path("file:///home/user/Música/Portada.png"),
            Some(PathBuf::from("/home/user/Música/Portada.png"))
        );
    }

    #[test]
    fn rejects_remote_non_file_and_malformed_urls() {
        assert_eq!(file_url_to_path("https://example.com/cover.png"), None);
        assert_eq!(
            file_url_to_path("file://example.com/home/user/cover.png"),
            None
        );
        assert_eq!(file_url_to_path("file:///home/user/bad%2.png"), None);
        assert_eq!(file_url_to_path("file:///home/user/bad%ZZ.png"), None);
    }
}

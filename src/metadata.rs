use std::path::PathBuf;

use mpris::PlayerFinder;

use crate::fl;
use crate::player::{
    album_art_path_from_metadata, find_selected_or_active, playback_state_from_player,
    player_sources,
};
use crate::window::PlaybackState;

#[derive(Clone, Debug)]
pub struct PlaybackCapabilities {
    pub seek: bool,
    pub previous: bool,
    pub next: bool,
    pub shuffle: bool,
    pub loop_mode: bool,
}

#[derive(Clone, Debug)]
pub struct NowPlayingData {
    pub text: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub player_bus_name: String,
    pub sources: Vec<(String, String)>,
    pub duration_seconds: Option<u64>,
    pub position_seconds: Option<u64>,
    pub capabilities: PlaybackCapabilities,
    pub shuffle: bool,
    pub state: PlaybackState,
    pub album_art_path: Option<PathBuf>,
    pub has_active_media: bool,
}

pub fn now_playing_snapshot() -> NowPlayingData {
    let finder = PlayerFinder::new();

    if let Ok(finder) = finder {
        if let Some(player) = find_selected_or_active(&finder) {
            return now_playing_from_player(&player);
        }
    }

    NowPlayingData {
        text: fl!("nothing-playing"),
        title: fl!("nothing-playing"),
        artist: String::new(),
        album: String::new(),
        player_bus_name: String::new(),
        sources: Vec::new(),
        duration_seconds: None,
        position_seconds: None,
        shuffle: false,
        capabilities: PlaybackCapabilities {
            seek: false,
            previous: false,
            next: false,
            shuffle: false,
            loop_mode: false,
        },
        state: PlaybackState::Stopped,
        album_art_path: None,
        has_active_media: false,
    }
}

pub fn now_playing_from_player(player: &mpris::Player) -> NowPlayingData {
    let playback_state = playback_state_from_player(player);

    if let Ok(meta) = player.get_metadata() {
        let title = meta
            .title()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| fl!("unknown-title"));
        let artist = meta
            .artists()
            .and_then(|a| a.first().copied())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| fl!("unknown-artist"));
        let album_art_path = album_art_path_from_metadata(&meta);

        return NowPlayingData {
            text: format!("{title} • {artist}"),
            title,
            artist,
            album: meta.album_name().unwrap_or_default().to_owned(),
            player_bus_name: player.bus_name().to_owned(),
            sources: player_sources(),
            duration_seconds: meta.length().map(|duration| duration.as_secs()),
            position_seconds: player
                .get_position()
                .ok()
                .map(|duration| duration.as_secs()),
            capabilities: PlaybackCapabilities {
                seek: player.can_seek().unwrap_or(false),
                previous: player.can_go_previous().unwrap_or(false),
                next: player.can_go_next().unwrap_or(false),
                shuffle: player.can_shuffle().unwrap_or(false),
                loop_mode: player.can_loop().unwrap_or(false),
            },
            shuffle: player.get_shuffle().unwrap_or(false),
            state: playback_state,
            album_art_path,
            has_active_media: true,
        };
    }

    NowPlayingData {
        text: fl!("nothing-playing"),
        title: fl!("nothing-playing"),
        artist: String::new(),
        album: String::new(),
        player_bus_name: String::new(),
        sources: Vec::new(),
        duration_seconds: None,
        position_seconds: None,
        capabilities: PlaybackCapabilities {
            seek: false,
            previous: false,
            next: false,
            shuffle: false,
            loop_mode: false,
        },
        shuffle: false,
        state: playback_state,
        album_art_path: None,
        has_active_media: false,
    }
}

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use mpris::{LoopStatus, PlaybackStatus, PlayerFinder};

use crate::window::PlaybackState;

static SELECTED_PLAYER: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static ART_DOWNLOADS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static ART_FAILURES: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();

fn selected_player_slot() -> &'static Mutex<Option<String>> {
    SELECTED_PLAYER.get_or_init(|| Mutex::new(None))
}

pub fn select_player(bus_name: String) {
    if let Ok(mut selected) = selected_player_slot().lock() {
        *selected = Some(bus_name);
    }
}

/// Select the explicitly chosen source when it still exists; otherwise follow
/// MPRIS' active-player preference (playing, paused, metadata, first player).
/// Taking the already enumerated player list lets a poll build its source list
/// and chosen snapshot from a single DBus enumeration.
pub fn find_selected_or_active_from_players(players: Vec<mpris::Player>) -> Option<mpris::Player> {
    let selected = selected_player_slot()
        .lock()
        .ok()
        .and_then(|selected| selected.clone());
    if let Some(bus_name) = selected {
        if let Some(player) = players
            .iter()
            .position(|player| player.bus_name() == bus_name)
        {
            return players.into_iter().nth(player);
        }
        // A selected player which has exited should not trap the applet on an
        // empty source forever.
        if let Ok(mut selected) = selected_player_slot().lock() {
            *selected = None;
        }
    }
    let mut first_paused = None;
    let mut first_with_track = None;
    let mut first_found = None;
    for player in players {
        match player.get_playback_status() {
            Ok(PlaybackStatus::Playing) => return Some(player),
            Ok(PlaybackStatus::Paused) if first_paused.is_none() => first_paused = Some(player),
            _ if first_with_track.is_none()
                && player
                    .get_metadata()
                    .map(|metadata| !metadata.is_empty())
                    .unwrap_or(false) =>
            {
                first_with_track = Some(player)
            }
            _ if first_found.is_none() => first_found = Some(player),
            _ => {}
        }
    }
    first_paused.or(first_with_track).or(first_found)
}

pub fn playback_state_from_player(player: &mpris::Player) -> PlaybackState {
    match player.get_playback_status() {
        Ok(PlaybackStatus::Playing) => PlaybackState::Playing,
        Ok(PlaybackStatus::Paused) => PlaybackState::Paused,
        Ok(PlaybackStatus::Stopped) => PlaybackState::Stopped,
        Err(_) => PlaybackState::Unknown,
    }
}

pub fn album_art_path_from_metadata(meta: &mpris::Metadata) -> Option<PathBuf> {
    let art_url = meta.art_url()?;
    file_url_to_path(art_url).or_else(|| download_remote_album_art(art_url))
}

/// MPRIS players are allowed to publish either a local `file://` URL or a
/// remote HTTP(S) URL for cover art. Browsers and Spotify commonly use the
/// latter, so cache it locally before handing it to COSMIC's image widget.
fn download_remote_album_art(url: &str) -> Option<PathBuf> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return None;
    }

    let cache_dir = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?
        .join("cosmic-ext-applet-now-playing");
    fs::create_dir_all(&cache_dir).ok()?;

    let path = cache_dir.join(format!("{}.{}", stable_url_hash(url), image_extension(url)));
    if path.is_file() {
        return Some(path);
    }

    let failures = ART_FAILURES.get_or_init(|| Mutex::new(HashMap::new()));
    if failures
        .lock()
        .ok()
        .and_then(|failures| failures.get(url).copied())
        .is_some_and(|last_failure| last_failure.elapsed() < Duration::from_secs(30))
    {
        return None;
    }

    let downloads = ART_DOWNLOADS.get_or_init(|| Mutex::new(HashSet::new()));
    let should_start = downloads
        .lock()
        .ok()
        .is_some_and(|mut downloads| downloads.insert(url.to_owned()));
    if !should_start {
        return None;
    }

    let url = url.to_owned();
    std::thread::spawn(move || {
        let temporary = path.with_extension(format!("{}.part", image_extension(&url)));
        let result = (|| -> io::Result<()> {
            let response = ureq::get(&url)
                .timeout(Duration::from_secs(8))
                .call()
                .map_err(|error| io::Error::other(error.to_string()))?;
            let mut reader = response.into_reader();
            let mut file = fs::File::create(&temporary)?;
            io::copy(&mut reader, &mut file)?;
            file.sync_all()?;
            fs::rename(&temporary, &path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
            if let Ok(mut failures) = ART_FAILURES
                .get_or_init(|| Mutex::new(HashMap::new()))
                .lock()
            {
                failures.insert(url.clone(), Instant::now());
            }
        }
        if let Ok(mut downloads) = ART_DOWNLOADS
            .get_or_init(|| Mutex::new(HashSet::new()))
            .lock()
        {
            downloads.remove(&url);
        }
    });
    None
}

fn stable_url_hash(url: &str) -> u64 {
    // FNV-1a gives the cache a stable, filesystem-safe name without adding a
    // hashing dependency or storing the remote URL in a filename.
    url.as_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

fn image_extension(url: &str) -> &'static str {
    let path = url.split('?').next().unwrap_or(url).to_ascii_lowercase();
    if path.ends_with(".png") {
        "png"
    } else if path.ends_with(".webp") {
        "webp"
    } else if path.ends_with(".jpeg") {
        "jpeg"
    } else {
        "jpg"
    }
}

fn file_url_to_path(url: &str) -> Option<PathBuf> {
    let path = url
        .strip_prefix("file://localhost")
        .or_else(|| url.strip_prefix("file://"))?;

    Some(PathBuf::from(percent_decode_path(path)))
}

fn percent_decode_path(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2])) {
                decoded.push((high << 4) | low);
                i += 3;
                continue;
            }
        }

        decoded.push(bytes[i]);
        i += 1;
    }

    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub fn player_sources_from_players(players: &[mpris::Player]) -> Vec<(String, String)> {
    players
        .iter()
        .map(|player| (player.bus_name().to_owned(), player.identity().to_owned()))
        .collect()
}

pub fn with_player<F>(bus_name: &str, f: F)
where
    F: FnOnce(&mpris::Player),
{
    if let Ok(finder) = PlayerFinder::new() {
        if let Ok(players) = finder.find_all() {
            if let Some(player) = players
                .into_iter()
                .find(|player| player.bus_name() == bus_name)
            {
                f(&player);
            }
        }
    }
}

pub fn cycle_loop_status(player: &mpris::Player) {
    let next = match player.get_loop_status().unwrap_or(LoopStatus::None) {
        LoopStatus::None => LoopStatus::Playlist,
        LoopStatus::Playlist => LoopStatus::Track,
        LoopStatus::Track => LoopStatus::None,
    };
    let _ = player.set_loop_status(next);
}

#[cfg(test)]
mod tests {
    use super::{file_url_to_path, image_extension};
    use std::path::PathBuf;

    #[test]
    fn parses_file_url_paths() {
        assert_eq!(
            file_url_to_path("file:///home/user/Music/Album%20Art.png"),
            Some(PathBuf::from("/home/user/Music/Album Art.png"))
        );
    }

    #[test]
    fn ignores_non_file_urls() {
        assert_eq!(file_url_to_path("https://example.com/cover.png"), None);
    }

    #[test]
    fn chooses_a_safe_cache_extension() {
        assert_eq!(
            image_extension("https://example.com/cover.webp?size=640"),
            "webp"
        );
        assert_eq!(image_extension("https://example.com/cover"), "jpg");
    }
}

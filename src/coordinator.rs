//! The single owner of background MPRIS polling.
//!
//! Keeping event watching and position polling in one worker prevents the two
//! old subscriptions from racing to overwrite each other's snapshots.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use cosmic::iced::futures::channel::mpsc;
use cosmic::iced::{stream::channel, Subscription};
use mpris::PlayerFinder;

use crate::metadata::{now_playing_snapshot, NowPlayingData};
use crate::player::find_selected_or_active_from_players;
use crate::window::PlaybackState;

static POPUP_OPEN: AtomicBool = AtomicBool::new(false);
static POLL_WAKE: OnceLock<(Mutex<bool>, Condvar)> = OnceLock::new();

const ACTIVE_INTERVAL: Duration = Duration::from_secs(1);
const BACKGROUND_INTERVAL: Duration = Duration::from_secs(3);

pub fn set_popup_open(open: bool) {
    POPUP_OPEN.store(open, Ordering::Relaxed);
    request_refresh();
}

/// Request one immediate snapshot without changing the normal polling rate.
/// This is used for MPRIS signals and user-initiated media commands.
pub fn request_refresh() {
    let (pending, wake) = POLL_WAKE.get_or_init(|| (Mutex::new(false), Condvar::new()));
    if let Ok(mut pending) = pending.lock() {
        *pending = true;
        wake.notify_one();
    }
}

pub fn subscription() -> Subscription<NowPlayingData> {
    Subscription::run(|| {
        channel(8, |mut output: mpsc::Sender<NowPlayingData>| async move {
            thread::spawn(move || monitor_loop(&mut output));
            thread::spawn(watch_mpris_events);
        })
    })
}

/// Wait for MPRIS signals from the currently selected/active player. The
/// iterator blocks in D-Bus while idle, so it adds no polling work. A normal
/// polling pass remains as a fallback for players which do not emit signals.
fn watch_mpris_events() {
    loop {
        let player = PlayerFinder::new()
            .ok()
            .and_then(|finder| finder.find_all().ok())
            .and_then(find_selected_or_active_from_players);

        let Some(player) = player else {
            thread::sleep(BACKGROUND_INTERVAL);
            continue;
        };

        let Ok(mut events) = player.events() else {
            thread::sleep(BACKGROUND_INTERVAL);
            continue;
        };

        // Re-select the player after every event. A player can become active
        // or disappear between signals, and the coordinator will obtain the
        // complete fresh snapshot.
        if events.next().is_some() {
            request_refresh();
        }
    }
}

fn monitor_loop(output: &mut mpsc::Sender<NowPlayingData>) {
    let mut last_sent: Option<NowPlayingData> = None;

    loop {
        if output.is_closed() {
            break;
        }

        let popup_open = POPUP_OPEN.load(Ordering::Relaxed);
        let snapshot = now_playing_snapshot(popup_open);
        let position_is_visible = popup_open && snapshot.state == PlaybackState::Playing;
        let changed = match last_sent.as_ref() {
            None => true,
            Some(previous) if position_is_visible => previous != &snapshot,
            Some(previous) => !previous.same_except_position(&snapshot),
        };

        if changed {
            match output.try_send(snapshot.clone()) {
                Ok(()) => last_sent = Some(snapshot),
                Err(error) if error.is_disconnected() => break,
                // The UI is temporarily busy. Dropping this state is safe:
                // the next scan supplies a complete, current snapshot.
                Err(_) => {}
            }
        }

        wait_for_refresh(if popup_open {
            ACTIVE_INTERVAL
        } else {
            BACKGROUND_INTERVAL
        });
    }
}

fn wait_for_refresh(interval: Duration) {
    let (pending, wake) = POLL_WAKE.get_or_init(|| (Mutex::new(false), Condvar::new()));
    if let Ok(mut pending) = pending.lock() {
        if std::mem::take(&mut *pending) {
            return;
        }
        if let Ok((mut pending, _)) = wake.wait_timeout(pending, interval) {
            _ = std::mem::take(&mut *pending);
        }
    } else {
        thread::sleep(interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::PlaybackCapabilities;
    use mpris::LoopStatus;

    fn snapshot(position_seconds: Option<u64>) -> NowPlayingData {
        NowPlayingData {
            text: "Song • Artist".into(),
            title: "Song".into(),
            artist: "Artist".into(),
            album: String::new(),
            player_bus_name: "org.mpris.MediaPlayer2.test".into(),
            sources: vec![],
            duration_seconds: Some(180),
            position_seconds,
            capabilities: PlaybackCapabilities {
                seek: true,
                previous: true,
                next: true,
                shuffle: true,
                loop_mode: true,
            },
            shuffle: false,
            loop_status: LoopStatus::None,
            state: PlaybackState::Playing,
            album_art_path: None,
            album_art_dimensions: None,
            has_active_media: true,
        }
    }

    #[test]
    fn static_comparison_ignores_position() {
        assert!(snapshot(Some(1)).same_except_position(&snapshot(Some(2))));
    }

    #[test]
    fn static_comparison_includes_album_art_dimensions() {
        let original = snapshot(Some(1));
        let mut resized_art = original.clone();
        resized_art.album_art_dimensions = Some((16, 9));

        assert!(!original.same_except_position(&resized_art));
    }
}

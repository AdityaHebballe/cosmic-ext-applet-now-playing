//! The single owner of background MPRIS polling.
//!
//! Keeping event watching and position polling in one worker prevents the two
//! old subscriptions from racing to overwrite each other's snapshots.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use cosmic::iced::futures::channel::mpsc;
use cosmic::iced::{stream::channel, Subscription};

use crate::metadata::{now_playing_snapshot, NowPlayingData};
use crate::window::PlaybackState;

static POPUP_OPEN: AtomicBool = AtomicBool::new(false);

const ACTIVE_INTERVAL: Duration = Duration::from_secs(1);
const IDLE_INTERVAL: Duration = Duration::from_secs(3);

pub fn set_popup_open(open: bool) {
    POPUP_OPEN.store(open, Ordering::Relaxed);
}

pub fn subscription() -> Subscription<NowPlayingData> {
    Subscription::run(|| {
        channel(8, |mut output: mpsc::Sender<NowPlayingData>| async move {
            thread::spawn(move || monitor_loop(&mut output));
        })
    })
}

fn monitor_loop(output: &mut mpsc::Sender<NowPlayingData>) {
    let mut last_sent: Option<NowPlayingData> = None;

    loop {
        if output.is_closed() {
            break;
        }

        let snapshot = now_playing_snapshot();
        let popup_open = POPUP_OPEN.load(Ordering::Relaxed);
        let position_is_visible = popup_open && snapshot.state == PlaybackState::Playing;
        let active = snapshot.state == PlaybackState::Playing;
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

        thread::sleep(if popup_open || active {
            ACTIVE_INTERVAL
        } else {
            IDLE_INTERVAL
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::PlaybackCapabilities;

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
            state: PlaybackState::Playing,
            album_art_path: None,
            has_active_media: true,
        }
    }

    #[test]
    fn static_comparison_ignores_position() {
        assert!(snapshot(Some(1)).same_except_position(&snapshot(Some(2))));
    }
}

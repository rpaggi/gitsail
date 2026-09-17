//! Reads terminal input on a background thread and forwards it onto the
//! same channel background [`crate::worker::Command`]s report their
//! results on, so the main loop has exactly one place to wait (SAD §18,
//! §26: the render loop's consumer never blocks on Git process execution,
//! because Git results arrive on this same channel rather than being
//! polled for separately).

use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event;

use crate::message::Message;

/// Spawns the input-reading thread. `tick_rate` bounds how long a poll
/// waits before yielding a [`Message::Tick`], so the main loop wakes up
/// periodically even with no key pressed.
pub fn spawn(tx: Sender<Message>, tick_rate: Duration) {
    thread::spawn(move || {
        let mut last_tick = Instant::now();
        loop {
            let timeout = tick_rate.saturating_sub(last_tick.elapsed());
            let has_event = event::poll(timeout).unwrap_or(false);
            if has_event {
                match event::read() {
                    Ok(ev) => {
                        if tx.send(Message::Term(ev)).is_err() {
                            return;
                        }
                    }
                    // The terminal went away (e.g. stdin closed); nothing
                    // more to read.
                    Err(_) => return,
                }
            }
            if last_tick.elapsed() >= tick_rate {
                if tx.send(Message::Tick).is_err() {
                    return;
                }
                last_tick = Instant::now();
            }
        }
    });
}

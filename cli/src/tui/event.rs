//! Terminal event polling.

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyEvent};

/// Polls for a key event up to `timeout`, returning `Ok(None)` if none
/// arrives in time. Non-key events are swallowed so the caller only deals
/// with key presses.
pub fn poll_key(timeout: Duration) -> io::Result<Option<KeyEvent>> {
    if event::poll(timeout)? {
        if let Event::Key(key) = event::read()? {
            return Ok(Some(key));
        }
    }
    Ok(None)
}

//! Qualification view of the window lane's power latch (issue 117, X-M1 1.50).
//!
//! `composition::qualification` writes `power_held` to the stock latch at `0x16c0710` and
//! reads it back. `window.rs`'s `submit` sets it only while the window status holds power
//! and `release_power` clears it, which are the stock latch's set and clear triggers.

use super::Window;

/// The latch `window` holds.
pub fn power_held(window: &Window) -> bool { window.power_held }

/// Restores the latch a view names; the inverse of `power_held`.
pub fn restore(window: &mut Window, held: bool) { window.power_held = held; }

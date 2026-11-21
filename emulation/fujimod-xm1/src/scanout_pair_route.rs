//! Binds a route checkpoint to a presented frame. The stock role waits for the first presentation at or
//! after the checkpoint's route point; an owned role takes exactly the stock role's wait steps. The route's
//! last checkpoint also waits until no observed call is open, so both roles end between calls.

/// Wait steps a stock checkpoint may take before it is recorded as it stands.
pub const FRAME_WAIT_LIMIT: u32 = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Binding { Frame, FrameBetweenCalls }

pub trait Presenting {
    /// The digest of the frame the display presents now.
    fn presented(&mut self) -> Result<Option<String>, String>;
    fn open_calls(&self) -> usize;
    fn wait(&mut self) -> Result<(), String>;
}

/// The wait steps taken and the frame presented after them. `replay` is the stock role's step count.
pub fn bind_frame<P: Presenting>(route: &mut P, binding: Binding, replay: Option<u32>) -> Result<(u32, Option<String>), String> {
    if let Some(steps) = replay {
        for _ in 0..steps { route.wait()?; }
        return Ok((steps, route.presented()?));
    }
    let mut waits = 0;
    loop {
        let frame = route.presented()?;
        let settled = binding == Binding::Frame || route.open_calls() == 0;
        if (frame.is_some() && settled) || waits == FRAME_WAIT_LIMIT { return Ok((waits, frame)); }
        route.wait()?;
        waits += 1;
    }
}

#[cfg(test)]
#[path = "scanout_pair_route_tests.rs"]
mod tests;

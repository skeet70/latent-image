//! Binds a route checkpoint to a presented frame. The stock role waits for the first presentation at or
//! after the checkpoint's route point; an owned role takes exactly the stock role's wait steps.

/// Wait steps a stock checkpoint may take before it is recorded without a frame.
pub const FRAME_WAIT_LIMIT: u32 = 64;

pub trait Presenting {
    /// The digest of the frame the display presents now.
    fn presented(&mut self) -> Result<Option<String>, String>;
    fn wait(&mut self) -> Result<(), String>;
}

/// The wait steps taken and the frame presented after them. `replay` is the stock role's step count.
pub fn bind_frame<P: Presenting>(route: &mut P, replay: Option<u32>) -> Result<(u32, Option<String>), String> {
    if let Some(steps) = replay {
        for _ in 0..steps { route.wait()?; }
        return Ok((steps, route.presented()?));
    }
    let mut waits = 0;
    loop {
        if let Some(frame) = route.presented()? { return Ok((waits, Some(frame))); }
        if waits == FRAME_WAIT_LIMIT { return Ok((waits, None)); }
        route.wait()?;
        waits += 1;
    }
}

#[cfg(test)]
#[path = "scanout_pair_route_tests.rs"]
mod tests;

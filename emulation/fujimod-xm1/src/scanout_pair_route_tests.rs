use super::*;

/// A display that presents `frames[step]` with `open[step]` calls open after `step` wait steps, and no frame and
/// no open call past the lists.
struct Display { frames: Vec<Option<&'static str>>, open: Vec<usize>, step: usize }

impl Display {
    fn new(frames: &[Option<&'static str>]) -> Self { Self { frames: frames.to_vec(), open: Vec::new(), step: 0 } }
    fn with_open(frames: &[Option<&'static str>], open: &[usize]) -> Self { Self { open: open.to_vec(), ..Self::new(frames) } }
}

impl Presenting for Display {
    fn presented(&mut self) -> Result<Option<String>, String> {
        Ok(self.frames.get(self.step).copied().flatten().map(str::to_owned))
    }
    fn open_calls(&self) -> usize { self.open.get(self.step).copied().unwrap_or(0) }
    fn wait(&mut self) -> Result<(), String> {
        self.step += 1;
        Ok(())
    }
}

#[test]
fn stock_takes_the_frame_already_presented() {
    let mut display = Display::new(&[Some("menu")]);
    assert_eq!(bind_frame(&mut display, Binding::Frame, None), Ok((0, Some("menu".into()))));
    assert_eq!(display.step, 0);
}

#[test]
fn stock_waits_for_the_first_presented_frame() {
    let mut display = Display::new(&[None, None, Some("setup"), Some("later")]);
    assert_eq!(bind_frame(&mut display, Binding::Frame, None), Ok((2, Some("setup".into()))));
}

#[test]
fn stock_records_no_frame_at_the_wait_bound() {
    let mut display = Display::new(&[]);
    assert_eq!(bind_frame(&mut display, Binding::Frame, None), Ok((FRAME_WAIT_LIMIT, None)));
    assert_eq!(display.step, FRAME_WAIT_LIMIT as usize);
}

#[test]
fn a_frame_checkpoint_ignores_open_calls() {
    let mut display = Display::with_open(&[Some("menu")], &[1]);
    assert_eq!(bind_frame(&mut display, Binding::Frame, None), Ok((0, Some("menu".into()))));
}

#[test]
fn the_last_checkpoint_waits_until_no_call_is_open() {
    let mut display = Display::with_open(&[Some("a"), Some("b"), Some("c")], &[1, 2, 0]);
    assert_eq!(bind_frame(&mut display, Binding::FrameBetweenCalls, None), Ok((2, Some("c".into()))));
}

#[test]
fn the_last_checkpoint_stops_at_the_bound_with_a_call_open() {
    let mut display = Display::with_open(&[Some("a"); FRAME_WAIT_LIMIT as usize + 1], &[1; FRAME_WAIT_LIMIT as usize + 1]);
    assert_eq!(bind_frame(&mut display, Binding::FrameBetweenCalls, None), Ok((FRAME_WAIT_LIMIT, Some("a".into()))));
}

#[test]
fn owned_takes_the_stock_steps_even_when_a_frame_presents_earlier() {
    let mut display = Display::new(&[Some("early"), Some("middle"), Some("stock-point")]);
    assert_eq!(bind_frame(&mut display, Binding::Frame, Some(2)), Ok((2, Some("stock-point".into()))));
}

#[test]
fn owned_takes_the_stock_steps_with_its_own_calls_open() {
    let mut display = Display::with_open(&[Some("a"), Some("b")], &[1, 1]);
    assert_eq!(bind_frame(&mut display, Binding::FrameBetweenCalls, Some(1)), Ok((1, Some("b".into()))));
}

#[test]
fn owned_reports_no_frame_when_none_presents_at_the_stock_step() {
    let mut display = Display::new(&[None, None, None, Some("late")]);
    assert_eq!(bind_frame(&mut display, Binding::Frame, Some(2)), Ok((2, None)));
}

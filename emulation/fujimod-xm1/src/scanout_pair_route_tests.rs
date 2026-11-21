use super::*;

/// A display that presents `frames[step]` after `step` wait steps, and nothing past the list.
struct Display { frames: Vec<Option<&'static str>>, step: usize }

impl Display {
    fn new(frames: &[Option<&'static str>]) -> Self { Self { frames: frames.to_vec(), step: 0 } }
}

impl Presenting for Display {
    fn presented(&mut self) -> Result<Option<String>, String> {
        Ok(self.frames.get(self.step).copied().flatten().map(str::to_owned))
    }
    fn wait(&mut self) -> Result<(), String> {
        self.step += 1;
        Ok(())
    }
}

#[test]
fn stock_takes_the_frame_already_presented() {
    let mut display = Display::new(&[Some("menu")]);
    assert_eq!(bind_frame(&mut display, None), Ok((0, Some("menu".into()))));
    assert_eq!(display.step, 0);
}

#[test]
fn stock_waits_for_the_first_presented_frame() {
    let mut display = Display::new(&[None, None, Some("setup"), Some("later")]);
    assert_eq!(bind_frame(&mut display, None), Ok((2, Some("setup".into()))));
}

#[test]
fn stock_records_no_frame_at_the_wait_bound() {
    let mut display = Display::new(&[]);
    assert_eq!(bind_frame(&mut display, None), Ok((FRAME_WAIT_LIMIT, None)));
    assert_eq!(display.step, FRAME_WAIT_LIMIT as usize);
}

#[test]
fn owned_takes_the_stock_steps_even_when_a_frame_presents_earlier() {
    let mut display = Display::new(&[Some("early"), Some("middle"), Some("stock-point")]);
    assert_eq!(bind_frame(&mut display, Some(2)), Ok((2, Some("stock-point".into()))));
}

#[test]
fn owned_reports_no_frame_when_none_presents_at_the_stock_step() {
    let mut display = Display::new(&[None, None, None, Some("late")]);
    assert_eq!(bind_frame(&mut display, Some(2)), Ok((2, None)));
}

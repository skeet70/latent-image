//! What the motion layer reaches in image 4 besides configuration values and stock memory.

/// A dispatch tick count (0x7257a8).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Ticks(pub u32);

pub trait Clock {
    fn now(&mut self) -> Ticks;
    /// Ten times the ticks since `since` (0x7257e0).
    fn elapsed(&mut self, since: Ticks) -> u32;
}

/// The lens position tables image 4 keeps in its configuration.
pub trait LensTables {
    /// The first position 0x7498e0 returns for `lens` at `step` and `reference`, with no second
    /// output.
    fn channel_position(&mut self, lens: u32, step: u32, reference: u32, mode: u32) -> i16;
    /// `value` shifted by the configured offset for a change of side from `from` to `to`
    /// (0x747aac).
    fn side_adjusted(&mut self, value: i16, from: u32, to: u32) -> i16;
    /// The table offset of `delta` steps for `lens` at `step` (0x747e70).
    fn table_offset(&mut self, lens: u32, step: u32, delta: i16) -> i32;
}

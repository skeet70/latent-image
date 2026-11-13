#![forbid(unsafe_code)]

//! Which display outputs are active, from the display driver's route and status record.

/// The display driver's route byte (0x6c68c0).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplayRoute(pub u8);

/// The route on which status byte 7 gates the outputs.
pub const GATED_ROUTE: DisplayRoute = DisplayRoute(2);

pub const STATUS_BYTES: usize = 60;

/// The display driver's status record (0x6c4bec).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplayStatus(pub [u8; STATUS_BYTES]);

pub trait Display {
    fn route(&self) -> DisplayRoute;
    fn status(&self) -> DisplayStatus;
}

/// Bit k is set when status byte 16 + k is nonzero (0x6c68e8).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Outputs(pub u8);

fn outputs(display: &impl Display) -> Outputs {
    if display.route() == DisplayRoute(0) {
        return Outputs::default();
    }
    let status = display.status().0;
    Outputs(status[16..20].iter().enumerate().fold(0, |bits, (bit, byte)| if *byte != 0 { bits | 1 << bit } else { bits }))
}

/// The active outputs (0x6b7468).
pub fn active_outputs(display: &impl Display) -> Outputs {
    if display.route() == GATED_ROUTE && display.status().0[7] == 0 {
        return Outputs::default();
    }
    outputs(display)
}

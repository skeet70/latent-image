#![forbid(unsafe_code)]

//! Which display outputs are active, from the display driver's route and status record.

/// The display driver's route byte (0x6c68c0).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplayRoute(pub u8);

/// The route on which status byte 7 gates the outputs.
pub const GATED_ROUTE: DisplayRoute = DisplayRoute(2);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Outputs(pub u8);

pub trait Display {
    fn route(&self) -> DisplayRoute;
    /// Whether the status record admits outputs on the gated route (0x6b7468).
    fn gate_open(&self) -> bool;
    /// The status record's four output bytes as bits (0x6c68e8).
    fn outputs(&self) -> Outputs;
}

/// The active outputs (0x6b7468).
pub fn active_outputs(display: &impl Display) -> Outputs {
    let route = display.route();
    if route == DisplayRoute(0) || (route == GATED_ROUTE && !display.gate_open()) {
        return Outputs::default();
    }
    display.outputs()
}

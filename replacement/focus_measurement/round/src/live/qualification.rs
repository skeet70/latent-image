//! Qualification view of the live measurement child (issue 117, X-M1 1.50).
//!
//! The round and its calculation jobs are what `composition::qualification` projects onto
//! the stock machine-state halfwords at `0x169d4b4` (machines 8, 13, 24 and 29). The view
//! is built by exhaustive conversions, so a new private state fails this crate's compile.

use super::Measurement;
use crate::jobs::qualification::{self as jobs, JobView};
use crate::round::qualification::{self as round, FanView, PhaseView};
use crate::window::qualification as window;

/// The typed state the stock measurement machines read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct View {
    pub phase: PhaseView,
    pub fan: FanView,
    pub auxiliary: JobView,
    pub window: JobView,
    pub source: JobView,
    pub power_held: bool,
}

/// The view of the state `measurement` holds.
pub fn view(measurement: &Measurement) -> View {
    View {
        phase: round::phase(&measurement.round),
        fan: round::fan(&measurement.round),
        auxiliary: jobs::view(&measurement.auxiliary),
        window: jobs::view(&measurement.window),
        source: jobs::view(&measurement.source),
        power_held: window::power_held(&measurement.window_work),
    }
}

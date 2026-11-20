//! Qualification view of the live measurement child (issue 117, X-M1 1.50).
//!
//! The round and its calculation jobs are what `composition::qualification` projects onto
//! the stock machine-state halfwords at `0x169d4b4` (machines 8, 13, 24 and 29). The view
//! is built by exhaustive conversions, so a new private state fails this crate's compile.

use super::Measurement;
use crate::acquisition::qualification::StageView;
use crate::jobs::qualification::{self as jobs, JobView};
use crate::lanes::Lanes;
use crate::round::qualification::{self as round, FanView, PhaseView, PlannerView};
use crate::window::qualification as window;

/// The typed state the stock measurement machines read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct View {
    pub phase: PhaseView,
    pub fan: FanView,
    pub selected: Lanes,
    pub acquisition: StageView,
    pub first_busy: bool,
    pub second_busy: bool,
    pub auxiliary: JobView,
    pub window: JobView,
    pub source: JobView,
    pub power_held: bool,
    pub planner: PlannerView,
}

/// The view of the state `measurement` holds.
pub fn view(measurement: &Measurement) -> View {
    View {
        phase: round::phase(&measurement.round),
        fan: round::fan(&measurement.round),
        selected: round::selected(&measurement.round),
        acquisition: crate::acquisition::qualification::stage(&measurement.acquisition),
        first_busy: round::first_busy(&measurement.round),
        second_busy: round::second_busy(&measurement.round),
        auxiliary: jobs::view(&measurement.auxiliary),
        window: jobs::view(&measurement.window),
        source: jobs::view(&measurement.source),
        power_held: window::power_held(&measurement.window_work),
        planner: round::planner(&measurement.round),
    }
}

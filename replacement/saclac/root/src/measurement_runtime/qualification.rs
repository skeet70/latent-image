//! Qualification view of the measurement runtime (issue 117, X-M1 1.50).
//!
//! The round and its calculation jobs are the typed state behind stock machines 8, 13, 24
//! and 29; `round::live::qualification` performs the exhaustive conversion.

use super::MeasurementRuntime;
use xm1_focus_measurement_round::live::qualification::View;
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;

/// The view of `runtime`.
pub fn view<'s, M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, S, X>(
    runtime: &MeasurementRuntime<'s, M, R, E, P, I, T, S, X>,
) -> View {
    xm1_focus_measurement_round::live::qualification::view(&runtime.measurement)
}

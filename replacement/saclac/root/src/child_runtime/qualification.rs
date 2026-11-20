//! Qualification view of the child runtime (issue 117, X-M1 1.50).
//!
//! The children are behind `RefCell`s; the reads are `try_borrow` (read-only, non-panicking),
//! so a live mutable borrow refuses the view.

use super::ChildRuntime;
use xm1_focus_measurement_round::acquisition::qualification::StageView;
use xm1_focus_measurement_round::live::qualification::View as MeasurementView;
use xm1_saclac_lens_drive::live::qualification::StateView as LensView;
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;

/// The child state the stock machines read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct View {
    pub measurement: Option<MeasurementView>,
    pub lens_live: Option<LensView>,
    /// The S1 acquisition child's stage; machine 24 reads it while `Root::Mode` is on the S1
    /// branch (`S1`/`SuspendingForS1`), the live measurement's stage otherwise.
    pub s1_acquisition: Option<StageView>,
}

/// The view of `children`; a child that is currently mutably borrowed is `None`.
pub fn view<'s, M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, S, X>(
    children: &ChildRuntime<'s, M, R, E, P, I, T, S, X>,
) -> View {
    let measurement = children
        .measurement
        .try_borrow()
        .ok()
        .map(|measurement| crate::measurement_runtime::qualification::view(&measurement));
    let lens_live = children
        .lens
        .try_borrow()
        .ok()
        .map(|lens| xm1_saclac_lens_drive::children::qualification::live_state(&lens));
    let s1_acquisition = children
        .acquisition
        .try_borrow()
        .ok()
        .map(|acquisition| xm1_focus_measurement_round::children::qualification::stage(&acquisition));
    View { measurement, lens_live, s1_acquisition }
}

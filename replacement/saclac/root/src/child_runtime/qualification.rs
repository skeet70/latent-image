//! Qualification view of the child runtime (issue 117, X-M1 1.50).
//!
//! The measurement child is behind a `RefCell`; the read is `try_borrow_unguarded` so it
//! neither takes nor observes the borrow flag. A live borrow refuses the view.

use super::ChildRuntime;
use xm1_focus_measurement_round::live::qualification::View as MeasurementView;
use xm1_saclac_lens_drive::live::qualification::StateView as LensView;
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;

/// The child state the stock machines read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct View {
    pub measurement: Option<MeasurementView>,
    pub lens_live: Option<LensView>,
}

/// The view of `children`; a child that is currently mutably borrowed is `None`.
pub fn view<'s, M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, S, X>(
    children: &ChildRuntime<'s, M, R, E, P, I, T, S, X>,
) -> View {
    // SAFETY: read-only view of a cell the caller owns; no borrow flag is taken.
    let measurement = unsafe { children.measurement.try_borrow_unguarded() }
        .ok()
        .map(crate::measurement_runtime::qualification::view);
    // SAFETY: as above.
    let lens_live = unsafe { children.lens.try_borrow_unguarded() }
        .ok()
        .map(xm1_saclac_lens_drive::children::qualification::live_state);
    View { measurement, lens_live }
}

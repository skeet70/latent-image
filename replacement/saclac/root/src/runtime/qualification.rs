//! Qualification view of the root runtime (issue 117, X-M1 1.50).
//!
//! `composition::qualification` reads this view through the declared layout of the qualified
//! build and projects it onto the stock machine-state halfwords and the other exposed fields.
//! Every `RefCell` read is `try_borrow_unguarded`: the view is read-only and takes no borrow
//! flag. A cell that is mutably borrowed refuses the view instead of observing a panic.

use super::Runtime;
use crate::acquisition::qualification as acquisition;
use crate::child_runtime::qualification as child;
use crate::record_contexts::qualification::{self as contexts, ContextsView};
use xm1_focus_measurement_round::live::qualification::View as MeasurementView;
use xm1_saclac_controllers::live::qualification::StateView as LiveStateView;
use xm1_saclac_controllers::live_view::qualification::StateView as LiveViewControllerView;
use xm1_saclac_controllers::root::qualification::ModeView;
use xm1_saclac_interface::calc::AcquisitionGrid;
use xm1_saclac_lens_drive::live::qualification::StateView as LensView;
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;

/// The root state the qualification map reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct View {
    pub root: ModeView,
    pub live: LiveStateView,
    pub live_view: LiveViewControllerView,
    pub contexts: ContextsView,
    pub measurement: Option<MeasurementView>,
    pub lens_live: Option<LensView>,
    pub live_grid: AcquisitionGrid,
}

/// The view of `runtime`, or `None` when a cell it reads is mutably borrowed.
pub fn view<'s, M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, S, X, A, D>(
    runtime: &Runtime<'s, M, R, E, P, I, T, S, X, A, D>,
) -> Option<View> {
    // SAFETY: read-only views of cells the caller owns; no borrow flag is taken.
    let root = unsafe { runtime.root.try_borrow_unguarded() }.ok()?;
    // SAFETY: as above.
    let live = unsafe { runtime.live.try_borrow_unguarded() }.ok()?;
    // SAFETY: as above.
    let live_view = unsafe { runtime.profiles.view.try_borrow_unguarded() }.ok()?;
    // SAFETY: as above.
    let held = unsafe { runtime.contexts.try_borrow_unguarded() }.ok()?;
    // SAFETY: as above.
    let services = unsafe { runtime.services.try_borrow_unguarded() }.ok()?;
    let children = child::view(&runtime.children);
    Some(View {
        root: xm1_saclac_controllers::root::qualification::view(&root),
        live: xm1_saclac_controllers::live::qualification::view(&live),
        live_view: xm1_saclac_controllers::live_view::qualification::view(&live_view),
        contexts: contexts::view(&held),
        measurement: children.measurement,
        lens_live: children.lens_live,
        live_grid: acquisition::live_grid(&services.acquisition),
    })
}

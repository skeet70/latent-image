//! Qualification view of the root runtime (issue 117, X-M1 1.50).
//!
//! `composition::qualification` reads this view through the declared layout of the qualified
//! build and projects it onto the stock machine-state halfwords and the other exposed fields.
//! The `RefCell` reads are `try_borrow`: read-only and non-panicking, so a mutably borrowed
//! cell refuses the view instead of observing a panic. (`try_borrow_unguarded` would need
//! `unsafe`, which the modules this file lives under forbid.)

use super::Runtime;
use crate::acquisition::qualification as acquisition;
use crate::child_runtime::qualification as child;
use crate::record_contexts::qualification::{self as contexts, ContextsView};
use xm1_focus_measurement_round::acquisition::qualification::StageView;
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
    /// The S1 acquisition child's stage (machine 24 on the S1 branch).
    pub s1_acquisition: Option<StageView>,
    /// The pipeline snapshot counter at `0x016f672c`, read through Root's own
    /// `stock::acquisition::timing` accessor; it is the frame-period selector Root's
    /// `effects.frame_interval` uses.
    pub timing: u32,
}

/// The view of `runtime`, or `None` when a cell it reads is mutably borrowed.
pub fn view<'s, M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, S, X, A, D>(
    runtime: &Runtime<'s, M, R, E, P, I, T, S, X, A, D>,
) -> Option<View> {
    let root = runtime.root.try_borrow().ok()?;
    let live = runtime.live.try_borrow().ok()?;
    let live_view = runtime.profiles.view.try_borrow().ok()?;
    let held = runtime.contexts.try_borrow().ok()?;
    let services = runtime.services.try_borrow().ok()?;
    let children = child::view(&runtime.children);
    let timing = crate::stock::acquisition::timing(&services.machine).0;
    Some(View {
        root: xm1_saclac_controllers::root::qualification::view(&root),
        live: xm1_saclac_controllers::live::qualification::view(&live),
        live_view: xm1_saclac_controllers::live_view::qualification::view(&live_view),
        contexts: contexts::view(&held),
        measurement: children.measurement,
        lens_live: children.lens_live,
        live_grid: acquisition::live_grid(&services.acquisition),
        s1_acquisition: children.s1_acquisition,
        timing,
    })
}

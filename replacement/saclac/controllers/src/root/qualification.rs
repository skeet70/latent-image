//! Qualification view of the root controller's mode (issue 117, X-M1 1.50).
//!
//! `composition::qualification` maps this view onto machine 0's stock state halfword
//! (`0x169d4b4`, 11 states at `0xf1158c`). The conversion is exhaustive over the private
//! `Mode` and `Pending` enums, so a new mode fails this crate's qualification compile.

use super::{Mode, Pending, Root};

/// One variant per `Mode` value, with `Pending`'s four values flattened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModeView {
    Stopped,
    Started,
    LensStarting,
    LensStopping,
    Live,
    PendingDuringLensStart,
    PendingParameters,
    PendingAutofocus,
    PendingLensOperation,
    SuspendingForS1,
    S1,
}

/// The view of the mode `root` holds.
pub fn view(root: &Root) -> ModeView {
    match root.mode {
        Mode::Stopped => ModeView::Stopped,
        Mode::Started => ModeView::Started,
        Mode::LensStarting => ModeView::LensStarting,
        Mode::LensStopping => ModeView::LensStopping,
        Mode::Live => ModeView::Live,
        Mode::Pending(Pending::DuringLensStart) => ModeView::PendingDuringLensStart,
        Mode::Pending(Pending::Parameters) => ModeView::PendingParameters,
        Mode::Pending(Pending::Autofocus) => ModeView::PendingAutofocus,
        Mode::Pending(Pending::LensOperation) => ModeView::PendingLensOperation,
        Mode::SuspendingForS1 => ModeView::SuspendingForS1,
        Mode::S1 => ModeView::S1,
    }
}

/// Restores the mode a view names; the inverse of `view` for every view.
pub fn restore(root: &mut Root, view: ModeView) {
    root.mode = match view {
        ModeView::Stopped => Mode::Stopped,
        ModeView::Started => Mode::Started,
        ModeView::LensStarting => Mode::LensStarting,
        ModeView::LensStopping => Mode::LensStopping,
        ModeView::Live => Mode::Live,
        ModeView::PendingDuringLensStart => Mode::Pending(Pending::DuringLensStart),
        ModeView::PendingParameters => Mode::Pending(Pending::Parameters),
        ModeView::PendingAutofocus => Mode::Pending(Pending::Autofocus),
        ModeView::PendingLensOperation => Mode::Pending(Pending::LensOperation),
        ModeView::SuspendingForS1 => Mode::SuspendingForS1,
        ModeView::S1 => Mode::S1,
    };
}

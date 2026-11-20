//! Qualification view of the live router's state (issue 117, X-M1 1.50).
//!
//! `composition::qualification` maps this view onto machine 11's stock state halfword
//! (`0x169d4b4 + 2*11`, 31 states at `0xf1158c`). The nested stages are flattened so the
//! view is one variant per reachable `(State, stage, hold)` tuple; the conversion is
//! exhaustive over the private enums.

use super::{DriveStage, Hold, PrimaryStage, Router, SecondaryStage, State};

/// One variant per reachable private state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateView {
    Idle,
    PrimarySuspending,
    PrimaryLens,
    PrimaryUnpaired,
    PrimaryPaired,
    PrimaryResuming,
    SecondaryLive,
    SecondarySuspending,
    SecondaryLens,
    SecondaryResuming,
    Autofocus,
    Command23,
    Command11,
    DriveSuspendingFree,
    DrivePreparedFree,
    DriveLensFree,
    DriveResumingFree,
    DriveSuspendingHeld,
    DrivePreparedHeld,
    DriveLensHeld,
    DriveResumingHeld,
    Command14,
    Command2b,
    Command2c,
}

/// The view of the state `router` holds.
pub fn view(router: &Router) -> StateView {
    match router.state {
        State::Idle => StateView::Idle,
        State::Primary(PrimaryStage::Suspending) => StateView::PrimarySuspending,
        State::Primary(PrimaryStage::Lens) => StateView::PrimaryLens,
        State::Primary(PrimaryStage::Unpaired) => StateView::PrimaryUnpaired,
        State::Primary(PrimaryStage::Paired) => StateView::PrimaryPaired,
        State::Primary(PrimaryStage::Resuming) => StateView::PrimaryResuming,
        State::Secondary(SecondaryStage::Live) => StateView::SecondaryLive,
        State::Secondary(SecondaryStage::Suspending) => StateView::SecondarySuspending,
        State::Secondary(SecondaryStage::Lens) => StateView::SecondaryLens,
        State::Secondary(SecondaryStage::Resuming) => StateView::SecondaryResuming,
        State::Autofocus => StateView::Autofocus,
        State::Command23 => StateView::Command23,
        State::Command11 => StateView::Command11,
        State::Drive(DriveStage::Suspending, Hold::Free) => StateView::DriveSuspendingFree,
        State::Drive(DriveStage::Prepared, Hold::Free) => StateView::DrivePreparedFree,
        State::Drive(DriveStage::Lens, Hold::Free) => StateView::DriveLensFree,
        State::Drive(DriveStage::Resuming, Hold::Free) => StateView::DriveResumingFree,
        State::Drive(DriveStage::Suspending, Hold::Command11) => StateView::DriveSuspendingHeld,
        State::Drive(DriveStage::Prepared, Hold::Command11) => StateView::DrivePreparedHeld,
        State::Drive(DriveStage::Lens, Hold::Command11) => StateView::DriveLensHeld,
        State::Drive(DriveStage::Resuming, Hold::Command11) => StateView::DriveResumingHeld,
        State::Command14 => StateView::Command14,
        State::Command2b => StateView::Command2b,
        State::Command2c => StateView::Command2c,
    }
}

/// Restores the state a view names; the inverse of `view` for every view.
pub fn restore(router: &mut Router, view: StateView) {
    router.state = match view {
        StateView::Idle => State::Idle,
        StateView::PrimarySuspending => State::Primary(PrimaryStage::Suspending),
        StateView::PrimaryLens => State::Primary(PrimaryStage::Lens),
        StateView::PrimaryUnpaired => State::Primary(PrimaryStage::Unpaired),
        StateView::PrimaryPaired => State::Primary(PrimaryStage::Paired),
        StateView::PrimaryResuming => State::Primary(PrimaryStage::Resuming),
        StateView::SecondaryLive => State::Secondary(SecondaryStage::Live),
        StateView::SecondarySuspending => State::Secondary(SecondaryStage::Suspending),
        StateView::SecondaryLens => State::Secondary(SecondaryStage::Lens),
        StateView::SecondaryResuming => State::Secondary(SecondaryStage::Resuming),
        StateView::Autofocus => State::Autofocus,
        StateView::Command23 => State::Command23,
        StateView::Command11 => State::Command11,
        StateView::DriveSuspendingFree => State::Drive(DriveStage::Suspending, Hold::Free),
        StateView::DrivePreparedFree => State::Drive(DriveStage::Prepared, Hold::Free),
        StateView::DriveLensFree => State::Drive(DriveStage::Lens, Hold::Free),
        StateView::DriveResumingFree => State::Drive(DriveStage::Resuming, Hold::Free),
        StateView::DriveSuspendingHeld => State::Drive(DriveStage::Suspending, Hold::Command11),
        StateView::DrivePreparedHeld => State::Drive(DriveStage::Prepared, Hold::Command11),
        StateView::DriveLensHeld => State::Drive(DriveStage::Lens, Hold::Command11),
        StateView::DriveResumingHeld => State::Drive(DriveStage::Resuming, Hold::Command11),
        StateView::Command14 => State::Command14,
        StateView::Command2b => State::Command2b,
        StateView::Command2c => State::Command2c,
    };
}

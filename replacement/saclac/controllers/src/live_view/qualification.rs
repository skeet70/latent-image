//! Qualification view of the live view controller's state (issue 117, X-M1 1.50).
//!
//! `composition::qualification` maps this view onto machine 12's stock state halfword
//! (`0x169d4b4 + 2*12`, 25 states at `0xf1158c`). The conversion is exhaustive over the
//! private `State` enum, so a new state fails this crate's qualification compile.

use super::{Controller, State};

/// One variant per private state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateView {
    Idle,
    ResumePending,
    IdleBusy,
    Starting,
    Sequencing,
    Configuring,
    Running,
    Suspending,
    Resuming,
    ResumeRetune,
    SuspendRetune,
    Reconfiguring,
    AutofocusRetune,
    Adjusting,
    Retuning,
    RetuneSuspending,
    RetuneResuming,
    Measuring,
    Restarting,
    StoppingForRefresh,
    Refreshing,
    StoppingForAutofocus,
    RefreshingForAutofocus,
    Windowing,
    Stopping,
    Restoring,
}

/// The view of the state `controller` holds.
pub fn view(controller: &Controller) -> StateView {
    match controller.state {
        State::Idle => StateView::Idle,
        State::ResumePending => StateView::ResumePending,
        State::IdleBusy => StateView::IdleBusy,
        State::Starting => StateView::Starting,
        State::Sequencing => StateView::Sequencing,
        State::Configuring => StateView::Configuring,
        State::Running => StateView::Running,
        State::Suspending => StateView::Suspending,
        State::Resuming => StateView::Resuming,
        State::ResumeRetune => StateView::ResumeRetune,
        State::SuspendRetune => StateView::SuspendRetune,
        State::Reconfiguring => StateView::Reconfiguring,
        State::AutofocusRetune => StateView::AutofocusRetune,
        State::Adjusting => StateView::Adjusting,
        State::Retuning => StateView::Retuning,
        State::RetuneSuspending => StateView::RetuneSuspending,
        State::RetuneResuming => StateView::RetuneResuming,
        State::Measuring => StateView::Measuring,
        State::Restarting => StateView::Restarting,
        State::StoppingForRefresh => StateView::StoppingForRefresh,
        State::Refreshing => StateView::Refreshing,
        State::StoppingForAutofocus => StateView::StoppingForAutofocus,
        State::RefreshingForAutofocus => StateView::RefreshingForAutofocus,
        State::Windowing => StateView::Windowing,
        State::Stopping => StateView::Stopping,
        State::Restoring => StateView::Restoring,
    }
}

/// Restores the state a view names; the inverse of `view` for every view.
pub fn restore(controller: &mut Controller, view: StateView) {
    controller.state = match view {
        StateView::Idle => State::Idle,
        StateView::ResumePending => State::ResumePending,
        StateView::IdleBusy => State::IdleBusy,
        StateView::Starting => State::Starting,
        StateView::Sequencing => State::Sequencing,
        StateView::Configuring => State::Configuring,
        StateView::Running => State::Running,
        StateView::Suspending => State::Suspending,
        StateView::Resuming => State::Resuming,
        StateView::ResumeRetune => State::ResumeRetune,
        StateView::SuspendRetune => State::SuspendRetune,
        StateView::Reconfiguring => State::Reconfiguring,
        StateView::AutofocusRetune => State::AutofocusRetune,
        StateView::Adjusting => State::Adjusting,
        StateView::Retuning => State::Retuning,
        StateView::RetuneSuspending => State::RetuneSuspending,
        StateView::RetuneResuming => State::RetuneResuming,
        StateView::Measuring => State::Measuring,
        StateView::Restarting => State::Restarting,
        StateView::StoppingForRefresh => State::StoppingForRefresh,
        StateView::Refreshing => State::Refreshing,
        StateView::StoppingForAutofocus => State::StoppingForAutofocus,
        StateView::RefreshingForAutofocus => State::RefreshingForAutofocus,
        StateView::Windowing => State::Windowing,
        StateView::Stopping => State::Stopping,
        StateView::Restoring => State::Restoring,
    };
}

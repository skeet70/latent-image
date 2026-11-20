//! Qualification view of a calculation job's state (issue 117, X-M1 1.50).
//!
//! `composition::qualification` maps the view onto machine 24's stock state halfword
//! (`0x169d4e4`, 5 states) for the window job and machine 29's (`0x169d4ee`, 4 states) for
//! the auxiliary job. The conversion is exhaustive over the private `State` enum.

use super::{CalculationJob, State};

/// One variant per private state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobView { Disabled, Ready, Running, Stopping }

/// The view of the state `job` holds.
pub fn view(job: &CalculationJob) -> JobView {
    match job.state {
        State::Disabled => JobView::Disabled,
        State::Ready => JobView::Ready,
        State::Running => JobView::Running,
        State::Stopping => JobView::Stopping,
    }
}

/// Restores the state a view names; the inverse of `view` for every view.
pub fn restore(job: &mut CalculationJob, view: JobView) {
    job.state = match view {
        JobView::Disabled => State::Disabled,
        JobView::Ready => State::Ready,
        JobView::Running => State::Running,
        JobView::Stopping => State::Stopping,
    };
}

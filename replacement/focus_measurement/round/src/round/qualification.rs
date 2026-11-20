//! Qualification view of the measurement round's phase and fan (issue 117, X-M1 1.50).
//!
//! `composition::qualification` maps the phase onto machine 13's stock state halfword
//! (`0x169d4b4 + 2*13`, 6 states at `0xf1158c`) and the fan onto machine 8's
//! (`0x169d4c4`, 3 states). Both conversions are exhaustive over the private enums.

use super::{Fan, Phase, Round};
use crate::lanes::Lanes;
use crate::planner::Slot;

/// One variant per `Phase` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhaseView { Dormant, Idle, Armed, Running, Stopping }

/// One variant per `Fan` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FanView { Ready, Draining, Stopping }

/// The view of the phase `round` holds.
pub fn phase(round: &Round) -> PhaseView {
    match round.phase {
        Phase::Dormant => PhaseView::Dormant,
        Phase::Idle => PhaseView::Idle,
        Phase::Armed => PhaseView::Armed,
        Phase::Running => PhaseView::Running,
        Phase::Stopping => PhaseView::Stopping,
    }
}

/// The view of the fan `round` holds.
pub fn fan(round: &Round) -> FanView {
    match round.fan {
        Fan::Ready => FanView::Ready,
        Fan::Draining => FanView::Draining,
        Fan::Stopping => FanView::Stopping,
    }
}

/// The lanes `round` has selected; the round's category-pair decision.
pub fn selected(round: &Round) -> Lanes { round.selected }

/// Whether the planner's first pair slot is busy (the stock `0x16be047` done byte is zero
/// exactly when it is not).
pub fn first_busy(round: &Round) -> bool { round.planner.busy(Slot::First) }

/// Whether the planner's second pair slot is busy (the stock `0x16be048` done byte).
pub fn second_busy(round: &Round) -> bool { round.planner.busy(Slot::Second) }

/// Restores the phase a view names; the inverse of `phase` for every view.
pub fn restore_phase(round: &mut Round, view: PhaseView) {
    round.phase = match view {
        PhaseView::Dormant => Phase::Dormant,
        PhaseView::Idle => Phase::Idle,
        PhaseView::Armed => Phase::Armed,
        PhaseView::Running => Phase::Running,
        PhaseView::Stopping => Phase::Stopping,
    };
}

/// Restores the fan a view names; the inverse of `fan` for every view.
pub fn restore_fan(round: &mut Round, view: FanView) {
    round.fan = match view {
        FanView::Ready => Fan::Ready,
        FanView::Draining => Fan::Draining,
        FanView::Stopping => Fan::Stopping,
    };
}

//! Qualification view of the live lens child's state (issue 117, X-M1 1.50).
//!
//! `composition::qualification` maps this view onto machine 1's stock state halfword
//! (`0x169d4b4 + 2*1`, 14 states at `0xf1158c`). The conversion is exhaustive over the
//! private `State` enum; the settings payload is flattened because it is not a state index.

use super::{LiveLens, State};

/// One variant per private state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateView { Idle, Prepared, Profile, Settings, Driving, Stepping, Selecting }

/// The view of the state `lens` holds.
pub fn view(lens: &LiveLens) -> StateView {
    match lens.state {
        State::Idle => StateView::Idle,
        State::Prepared => StateView::Prepared,
        State::Profile => StateView::Profile,
        State::Settings(_) => StateView::Settings,
        State::Driving => StateView::Driving,
        State::Stepping => StateView::Stepping,
        State::Selecting => StateView::Selecting,
    }
}

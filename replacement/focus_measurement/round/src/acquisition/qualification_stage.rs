//! Qualification view of the acquisition machine's stage (issue 117, X-M1 1.50).
//!
//! Machine 24 is calc1 0x3d00/0x3d01 and imgdc 3f2c/3f2d: the cell table at `0xf11a04` moves
//! state 1 -> 3 on event 0xc8 (row 3, action `0xf0de30` = the 0x3d00 producer) and 1 -> 2 on
//! 0xc9 (row 4, action `0xf0de44` = the 0x3d01 producer). The acquisition's `Primary` stage is
//! the 0x3d00 request and `Secondary` the 0x3d01 request, so this view is the stock state's
//! counterpart. The stock state 4 (a start while a request is in flight) has no acquisition
//! stage and is a gap.

use super::{Acquisition, Stage};

/// One variant per reachable stage; `Primary(Pass)` is flattened because the stock state does
/// not distinguish the pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StageView {
    Disabled,
    Ready,
    Primary,
    Secondary,
    StoppingPrimary,
    StoppingSecondary,
}

/// The view of the stage `acquisition` holds.
pub fn stage(acquisition: &Acquisition) -> StageView {
    match acquisition.stage {
        Stage::Disabled => StageView::Disabled,
        Stage::Ready => StageView::Ready,
        Stage::Primary(_) => StageView::Primary,
        Stage::Secondary => StageView::Secondary,
        Stage::StoppingPrimary => StageView::StoppingPrimary,
        Stage::StoppingSecondary => StageView::StoppingSecondary,
    }
}

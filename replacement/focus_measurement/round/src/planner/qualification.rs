//! Qualification view of the planner (issue 117, X-M1 1.50).
//!
//! The stock adjustment sequence keeps the planner in four words: the second-stage cursor at
//! `0x016d8778`, the mode at `0x016d877c`, the phase at `0x016d8ff0` and the other cursor at
//! `0x016d8ff1` (`tu_00fa9588.c:415-487`, `FUN_00fa992c`). The mode and phase conversions are
//! exhaustive over the private enums, so a new mode or phase fails this crate's qualification
//! compile.

use super::{Mode, Phase, Planner};

/// One variant per `Mode` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModeView { Default, RecordSelected, ProfileSelected, AboveThreshold, BelowThreshold }

/// One variant per `Phase` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhaseView { First, Second, Third, Fourth }

/// The planner state the stock adjustment sequence reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct View {
    pub first_cursor: usize,
    pub second_cursor: usize,
    pub phase: PhaseView,
    pub mode: ModeView,
}

/// The view of the planner `planner` holds.
pub fn view(planner: &Planner) -> View {
    View {
        first_cursor: planner.first_cursor,
        second_cursor: planner.second_cursor,
        phase: phase(planner.phase),
        mode: mode(planner.mode),
    }
}

/// The stock phase index: `First..Fourth` is `0..3` (`FUN_00fa992c` wraps above 3,
/// `tu_00fa9588.c:482-484`).
pub fn phase(phase: Phase) -> PhaseView {
    match phase {
        Phase::First => PhaseView::First,
        Phase::Second => PhaseView::Second,
        Phase::Third => PhaseView::Third,
        Phase::Fourth => PhaseView::Fourth,
    }
}

/// The stock mode word: the index into the seven-byte cadence row at `0xfab110` that
/// `FUN_00fa72bc` (`tu_00fa6bb4.c`) selects with. The row maps index 0 to the `Default`
/// property (`base + 5`), 1 to `RecordSelected` (`base + 0`), 3 to `ProfileSelected`
/// (`base + 2`), 4 to `AboveThreshold` (`base + 3`) and 5 to `BelowThreshold` (`base + 4`)
/// (`stock.rs:61-79`); index 2 is the third selection test of `FUN_00fa992c`
/// (`tu_00fa9588.c:415-451`) and has no Root mode, and 6 is the never-stored disabled value
/// the same function tests at `:459`.
pub fn mode(mode: Mode) -> ModeView {
    match mode {
        Mode::Default => ModeView::Default,
        Mode::RecordSelected => ModeView::RecordSelected,
        Mode::ProfileSelected => ModeView::ProfileSelected,
        Mode::AboveThreshold => ModeView::AboveThreshold,
        Mode::BelowThreshold => ModeView::BelowThreshold,
    }
}

//! Root stock-encoding qualification map (issue 117, X-M1 1.50).
//!
//! Compiled only with `--cfg saclac_qualification`; never part of a production artifact.
//! This module holds the field table, the per-machine encoders and decoders and the overlay
//! codec so the map is host-testable. `stock::composition::qualification` holds the ABI
//! entries that read `OWNER` and call this module.
//!
//! Every encoder matches a crate-level qualification view exhaustively, with no wildcard
//! arm, so a new Root state fails the qualification build.
//!
//! ## Machine map (declared)
//!
//! | machine | stock states (`0xf1158c`) | wake row (event 0x4b) | Root view |
//! | ---: | ---: | ---: | --- |
//! | 0 | 11 | 47 | `controllers::root::ModeView` (bijection) |
//! | 1 | 14 | 19 | `lens_drive::live::StateView` (7 of 14) |
//! | 8 | 3 | 27 | `round::FanView` (bijection) |
//! | 11 | 31 | 20 | `controllers::live::StateView` (24 of 31) |
//! | 12 | 25 | 46 | `controllers::live_view::StateView` (25 of 26, `Restoring` shares 24) |
//! | 13 | 6 | 15 | `round::PhaseView` (5 of 6) |
//! | 24 | 5 | 7 | `round::acquisition::StageView` (4 of 5), the S1 child's while `ModeView` is `S1`/`SuspendingForS1`, the live measurement's otherwise |
//! | 29 | 4 | 5 | auxiliary `round::JobView` (bijection) |
//!
//! The rows are the machines' event-row maps for code 0x4b
//! (`power_lifecycle::record_task43::event_row`). At those rows machines 1, 8, 24 and 29
//! select one constant action for every state (id5 `0xf03464`, `0xf05d5c`, `0xf0ded8` and
//! `0xf0f78c`, each `mov r0,#3`), so their state is read but does not change the step's
//! behaviour. Machine 0's eleven cells select eleven different actions (cells 517..527 of
//! `0xf11bfc`), machine 11's thirty-one cells select thirty-one (`0xf13728`, row 20),
//! machine 12's twenty-five select eight distinct actions (`0xf108c0`, row 46) and machine
//! 13's six select three (`0xf12134`, row 15). All cells' second bytes are 1, the null
//! second-table word, so the wake step never writes a state halfword.
//!
//! ## Field table
//!
//! `QUALIFICATION_FIELDS` lists the exposed `saclac_private` ranges
//! (`inputs/stock-step-exposed.json`, 1269 bytes) and the fields the step's other writes
//! reach (the shared profile state, the statistics descriptor and the adjustment sequence) in
//! overlay order, 1437 bytes. The eight machine-state halfwords are fields of their own. The
//! grid is two ranges because bytes 0..8 and 0x458 are written by the step before it reads
//! them. The 2760-byte area-statistics entries at `0x1ffee718` are not a field: the step
//! writes them before reading them in every captured cycle (0 exposed bytes), only stock
//! saclac consumes the copy, and Root retains no counterpart; as stock-private scratch
//! outside the fields they leave the abstract comparison.

use core::ptr;
use xm1_focus_measurement_round::acquisition::qualification::StageView;
use xm1_focus_measurement_round::jobs::qualification::JobView;
use xm1_focus_measurement_round::lanes::{Lane, Lanes};
use xm1_focus_measurement_round::round::qualification::{FanView, PhaseView, PlannerModeView, PlannerPhaseView};
use xm1_saclac_controllers::live::qualification::StateView as LiveView;
use xm1_saclac_controllers::live_view::qualification::StateView as LiveViewControllerView;
use xm1_saclac_controllers::root::qualification::ModeView;
use xm1_saclac_interface::stock::peers;
use xm1_saclac_lens_drive::live::qualification::StateView as LensView;
use crate::runtime::qualification::View as RuntimeView;

// ---------------------------------------------------------------------------
// Field table

/// The record handle word (`s53c_record_handles.words[2]`).
pub const FIELD_HANDLE: usize = 0;
/// Machine 0's state halfword; the other machines follow in `MACHINES` order.
pub const FIELD_MACHINE0: usize = 1;
pub const FIELD_MASK: usize = 9;
pub const FIELD_STAGE_FIRST: usize = 10;
pub const FIELD_STAGE_SECOND: usize = 11;
pub const FIELD_GRID_HEAD: usize = 12;
pub const FIELD_GRID_TAIL: usize = 13;
pub const FIELD_RECORD_HOLE0: usize = 14;
pub const FIELD_LATCH: usize = 20;
pub const FIELD_QUEUE: usize = 21;
/// The 132-byte shared profile state at `0x0091904c` (`ProfileRecords`' `FOCUS_RESULT`, read by
/// `0x00756734`: 132 bytes copied from `0x0091904c`).
pub const FIELD_FOCUS_RESULT: usize = 22;
/// `AreaStatistics.present` at `0x009190f0` (descriptor word 0).
pub const FIELD_PRESENT: usize = 23;
/// `AreaStatistics.layout` at `0x009190f4` (descriptor byte 4).
pub const FIELD_LAYOUT: usize = 24;
/// The entries pointer at `0x009190f8` (descriptor word 2), the uncached alias
/// `0x5ffee718` of `0x1ffee718`.
pub const FIELD_ENTRIES_POINTER: usize = 25;
/// The descriptor's word 3 at `0x009190fc`; no reader uses it and stock leaves it zero.
pub const FIELD_DESCRIPTOR_WORD3: usize = 26;
/// The adjustment block's word at `0x016d8708` (`blk_016d8708` offset 0). It is a constant of
/// the stock artifact: the block's getter `0xfa7f00` reads it and compares it with 1
/// (`id5.dis:965365-965368`), but no store reaches the word. Every `movw #0x8708` / `movt
/// #0x16d` sequence in `id5.dis` is one of the block's accessors and stores only at `+0x04`,
/// `+0x08`, `+0x40`, `+0x44`, `+0x48`, `+0x4c`, `+0x54` or `+0x58`; `id4.dis` has no
/// `#0x8708` at all, and neither image has a literal-pool `0x016d8708`. The word is 0 in all
/// 57 dumps, so it cannot take another value and is not Root state.
pub const FIELD_ADJUSTMENT_00: usize = 27;
/// The policy word at `0x016d8748` (block offset 0x40). Its only in-step read is
/// `s54y_select_policy` (`0xfa992c`) at `0xfa99f4` (`id5.dis:967091`), which passes it to
/// `FUN_00fa95a8` (`0xfa95a8` -> `z_read_profile_period`, `0x1005f60`) and uses the returned
/// period only as a nonzero gate for the timer/threshold comparison that selects modes 4/5
/// (`tu_00fa9588.c:441-448`). The word is the profile-period selector, not the band itself,
/// so a band-preserving canonical word is not established; the field refuses. Its writers are
/// the exposure-info initialisers `s54_initialize_fixed/auxiliary/continuous_exposure_info`
/// (`0xf33a18`/`0xf33ef4`/`0xf34710`) and `FUN_00f35d14`, each copying the exposure-info
/// struct's `+0x14` (`id5.dis:846983,847458,847873,849398`), so it is configuration; all 57
/// dumps hold 12. Root's same decision is `planned_mode` (`live.rs:206-212`) from
/// `effects.frame_interval(...)` and `frame_rate_threshold()`; Root does not retain this
/// cached selector.
pub const FIELD_CLASSIFICATION: usize = 28;
/// The adjustment block's word at `0x016d8750` (block offset 0x48). `FUN_00f308d4` sets it to
/// 1 or 0 beside its writes of the shared round words `AT_0140`/`AT_0144`/`AT_0148`
/// (`0x756a0c`/`0x756a54`/`0x756a9c`; `id5.dis:843828-843831`), and `FUN_00f30f64`,
/// `FUN_00f33094` (the measurement controller's row function) and others read it
/// (`id5.dis:844141,846183,847211`). All 57 dumps hold 0. Root has no retained word for this
/// profile-selection flag; `Round.selected` is the category-pair decision, not it.
pub const FIELD_ADJUSTMENT_48: usize = 29;
/// The adjustment block's word at `0x016d8760` (block offset 0x58). `FUN_00ef9930` sets it to
/// 1 (`id5.dis:787434`) from the record/reccont module (`0xe20c4c`, `0xe219c8`, `0xe39050`),
/// and `FUN_00f3a564` reads and clears it (`id5.dis:853713,853739`). All 57 pre-state dumps
/// hold 0. Root has no persistent counterpart: its `MeasurementUpdate`/`RecordEffect`
/// handoffs are created and consumed within one dispatch, not a flag a later step reads.
pub const FIELD_ADJUSTMENT_58: usize = 30;
/// `Planner.first_cursor` at `0x016d8778` (`tu_00fa9588.c:415,462-465,480`).
pub const FIELD_FIRST_CURSOR: usize = 31;
/// `Planner.mode` at `0x016d877c` (`tu_00fa9588.c:418-451,459-473`; `round/stock.rs:61-79`).
pub const FIELD_MODE: usize = 32;
/// `Planner.phase` at `0x016d8ff0` (`tu_00fa9588.c:463,473,482-484`, `First..Fourth` = 0..3).
pub const FIELD_PHASE: usize = 33;
/// `Planner.second_cursor` at `0x016d8ff1` (`tu_00fa9588.c:415,472-475,481`).
pub const FIELD_SECOND_CURSOR: usize = 34;
/// The number of mapped fields.
pub const FIELD_COUNT: usize = 35;

/// The eight machines the wake step reads, in the overlay's machine-field order.
pub const MACHINES: [u32; 8] = [0, 1, 8, 11, 12, 13, 24, 29];

/// Each machine's state count, `s53c_saclac_state_counts` at `0xf1158c`.
pub const STATE_COUNTS: [u16; 8] = [11, 14, 3, 31, 25, 6, 5, 4];

/// The overlay byte offsets, in field order.
pub const OFF_HANDLE: usize = 0;
pub const OFF_MACHINE0: usize = 4;
pub const OFF_MASK: usize = 20;
pub const OFF_STAGE_FIRST: usize = 21;
pub const OFF_STAGE_SECOND: usize = 22;
pub const OFF_GRID_HEAD: usize = 23;
pub const OFF_GRID_TAIL: usize = 1127;
pub const OFF_RECORD_HOLE0: usize = 1250;
pub const OFF_LATCH: usize = 1261;
pub const OFF_QUEUE: usize = 1265;
pub const OFF_FOCUS_RESULT: usize = 1269;
pub const OFF_PRESENT: usize = 1401;
pub const OFF_LAYOUT: usize = 1405;
pub const OFF_ENTRIES_POINTER: usize = 1406;
pub const OFF_DESCRIPTOR_WORD3: usize = 1410;
pub const OFF_ADJUSTMENT_00: usize = 1414;
pub const OFF_CLASSIFICATION: usize = 1418;
pub const OFF_ADJUSTMENT_48: usize = 1422;
pub const OFF_ADJUSTMENT_58: usize = 1426;
pub const OFF_FIRST_CURSOR: usize = 1430;
pub const OFF_MODE: usize = 1431;
pub const OFF_PHASE: usize = 1435;
pub const OFF_SECOND_CURSOR: usize = 1436;
/// The overlay's byte length.
pub const OVERLAY_BYTES: usize = 1437;

/// The stock's uncached alias of the area-statistics entries buffer.
pub const AREA_ENTRIES_POINTER: u32 = 0x5ffee7_18;

/// `(stock address, byte length)` per mapped field, in overlay order.
pub const QUALIFICATION_FIELDS: [[u32; 2]; FIELD_COUNT] = [
    [0x0169_cf08, 4],
    [0x0169_d4b4, 2],
    [0x0169_d4b6, 2],
    [0x0169_d4c4, 2],
    [0x0169_d4ca, 2],
    [0x0169_d4cc, 2],
    [0x0169_d4ce, 2],
    [0x0169_d4e4, 2],
    [0x0169_d4ee, 2],
    [0x016b_e044, 1],
    [0x016b_e047, 1],
    [0x016b_e048, 1],
    [0x016b_f378, 1104],
    [0x016b_f7c9, 123],
    [0x016b_fb8e, 2],
    [0x016b_fba6, 2],
    [0x016b_fbae, 2],
    [0x016b_fbbe, 2],
    [0x016b_fbf5, 1],
    [0x016b_fbfa, 2],
    [0x016c_0710, 4],
    [0x016c_236c, 4],
    [0x0091_904c, 132],
    [0x0091_90f0, 4],
    [0x0091_90f4, 1],
    [0x0091_90f8, 4],
    [0x0091_90fc, 4],
    [0x016d_8708, 4],
    [0x016d_8748, 4],
    [0x016d_8750, 4],
    [0x016d_8760, 4],
    [0x016d_8778, 1],
    [0x016d_877c, 4],
    [0x016d_8ff0, 1],
    [0x016d_8ff1, 1],
];

/// The grid ranges' offsets inside the 1236-byte block at `0x16bf370`.
const GRID_HEAD: core::ops::Range<usize> = 8..1112;
const GRID_TAIL: core::ops::Range<usize> = 1113..1236;

/// The record's never-written holes inside the 168-byte block at `0x16bfb88`.
const RECORD_HOLES: [(usize, usize); 6] = [(6, 2), (0x1e, 2), (0x26, 2), (0x36, 2), (0x6d, 1), (0x72, 2)];

// ---------------------------------------------------------------------------
// Per-machine encoders: exhaustive over the Root view, no wildcard arm.

/// Machine 0: the root shell's mode. Stock state 0 is the start/stop idle state (its wake
/// cell runs the fault action `0xf01d1c`), 1 is started, 4 is the live branch's forwarded
/// command and 9/10 are the S1 branch (`0xf02dc4`, `0xf02e50`).
pub fn encode_m0(view: ModeView) -> u16 {
    match view {
        ModeView::Stopped => 0,
        ModeView::Started => 1,
        ModeView::PendingDuringLensStart => 2,
        ModeView::PendingParameters => 3,
        ModeView::Live => 4,
        ModeView::PendingAutofocus => 5,
        ModeView::PendingLensOperation => 6,
        ModeView::LensStarting => 7,
        ModeView::LensStopping => 8,
        ModeView::SuspendingForS1 => 9,
        ModeView::S1 => 10,
    }
}

/// Machine 1: the live lens child. Its wake row's action is constant, so this projection is
/// representational; the remaining seven stock states have no typed counterpart.
pub fn encode_m1(view: LensView) -> u16 {
    match view {
        LensView::Idle => 0,
        LensView::Prepared => 1,
        LensView::Profile => 2,
        LensView::Settings => 3,
        LensView::Driving => 4,
        LensView::Stepping => 5,
        LensView::Selecting => 6,
    }
}

/// Machine 8: the measurement fan, one variant per stock state.
pub fn encode_m8(view: FanView) -> u16 {
    match view {
        FanView::Ready => 0,
        FanView::Draining => 1,
        FanView::Stopping => 2,
    }
}

/// Machine 11: the live router. Its row-20 cells drive the record pairs; the seven stock
/// states above the view's twenty-four have no typed counterpart.
pub fn encode_m11(view: LiveView) -> u16 {
    match view {
        LiveView::Idle => 0,
        LiveView::PrimarySuspending => 1,
        LiveView::PrimaryLens => 2,
        LiveView::PrimaryUnpaired => 3,
        LiveView::PrimaryPaired => 4,
        LiveView::PrimaryResuming => 5,
        LiveView::SecondaryLive => 6,
        LiveView::SecondarySuspending => 7,
        LiveView::SecondaryLens => 8,
        LiveView::SecondaryResuming => 9,
        LiveView::Autofocus => 10,
        LiveView::Command23 => 11,
        LiveView::Command11 => 12,
        LiveView::DriveSuspendingFree => 13,
        LiveView::DrivePreparedFree => 14,
        LiveView::DriveLensFree => 15,
        LiveView::DriveResumingFree => 16,
        LiveView::DriveSuspendingHeld => 17,
        LiveView::DrivePreparedHeld => 18,
        LiveView::DriveLensHeld => 19,
        LiveView::DriveResumingHeld => 20,
        LiveView::Command14 => 21,
        LiveView::Command2b => 22,
        LiveView::Command2c => 23,
    }
}

/// Machine 12: the live view controller. `Stopping` and `Restoring` share stock state 24,
/// the last of the machine's twenty-five; the stock state cannot distinguish them.
pub fn encode_m12(view: LiveViewControllerView) -> u16 {
    match view {
        LiveViewControllerView::Idle => 0,
        LiveViewControllerView::ResumePending => 1,
        LiveViewControllerView::IdleBusy => 2,
        LiveViewControllerView::Starting => 3,
        LiveViewControllerView::Sequencing => 4,
        LiveViewControllerView::Configuring => 5,
        LiveViewControllerView::Running => 6,
        LiveViewControllerView::Suspending => 7,
        LiveViewControllerView::Resuming => 8,
        LiveViewControllerView::ResumeRetune => 9,
        LiveViewControllerView::SuspendRetune => 10,
        LiveViewControllerView::Reconfiguring => 11,
        LiveViewControllerView::AutofocusRetune => 12,
        LiveViewControllerView::Adjusting => 13,
        LiveViewControllerView::Retuning => 14,
        LiveViewControllerView::RetuneSuspending => 15,
        LiveViewControllerView::RetuneResuming => 16,
        LiveViewControllerView::Measuring => 17,
        LiveViewControllerView::Restarting => 18,
        LiveViewControllerView::StoppingForRefresh => 19,
        LiveViewControllerView::Refreshing => 20,
        LiveViewControllerView::StoppingForAutofocus => 21,
        LiveViewControllerView::RefreshingForAutofocus => 22,
        LiveViewControllerView::Windowing => 23,
        LiveViewControllerView::Stopping => 24,
        LiveViewControllerView::Restoring => 24,
    }
}

/// Machine 13: the measurement round's phase. Stock state 5 has no typed counterpart.
pub fn encode_m13(view: PhaseView) -> u16 {
    match view {
        PhaseView::Dormant => 0,
        PhaseView::Idle => 1,
        PhaseView::Armed => 2,
        PhaseView::Running => 3,
        PhaseView::Stopping => 4,
    }
}

/// Machine 24: the acquisition machine. The cell table at `0xf11a04` writes 3 on event
/// 0xc8 (row 3, action `0xf0de30` = the 0x3d00 producer) and 2 on 0xc9 (row 4, action
/// `0xf0de44` = the 0x3d01 producer); the acquisition's `Primary` is the 0x3d00 request and
/// `Secondary` the 0x3d01 request. Stock state 4 (a start while a request is in flight) has
/// no acquisition stage and is refused on decode.
///
/// Root runs two acquisitions: the live branch's (`Measurement.acquisition`) and the S1
/// branch's (`ChildRuntime.acquisition`, `S1Acquisition`). `Root::Mode` selects which one the
/// stock machine 24 is following (`controllers/src/root.rs:109-320`): `S1` and
/// `SuspendingForS1` are the S1 branch (the half-press suspend sets `SuspendingForS1` and the
/// `BeginS1` completion sets `S1`), every other mode the live branch.
pub fn encode_m24(mode: ModeView, live: StageView, s1: StageView) -> u16 {
    match mode {
        ModeView::S1 => encode_stage(s1),
        ModeView::SuspendingForS1 => encode_stage(s1),
        ModeView::Stopped => encode_stage(live),
        ModeView::Started => encode_stage(live),
        ModeView::LensStarting => encode_stage(live),
        ModeView::LensStopping => encode_stage(live),
        ModeView::Live => encode_stage(live),
        ModeView::PendingDuringLensStart => encode_stage(live),
        ModeView::PendingParameters => encode_stage(live),
        ModeView::PendingAutofocus => encode_stage(live),
        ModeView::PendingLensOperation => encode_stage(live),
    }
}

/// The acquisition stage's stock index, shared by both branches.
pub fn encode_stage(view: StageView) -> u16 {
    match view {
        StageView::Disabled => 0,
        StageView::Ready => 1,
        StageView::Primary => 3,
        StageView::Secondary => 2,
        StageView::StoppingPrimary => 4,
        StageView::StoppingSecondary => 4,
    }
}

/// Machine 29: the auxiliary calculation job, one variant per stock state.
pub fn encode_m29(view: JobView) -> u16 {
    match view {
        JobView::Disabled => 0,
        JobView::Ready => 1,
        JobView::Running => 2,
        JobView::Stopping => 3,
    }
}

// ---------------------------------------------------------------------------
// The adjustment sequence's planner words and the shared area statistics.

/// The stock mode word at `0x016d877c`: the index into the seven-byte cadence row at
/// `0xfab110` that `FUN_00fa72bc` selects with (`tu_00fa6bb4.c`; `round/stock.rs:61-79` maps
/// Root's modes to the row's property keys). The table row maps index 0 to the `Default`
/// key (`base + 5`), 1 to `RecordSelected` (`base + 0`), 3 to `ProfileSelected`
/// (`base + 2`), 4 to `AboveThreshold` (`base + 3`) and 5 to `BelowThreshold` (`base + 4`).
/// Index 2 is the third selection test of `FUN_00fa992c` (`tu_00fa9588.c:415-451`) and has
/// no Root mode; 6 is the never-stored disabled value the same function tests.
pub fn encode_mode(view: PlannerModeView) -> u32 {
    match view {
        PlannerModeView::Default => 0,
        PlannerModeView::RecordSelected => 1,
        PlannerModeView::ProfileSelected => 3,
        PlannerModeView::AboveThreshold => 4,
        PlannerModeView::BelowThreshold => 5,
    }
}

/// Decodes the stock mode word; indices 2 and 6 have no Root mode (gap G12).
pub fn decode_mode(word: u32) -> Result<PlannerModeView, u32> {
    match word {
        0 => Ok(PlannerModeView::Default),
        1 => Ok(PlannerModeView::RecordSelected),
        3 => Ok(PlannerModeView::ProfileSelected),
        4 => Ok(PlannerModeView::AboveThreshold),
        5 => Ok(PlannerModeView::BelowThreshold),
        _ => Err(1),
    }
}

/// The stock phase index at `0x016d8ff0`: `First..Fourth` is `0..3`; `FUN_00fa992c` wraps
/// above 3 (`tu_00fa9588.c:482-484`).
pub fn encode_planner_phase(view: PlannerPhaseView) -> u8 {
    match view {
        PlannerPhaseView::First => 0,
        PlannerPhaseView::Second => 1,
        PlannerPhaseView::Third => 2,
        PlannerPhaseView::Fourth => 3,
    }
}

/// Decodes the stock phase index; 4 and above never occur (`tu_00fa9588.c:483-484`).
pub fn decode_planner_phase(byte: u8) -> Result<PlannerPhaseView, u32> {
    match byte {
        0 => Ok(PlannerPhaseView::First),
        1 => Ok(PlannerPhaseView::Second),
        2 => Ok(PlannerPhaseView::Third),
        3 => Ok(PlannerPhaseView::Fourth),
        _ => Err(1),
    }
}

/// A planner cursor is a byte at `0x016d8778`/`0x016d8ff1` (`ldrb`/`strb`,
/// `tu_00fa9588.c` and `id5.dis:967054,967059,967189,967192`); the planner's orders are four
/// and three entries, so a cursor above `u8::MAX` is outside the stock domain.
pub fn encode_cursor(cursor: usize) -> Result<u8, u32> {
    if cursor > usize::from(u8::MAX) { Err(1) } else { Ok(cursor as u8) }
}

/// Decodes a planner cursor byte.
pub const fn decode_cursor(byte: u8) -> usize { byte as usize }

/// The descriptor's third word: `0x5ffee718` (the uncached alias of the entries at
/// `0x1ffee718`) while the statistics are present, zero while absent. The measured stock
/// wakes hold `0` or `0x5ffee718` in the word's first byte's sense: 0 or 24 at
/// `0x009190f8`'s first byte (`wake-0000..0029`).
pub const fn entries_pointer(present: u32) -> u32 {
    if present == 0 { 0 } else { AREA_ENTRIES_POINTER }
}

/// Checks a stock pointer word against the presence word.
pub fn decode_entries_pointer(present: u32, pointer: u32) -> Result<(), u32> {
    if pointer == entries_pointer(present) { Ok(()) } else { Err(1) }
}

// ---------------------------------------------------------------------------
// The selection-adjustment mask: Root's category-pair decision to the stock bits.

/// The stock mask bit one Root lane stands for. The bits are the category pairs
/// `tu_00f04788.c:32-98` reads: bit0 First/Second (`m24`, the acquisition's calc1
/// 0x3d00/0x3d01 + imgdc 3f2c/3f2d, Root completion 0x87/0x88), bit1 Mode (`m33`, the
/// AF-search coordinator), bit2 Primary (`m14`, calc1 0x3d02, the auxiliary lane's
/// operation), bit3 Auxiliary (`m9`, imgdc 3f30..33 + calc 0x3c0a, Root image completions
/// 0x47/0x66/0x8b), bit4 Output (`m10`, bookkeeping), bit5 Secondary (`m29`, calc 0x3c0c,
/// `round_services.rs:80-82` posts completion 0x8d) and bit6 Tertiary (`m30`, calc 0x3c11,
/// `round_services.rs:88-90` posts completion 0x9a).
pub fn lane_mask(lane: Lane) -> u8 {
    match lane {
        Lane::Acquisition => 0x01,
        Lane::Search => 0x02,
        Lane::Auxiliary => 0x04,
        Lane::Image => 0x08,
        Lane::Bookkeeping => 0x10,
        Lane::Window => 0x20,
        Lane::Source => 0x40,
    }
}

/// Encodes the selected lanes as the stock mask; an empty selection is zero.
pub fn encode_mask(lanes: Lanes) -> u8 {
    let mut mask = 0;
    for lane in xm1_focus_measurement_round::lanes::ORDER {
        if lanes.contains(lane) { mask |= lane_mask(lane); }
    }
    mask
}

/// Decodes a stock mask into the lanes it names. Bits above bit6 have no Root counterpart.
pub fn decode_mask(mask: u8) -> Result<Lanes, u32> {
    if mask & 0x80 != 0 { return Err(1); }
    let mut lanes = Lanes::empty();
    for lane in xm1_focus_measurement_round::lanes::ORDER {
        if mask & lane_mask(lane) != 0 { lanes.insert(lane); }
    }
    Ok(lanes)
}

/// The pair done byte's only distinction in the wake step is zero/nonzero (see the field
/// table); the encoder writes one canonical nonzero value for a busy slot.
pub const fn stage_byte(busy: bool) -> u8 { if busy { 0x01 } else { 0 } }

/// Whether a pair done byte says the slot is busy/done.
pub const fn stage_busy(byte: u8) -> bool { byte != 0 }

// ---------------------------------------------------------------------------
// Decoders: a stock index to a view, refusing indices with no typed counterpart.

pub fn decode_m0(state: u16) -> Result<ModeView, u32> {
    match state {
        0 => Ok(ModeView::Stopped),
        1 => Ok(ModeView::Started),
        2 => Ok(ModeView::PendingDuringLensStart),
        3 => Ok(ModeView::PendingParameters),
        4 => Ok(ModeView::Live),
        5 => Ok(ModeView::PendingAutofocus),
        6 => Ok(ModeView::PendingLensOperation),
        7 => Ok(ModeView::LensStarting),
        8 => Ok(ModeView::LensStopping),
        9 => Ok(ModeView::SuspendingForS1),
        10 => Ok(ModeView::S1),
        _ => Err(1),
    }
}

pub fn decode_m1(state: u16) -> Result<LensView, u32> {
    match state {
        0 => Ok(LensView::Idle),
        1 => Ok(LensView::Prepared),
        2 => Ok(LensView::Profile),
        3 => Ok(LensView::Settings),
        4 => Ok(LensView::Driving),
        5 => Ok(LensView::Stepping),
        6 => Ok(LensView::Selecting),
        _ => Err(1),
    }
}

pub fn decode_m8(state: u16) -> Result<FanView, u32> {
    match state {
        0 => Ok(FanView::Ready),
        1 => Ok(FanView::Draining),
        2 => Ok(FanView::Stopping),
        _ => Err(1),
    }
}

pub fn decode_m11(state: u16) -> Result<LiveView, u32> {
    match state {
        0 => Ok(LiveView::Idle),
        1 => Ok(LiveView::PrimarySuspending),
        2 => Ok(LiveView::PrimaryLens),
        3 => Ok(LiveView::PrimaryUnpaired),
        4 => Ok(LiveView::PrimaryPaired),
        5 => Ok(LiveView::PrimaryResuming),
        6 => Ok(LiveView::SecondaryLive),
        7 => Ok(LiveView::SecondarySuspending),
        8 => Ok(LiveView::SecondaryLens),
        9 => Ok(LiveView::SecondaryResuming),
        10 => Ok(LiveView::Autofocus),
        11 => Ok(LiveView::Command23),
        12 => Ok(LiveView::Command11),
        13 => Ok(LiveView::DriveSuspendingFree),
        14 => Ok(LiveView::DrivePreparedFree),
        15 => Ok(LiveView::DriveLensFree),
        16 => Ok(LiveView::DriveResumingFree),
        17 => Ok(LiveView::DriveSuspendingHeld),
        18 => Ok(LiveView::DrivePreparedHeld),
        19 => Ok(LiveView::DriveLensHeld),
        20 => Ok(LiveView::DriveResumingHeld),
        21 => Ok(LiveView::Command14),
        22 => Ok(LiveView::Command2b),
        23 => Ok(LiveView::Command2c),
        _ => Err(1),
    }
}

pub fn decode_m12(state: u16) -> Result<LiveViewControllerView, u32> {
    match state {
        0 => Ok(LiveViewControllerView::Idle),
        1 => Ok(LiveViewControllerView::ResumePending),
        2 => Ok(LiveViewControllerView::IdleBusy),
        3 => Ok(LiveViewControllerView::Starting),
        4 => Ok(LiveViewControllerView::Sequencing),
        5 => Ok(LiveViewControllerView::Configuring),
        6 => Ok(LiveViewControllerView::Running),
        7 => Ok(LiveViewControllerView::Suspending),
        8 => Ok(LiveViewControllerView::Resuming),
        9 => Ok(LiveViewControllerView::ResumeRetune),
        10 => Ok(LiveViewControllerView::SuspendRetune),
        11 => Ok(LiveViewControllerView::Reconfiguring),
        12 => Ok(LiveViewControllerView::AutofocusRetune),
        13 => Ok(LiveViewControllerView::Adjusting),
        14 => Ok(LiveViewControllerView::Retuning),
        15 => Ok(LiveViewControllerView::RetuneSuspending),
        16 => Ok(LiveViewControllerView::RetuneResuming),
        17 => Ok(LiveViewControllerView::Measuring),
        18 => Ok(LiveViewControllerView::Restarting),
        19 => Ok(LiveViewControllerView::StoppingForRefresh),
        20 => Ok(LiveViewControllerView::Refreshing),
        21 => Ok(LiveViewControllerView::StoppingForAutofocus),
        22 => Ok(LiveViewControllerView::RefreshingForAutofocus),
        23 => Ok(LiveViewControllerView::Windowing),
        24 => Ok(LiveViewControllerView::Stopping),
        _ => Err(1),
    }
}

pub fn decode_m13(state: u16) -> Result<PhaseView, u32> {
    match state {
        0 => Ok(PhaseView::Dormant),
        1 => Ok(PhaseView::Idle),
        2 => Ok(PhaseView::Armed),
        3 => Ok(PhaseView::Running),
        4 => Ok(PhaseView::Stopping),
        _ => Err(1),
    }
}

pub fn decode_m24(state: u16) -> Result<StageView, u32> {
    match state {
        0 => Ok(StageView::Disabled),
        1 => Ok(StageView::Ready),
        2 => Ok(StageView::Secondary),
        3 => Ok(StageView::Primary),
        _ => Err(1),
    }
}

pub fn decode_m29(state: u16) -> Result<JobView, u32> {
    match state {
        0 => Ok(JobView::Disabled),
        1 => Ok(JobView::Ready),
        2 => Ok(JobView::Running),
        _ => Err(1),
    }
}

/// Encodes one machine's state from the runtime view.
pub fn encode_machine(machine: u32, view: &RuntimeView) -> Result<u16, u32> {
    match machine {
        0 => Ok(encode_m0(view.root)),
        1 => match view.lens_live {
            Some(lens) => Ok(encode_m1(lens)),
            None => Err(4),
        },
        8 => match view.measurement {
            Some(measurement) => Ok(encode_m8(measurement.fan)),
            None => Err(4),
        },
        11 => Ok(encode_m11(view.live)),
        12 => Ok(encode_m12(view.live_view)),
        13 => match view.measurement {
            Some(measurement) => Ok(encode_m13(measurement.phase)),
            None => Err(4),
        },
        24 => match (view.measurement, view.s1_acquisition) {
            (Some(measurement), Some(s1)) => Ok(encode_m24(view.root, measurement.acquisition, s1)),
            _ => Err(4),
        },
        29 => match view.measurement {
            Some(measurement) => Ok(encode_m29(measurement.auxiliary)),
            None => Err(4),
        },
        _ => Err(1),
    }
}

/// Decodes one machine's stock state.
pub fn decode_machine(machine: u32, state: u16) -> Result<(), u32> {
    match machine {
        0 => decode_m0(state).map(|_| ()),
        1 => decode_m1(state).map(|_| ()),
        8 => decode_m8(state).map(|_| ()),
        11 => decode_m11(state).map(|_| ()),
        12 => decode_m12(state).map(|_| ()),
        13 => decode_m13(state).map(|_| ()),
        24 => decode_m24(state).map(|_| ()),
        29 => decode_m29(state).map(|_| ()),
        _ => Err(1),
    }
}

/// Re-encodes a decoded machine state and checks it is the index it came from.
pub fn reencode_machine(machine: u32, state: u16) -> Result<u16, u32> {
    match machine {
        0 => decode_m0(state).map(encode_m0),
        1 => decode_m1(state).map(encode_m1),
        8 => decode_m8(state).map(encode_m8),
        11 => decode_m11(state).map(encode_m11),
        12 => decode_m12(state).map(encode_m12),
        13 => decode_m13(state).map(encode_m13),
        24 => decode_m24(state).map(encode_stage),
        29 => decode_m29(state).map(encode_m29),
        _ => Err(1),
    }
}

// ---------------------------------------------------------------------------
// Overlay codec

unsafe fn put_u16(out: *mut u8, at: usize, value: u16) {
    unsafe { ptr::write_unaligned(out.add(at) as *mut u16, value.to_le()) };
}

unsafe fn put_u32(out: *mut u8, at: usize, value: u32) {
    unsafe { ptr::write_unaligned(out.add(at) as *mut u32, value.to_le()) };
}

unsafe fn get_u16(stock: *const u8, at: usize) -> u16 {
    u16::from_le(unsafe { ptr::read_unaligned(stock.add(at) as *const u16) })
}

unsafe fn get_u32(stock: *const u8, at: usize) -> u32 {
    u32::from_le(unsafe { ptr::read_unaligned(stock.add(at) as *const u32) })
}

fn gap_word(field: usize, detail: u32) -> u32 { (field as u32) << 16 | detail }

/// Records the first refusal only; later fields are still written.
///
/// Details: `2` a stock value outside the typed domain, `3` a re-encode mismatch, `4` a cell
/// the view reads is mutably borrowed, `5` Root retains no typed counterpart. Details other
/// than `4` make the call code `3` (encoder refused); a decode refusal uses code `2`.
pub struct Refusal { first: Option<(usize, u32)> }

impl Refusal {
    pub const fn new() -> Self { Self { first: None } }
    pub fn note(&mut self, field: usize, detail: u32) { if self.first.is_none() { self.first = Some((field, detail)); } }
    pub fn code(&self) -> u32 {
        match self.first {
            None => 0,
            Some((_, 4)) => 4,
            Some(_) => 3,
        }
    }
    pub fn first(&self) -> Option<(usize, u32)> { self.first }
}

/// Writes the overlay fields this map can supply from `view`. A field Root cannot supply is
/// left untouched and recorded in `refusal`; nothing is defaulted.
///
/// # Safety
/// `out` must hold `OVERLAY_BYTES` bytes.
pub unsafe fn encode_overlay(view: &RuntimeView, out: *mut u8, refusal: &mut Refusal) {
    // Record handle: bank 2's current generation, all ones when absent.
    match view.contexts.slots[2].current {
        Some(word) => unsafe { put_u32(out, OFF_HANDLE, word) },
        None => unsafe { put_u32(out, OFF_HANDLE, u32::MAX) },
    }
    for (index, machine) in MACHINES.into_iter().enumerate() {
        match encode_machine(machine, view) {
            Ok(state) => unsafe { put_u16(out, OFF_MACHINE0 + 2 * index, state) },
            Err(detail) => refusal.note(FIELD_MACHINE0 + index, detail),
        }
    }
    // 0x16be044/+3/+4: the selection-adjustment mask and the two pair done bytes. The mask
    // is Root's category-pair decision (`Round.selected`); the done bytes are
    // `Planner::busy`, which the step reads only as zero/nonzero.
    match view.measurement {
        Some(measurement) => {
            unsafe {
                ptr::write(out.add(OFF_MASK), encode_mask(measurement.selected));
                ptr::write(out.add(OFF_STAGE_FIRST), stage_byte(measurement.first_busy));
                ptr::write(out.add(OFF_STAGE_SECOND), stage_byte(measurement.second_busy));
            }
        }
        None => {
            refusal.note(FIELD_MASK, 4);
            refusal.note(FIELD_STAGE_FIRST, 4);
            refusal.note(FIELD_STAGE_SECOND, 4);
        }
    }
    // The live branch's grid at 0x16bf370; bytes 0..8 and 0x458 are written-first.
    unsafe {
        ptr::copy_nonoverlapping(view.live_grid.0[GRID_HEAD].as_ptr(), out.add(OFF_GRID_HEAD), GRID_HEAD.end - GRID_HEAD.start);
        ptr::copy_nonoverlapping(view.live_grid.0[GRID_TAIL].as_ptr(), out.add(OFF_GRID_TAIL), GRID_TAIL.end - GRID_TAIL.start);
    }
    // The 0x3d00 record's never-written holes are zero in Root's record
    // (`acquisition_request` starts from `[0; 168]`).
    for (index, (_, length)) in RECORD_HOLES.into_iter().enumerate() {
        unsafe { ptr::write_bytes(out.add(OFF_RECORD_HOLE0 + record_hole_overlay_offset(index)), 0, length) };
    }
    // The window lane's power latch. Without the measurement view the latch is unknown and
    // must not be defaulted.
    match view.measurement {
        Some(measurement) => unsafe { put_u32(out, OFF_LATCH, u32::from(measurement.power_held)) },
        None => refusal.note(FIELD_LATCH, 4),
    }
    // The calc peer queue id.
    unsafe { put_u32(out, OFF_QUEUE, u32::from(peers::CALC.queue.number())) };
    // The 132-byte shared profile state (`FOCUS_RESULT`) and the statistics descriptor
    // (`AREA_STATISTICS`), both read from Root's `ProfileRecords` shared block.
    unsafe {
        ptr::copy_nonoverlapping(view.focus_result.as_ptr(), out.add(OFF_FOCUS_RESULT), 132);
        let present = u32::from_le_bytes(view.area_statistics[0..4].try_into().expect("descriptor presence"));
        put_u32(out, OFF_PRESENT, present);
        ptr::write(out.add(OFF_LAYOUT), view.area_statistics[4]);
        put_u32(out, OFF_ENTRIES_POINTER, entries_pointer(present));
        put_u32(out, OFF_DESCRIPTOR_WORD3, 0);
    }
    // The adjustment block's word at +0x00 is a constant of the stock artifact (no store
    // reaches it), so it is written as 0; the other three have no retained Root counterpart
    // (gap G12).
    unsafe { put_u32(out, OFF_ADJUSTMENT_00, 0) };
    refusal.note(FIELD_CLASSIFICATION, 5);
    refusal.note(FIELD_ADJUSTMENT_48, 5);
    refusal.note(FIELD_ADJUSTMENT_58, 5);
    // The planner's cursors, mode and phase.
    match view.measurement {
        Some(measurement) => {
            let planner = measurement.planner;
            match encode_cursor(planner.first_cursor) {
                Ok(byte) => unsafe { ptr::write(out.add(OFF_FIRST_CURSOR), byte) },
                Err(detail) => refusal.note(FIELD_FIRST_CURSOR, detail),
            }
            match encode_cursor(planner.second_cursor) {
                Ok(byte) => unsafe { ptr::write(out.add(OFF_SECOND_CURSOR), byte) },
                Err(detail) => refusal.note(FIELD_SECOND_CURSOR, detail),
            }
            unsafe {
                put_u32(out, OFF_MODE, encode_mode(planner.mode));
                ptr::write(out.add(OFF_PHASE), encode_planner_phase(planner.phase));
            }
        }
        None => {
            refusal.note(FIELD_FIRST_CURSOR, 4);
            refusal.note(FIELD_SECOND_CURSOR, 4);
            refusal.note(FIELD_MODE, 4);
            refusal.note(FIELD_PHASE, 4);
        }
    }
}

/// The overlay offset of record hole `index`.
pub fn record_hole_overlay_offset(index: usize) -> usize {
    let mut at = 0;
    let mut i = 0;
    while i < index {
        at += RECORD_HOLES[i].1;
        i += 1;
    }
    at
}

/// Decodes a stock overlay into the typed views and encodes it back into `out`. Returns the
/// refusal code (0, 2 or 3) and the gap word.
///
/// # Safety
/// `stock` and `out` must each hold `OVERLAY_BYTES` bytes and must not overlap.
pub unsafe fn roundtrip_overlay(stock: *const u8, out: *mut u8) -> (u32, Option<u32>) {
    unsafe { ptr::copy_nonoverlapping(stock, out, OVERLAY_BYTES) };
    let mut refusal = Refusal::new();
    let mut code = 0;
    // Machine states: decode then encode, and the result must be the same index.
    for (index, machine) in MACHINES.into_iter().enumerate() {
        let state = unsafe { get_u16(stock, OFF_MACHINE0 + 2 * index) };
        match reencode_machine(machine, state) {
            Ok(same) if same == state => unsafe { put_u16(out, OFF_MACHINE0 + 2 * index, same) },
            Ok(_) => { refusal.note(FIELD_MACHINE0 + index, 3); code = 3; }
            Err(detail) => { refusal.note(FIELD_MACHINE0 + index, detail); code = 2; }
        }
    }
    // The mask decodes to the selected lanes; bits above bit6 have no Root counterpart.
    let mask = unsafe { ptr::read(stock.add(OFF_MASK)) };
    match decode_mask(mask) {
        Ok(lanes) => {
            let encoded = encode_mask(lanes);
            if encoded != mask {
                refusal.note(FIELD_MASK, 3);
                code = 3;
            } else {
                unsafe { ptr::write(out.add(OFF_MASK), encoded) };
            }
        }
        Err(detail) => { refusal.note(FIELD_MASK, detail); code = 2; }
    }
    // The pair done bytes are a zero test in the wake step; a nonzero byte decodes to the
    // busy/done slot and re-encodes to the canonical nonzero value (non-identical by design).
    for (_, off) in [(FIELD_STAGE_FIRST, OFF_STAGE_FIRST), (FIELD_STAGE_SECOND, OFF_STAGE_SECOND)] {
        let byte = unsafe { ptr::read(stock.add(off)) };
        unsafe { ptr::write(out.add(off), stage_byte(stage_busy(byte))) };
    }
    // The grid's exposed ranges are the typed grid's own bytes; the copy already carries
    // them.
    // Record holes: a nonzero byte is residue with no typed counterpart.
    for (index, (_, length)) in RECORD_HOLES.into_iter().enumerate() {
        let at = OFF_RECORD_HOLE0 + record_hole_overlay_offset(index);
        let mut byte = 0;
        while byte < length {
            if unsafe { ptr::read(stock.add(at + byte)) } != 0 { refusal.note(FIELD_RECORD_HOLE0 + index, 2); code = 2; }
            byte += 1;
        }
        unsafe { ptr::write_bytes(out.add(at), 0, length) };
    }
    // Latch: only 0 and 1 are typed.
    let latch = unsafe { get_u32(stock, OFF_LATCH) };
    if latch > 1 { refusal.note(FIELD_LATCH, 2); code = 2; } else { unsafe { put_u32(out, OFF_LATCH, latch) } }
    // Queue word: the calc route's id only.
    let queue = unsafe { get_u32(stock, OFF_QUEUE) };
    if queue != u32::from(peers::CALC.queue.number()) { refusal.note(FIELD_QUEUE, 2); code = 2; } else { unsafe { put_u32(out, OFF_QUEUE, queue) } }
    // The shared profile state is a typed byte block (`FOCUS_RESULT`, 132 bytes); the copy
    // already carries it.
    // Descriptor: presence and layout are typed words; the pointer must name the entries
    // buffer exactly when the statistics are present; word 3 is never read and stock leaves
    // it zero.
    let present = unsafe { get_u32(stock, OFF_PRESENT) };
    let layout = unsafe { ptr::read(stock.add(OFF_LAYOUT)) };
    unsafe {
        put_u32(out, OFF_PRESENT, present);
        ptr::write(out.add(OFF_LAYOUT), layout);
    }
    let pointer = unsafe { get_u32(stock, OFF_ENTRIES_POINTER) };
    match decode_entries_pointer(present, pointer) {
        Ok(()) => unsafe { put_u32(out, OFF_ENTRIES_POINTER, pointer) },
        Err(detail) => { refusal.note(FIELD_ENTRIES_POINTER, detail); code = 2; }
    }
    let word3 = unsafe { get_u32(stock, OFF_DESCRIPTOR_WORD3) };
    if word3 != 0 { refusal.note(FIELD_DESCRIPTOR_WORD3, 2); code = 2; } else { unsafe { put_u32(out, OFF_DESCRIPTOR_WORD3, 0) } }
    // The adjustment block's word at +0x00 is a stock constant (0); the other three have no
    // Root read (gap G12).
    let constant = unsafe { get_u32(stock, OFF_ADJUSTMENT_00) };
    if constant != 0 { refusal.note(FIELD_ADJUSTMENT_00, 2); code = 2; } else { unsafe { put_u32(out, OFF_ADJUSTMENT_00, 0) } }
    for field in [FIELD_CLASSIFICATION, FIELD_ADJUSTMENT_48, FIELD_ADJUSTMENT_58] {
        refusal.note(field, 2);
        code = 2;
    }
    // The planner's cursors are byte domains; the mode and phase have typed counterparts.
    let first = unsafe { ptr::read(stock.add(OFF_FIRST_CURSOR)) };
    unsafe { ptr::write(out.add(OFF_FIRST_CURSOR), first) };
    let second = unsafe { ptr::read(stock.add(OFF_SECOND_CURSOR)) };
    unsafe { ptr::write(out.add(OFF_SECOND_CURSOR), second) };
    let mode = unsafe { get_u32(stock, OFF_MODE) };
    match decode_mode(mode) {
        Ok(view) => unsafe { put_u32(out, OFF_MODE, encode_mode(view)) },
        Err(detail) => { refusal.note(FIELD_MODE, detail); code = 2; }
    }
    let phase = unsafe { ptr::read(stock.add(OFF_PHASE)) };
    match decode_planner_phase(phase) {
        Ok(view) => unsafe { ptr::write(out.add(OFF_PHASE), encode_planner_phase(view)) },
        Err(detail) => { refusal.note(FIELD_PHASE, detail); code = 2; }
    }
    (code, refusal.first().map(|(field, detail)| gap_word(field, detail)))
}

// ---------------------------------------------------------------------------
// Tests: real measured values and the full accepted domains.

#[cfg(test)]
mod tests {
    use super::*;

    /// The measured stock states of cycle-stock-26
    /// (`runs/20261119T192511Z-cycle-step-coverage`, state.bin at 0x169d4b4).
    const MEASURED: [u16; 8] = [4, 0, 0, 0, 5, 2, 1, 1];

    #[test]
    fn the_measured_wake_states_decode_to_their_declared_views() {
        assert_eq!(decode_m0(4), Ok(ModeView::Live));
        assert_eq!(decode_m0(6), Ok(ModeView::PendingLensOperation));
        assert_eq!(decode_m1(0), Ok(LensView::Idle));
        assert_eq!(decode_m8(0), Ok(FanView::Ready));
        assert_eq!(decode_m11(0), Ok(LiveView::Idle));
        assert_eq!(decode_m11(3), Ok(LiveView::PrimaryUnpaired));
        assert_eq!(decode_m11(6), Ok(LiveView::SecondaryLive));
        assert_eq!(decode_m12(5), Ok(LiveViewControllerView::Configuring));
        assert_eq!(decode_m13(2), Ok(PhaseView::Armed));
        assert_eq!(decode_m24(1), Ok(StageView::Ready));
        assert_eq!(decode_m24(2), Ok(StageView::Secondary));
        assert_eq!(decode_m24(3), Ok(StageView::Primary));
        assert_eq!(decode_m24(4), Err(1));
        assert_eq!(decode_m29(1), Ok(JobView::Ready));
        assert_eq!(decode_m29(2), Ok(JobView::Running));
        assert_eq!(decode_m29(3), Err(1));
        for (index, machine) in MACHINES.into_iter().enumerate() {
            assert_eq!(decode_machine(machine, MEASURED[index]), Ok(()), "machine {machine}");
            assert_eq!(reencode_machine(machine, MEASURED[index]), Ok(MEASURED[index]), "machine {machine}");
        }
    }

    #[test]
    fn every_accepted_stock_state_round_trips() {
        for (index, machine) in MACHINES.into_iter().enumerate() {
            for state in 0..STATE_COUNTS[index] {
                match reencode_machine(machine, state) {
                    Ok(same) => assert_eq!(same, state, "machine {machine} state {state}"),
                    Err(_) => assert!(decode_machine(machine, state).is_err(), "machine {machine} state {state}"),
                }
            }
        }
    }

    #[test]
    fn every_typed_view_encodes_inside_its_machines_state_count() {
        const MODES: [ModeView; 11] = [ModeView::Stopped, ModeView::Started, ModeView::LensStarting, ModeView::LensStopping, ModeView::Live, ModeView::PendingDuringLensStart, ModeView::PendingParameters, ModeView::PendingAutofocus, ModeView::PendingLensOperation, ModeView::SuspendingForS1, ModeView::S1];
        for view in MODES { assert!((encode_m0(view)) < 11); }
        const LENS: [LensView; 7] = [LensView::Idle, LensView::Prepared, LensView::Profile, LensView::Settings, LensView::Driving, LensView::Stepping, LensView::Selecting];
        for view in LENS { assert!((encode_m1(view)) < 14); }
        const FANS: [FanView; 3] = [FanView::Ready, FanView::Draining, FanView::Stopping];
        for view in FANS { assert!((encode_m8(view)) < 3); }
        const LIVE: [LiveView; 24] = [LiveView::Idle, LiveView::PrimarySuspending, LiveView::PrimaryLens, LiveView::PrimaryUnpaired, LiveView::PrimaryPaired, LiveView::PrimaryResuming, LiveView::SecondaryLive, LiveView::SecondarySuspending, LiveView::SecondaryLens, LiveView::SecondaryResuming, LiveView::Autofocus, LiveView::Command23, LiveView::Command11, LiveView::DriveSuspendingFree, LiveView::DrivePreparedFree, LiveView::DriveLensFree, LiveView::DriveResumingFree, LiveView::DriveSuspendingHeld, LiveView::DrivePreparedHeld, LiveView::DriveLensHeld, LiveView::DriveResumingHeld, LiveView::Command14, LiveView::Command2b, LiveView::Command2c];
        for view in LIVE { assert!((encode_m11(view)) < 31); }
        const VIEWS: [LiveViewControllerView; 26] = [LiveViewControllerView::Idle, LiveViewControllerView::ResumePending, LiveViewControllerView::IdleBusy, LiveViewControllerView::Starting, LiveViewControllerView::Sequencing, LiveViewControllerView::Configuring, LiveViewControllerView::Running, LiveViewControllerView::Suspending, LiveViewControllerView::Resuming, LiveViewControllerView::ResumeRetune, LiveViewControllerView::SuspendRetune, LiveViewControllerView::Reconfiguring, LiveViewControllerView::AutofocusRetune, LiveViewControllerView::Adjusting, LiveViewControllerView::Retuning, LiveViewControllerView::RetuneSuspending, LiveViewControllerView::RetuneResuming, LiveViewControllerView::Measuring, LiveViewControllerView::Restarting, LiveViewControllerView::StoppingForRefresh, LiveViewControllerView::Refreshing, LiveViewControllerView::StoppingForAutofocus, LiveViewControllerView::RefreshingForAutofocus, LiveViewControllerView::Windowing, LiveViewControllerView::Stopping, LiveViewControllerView::Restoring];
        for view in VIEWS { assert!((encode_m12(view)) < 25); }
        const PHASES: [PhaseView; 5] = [PhaseView::Dormant, PhaseView::Idle, PhaseView::Armed, PhaseView::Running, PhaseView::Stopping];
        for view in PHASES { assert!((encode_m13(view)) < 6); }
        const STAGES: [StageView; 6] = [StageView::Disabled, StageView::Ready, StageView::Primary, StageView::Secondary, StageView::StoppingPrimary, StageView::StoppingSecondary];
        for view in STAGES { assert!((encode_stage(view)) < 5); }
        const JOBS: [JobView; 4] = [JobView::Disabled, JobView::Ready, JobView::Running, JobView::Stopping];
        for view in JOBS { assert!((encode_m29(view)) < 4); }
    }

    #[test]
    fn the_pair_done_bytes_are_a_zero_test() {
        assert_eq!(stage_byte(false), 0);
        assert_eq!(stage_byte(true), 0x01);
        assert!(!stage_busy(0));
        assert!(stage_busy(0x20));
        assert!(stage_busy(0x01));
        assert_eq!(stage_byte(stage_busy(0x20)), 0x01);
        assert_eq!(stage_byte(stage_busy(0x01)), 0x01);
    }

    #[test]
    fn the_selection_mask_is_the_selected_lanes() {
        use xm1_focus_measurement_round::lanes::{Lane, Lanes};
        assert_eq!(encode_mask(Lanes::empty()), 0);
        let mut measured = Lanes::empty();
        measured.insert(Lane::Acquisition);
        measured.insert(Lane::Auxiliary);
        measured.insert(Lane::Window);
        assert_eq!(encode_mask(measured), 0x25);
        assert_eq!(decode_mask(0x25), Ok(measured));
        for mask in 0..=0x7f {
            let lanes = decode_mask(mask).expect("every mask below bit7 has a lane set");
            assert_eq!(encode_mask(lanes), mask, "mask {mask:#04x}");
        }
        assert_eq!(decode_mask(0x80), Err(1));
    }

    #[test]
    fn machine_24_follows_the_acquisition_of_the_branch_root_mode_names() {
        const MODES: [ModeView; 11] = [ModeView::Stopped, ModeView::Started, ModeView::LensStarting, ModeView::LensStopping, ModeView::Live, ModeView::PendingDuringLensStart, ModeView::PendingParameters, ModeView::PendingAutofocus, ModeView::PendingLensOperation, ModeView::SuspendingForS1, ModeView::S1];
        const STAGES: [StageView; 6] = [StageView::Disabled, StageView::Ready, StageView::Primary, StageView::Secondary, StageView::StoppingPrimary, StageView::StoppingSecondary];
        for mode in MODES {
            let s1_branch = matches!(mode, ModeView::S1 | ModeView::SuspendingForS1);
            for live in STAGES {
                for s1 in STAGES {
                    let expected = if s1_branch { encode_stage(s1) } else { encode_stage(live) };
                    assert_eq!(encode_m24(mode, live, s1), expected, "{mode:?} live {live:?} s1 {s1:?}");
                }
            }
        }
        // The measured cycle-stock-26 machine 24 state 1 is `Ready` on whichever branch runs.
        assert_eq!(encode_m24(ModeView::Live, StageView::Ready, StageView::Disabled), 1);
        assert_eq!(encode_m24(ModeView::SuspendingForS1, StageView::Disabled, StageView::Ready), 1);
        assert_eq!(encode_m24(ModeView::S1, StageView::Disabled, StageView::Ready), 1);
    }

    #[test]
    fn the_planner_words_encode_their_typed_values() {
        const MODES: [PlannerModeView; 5] = [PlannerModeView::Default, PlannerModeView::RecordSelected, PlannerModeView::ProfileSelected, PlannerModeView::AboveThreshold, PlannerModeView::BelowThreshold];
        assert_eq!(encode_mode(PlannerModeView::Default), 0);
        assert_eq!(encode_mode(PlannerModeView::RecordSelected), 1);
        assert_eq!(encode_mode(PlannerModeView::ProfileSelected), 3);
        assert_eq!(encode_mode(PlannerModeView::AboveThreshold), 4);
        assert_eq!(encode_mode(PlannerModeView::BelowThreshold), 5);
        for view in MODES {
            let word = encode_mode(view);
            assert_eq!(decode_mode(word), Ok(view));
        }
        // The measured wakes hold mode 0 (Default) or 5 (BelowThreshold).
        assert_eq!(decode_mode(0), Ok(PlannerModeView::Default));
        assert_eq!(decode_mode(5), Ok(PlannerModeView::BelowThreshold));
        // Indices 2 (the third selection test) and 6 (the never-stored disabled value) have
        // no Root mode.
        for word in [2, 6, 7, u32::MAX] { assert_eq!(decode_mode(word), Err(1), "mode {word}"); }

        const PHASES: [PlannerPhaseView; 4] = [PlannerPhaseView::First, PlannerPhaseView::Second, PlannerPhaseView::Third, PlannerPhaseView::Fourth];
        for (index, view) in PHASES.into_iter().enumerate() {
            assert_eq!(encode_planner_phase(view), index as u8);
            assert_eq!(decode_planner_phase(index as u8), Ok(view));
        }
        // The measured wakes hold phases 0..3.
        assert_eq!(decode_planner_phase(0), Ok(PlannerPhaseView::First));
        assert_eq!(decode_planner_phase(3), Ok(PlannerPhaseView::Fourth));
        for byte in 4..=u8::MAX { assert_eq!(decode_planner_phase(byte), Err(1), "phase {byte}"); }

        // The measured wakes hold cursor bytes 0 or 1; the whole byte domain is the cursor.
        for cursor in [0usize, 1, 3, 4, 255] {
            assert_eq!(encode_cursor(cursor), Ok(cursor as u8));
            assert_eq!(decode_cursor(cursor as u8), cursor);
        }
        assert_eq!(encode_cursor(256), Err(1));
    }

    #[test]
    fn the_area_statistics_descriptor_round_trips_its_presence() {
        for present in [0u32, 1] {
            let pointer = entries_pointer(present);
            assert_eq!(pointer, if present == 0 { 0 } else { 0x5ffee7_18 });
            assert_eq!(decode_entries_pointer(present, pointer), Ok(()));
        }
        assert_eq!(entries_pointer(2), 0x5ffee7_18);
        assert_eq!(decode_entries_pointer(0, 0x5ffee7_18), Err(1));
        assert_eq!(decode_entries_pointer(1, 0), Err(1));
    }

    #[test]
    fn the_field_table_covers_the_exposed_ranges_in_overlay_order() {
        let mut total = 0;
        for field in QUALIFICATION_FIELDS { total += field[1] as usize; }
        assert_eq!(total, OVERLAY_BYTES);
        assert_eq!(QUALIFICATION_FIELDS[0], [0x0169_cf08, 4]);
        assert_eq!(QUALIFICATION_FIELDS[1], [0x0169_d4b4, 2]);
        assert_eq!(QUALIFICATION_FIELDS[8], [0x0169_d4ee, 2]);
        assert_eq!(QUALIFICATION_FIELDS[FIELD_MASK], [0x016b_e044, 1]);
        assert_eq!(QUALIFICATION_FIELDS[FIELD_GRID_HEAD], [0x016b_f378, 1104]);
        assert_eq!(QUALIFICATION_FIELDS[FIELD_LATCH], [0x016c_0710, 4]);
        assert_eq!(QUALIFICATION_FIELDS[FIELD_QUEUE], [0x016c_236c, 4]);
        assert_eq!(QUALIFICATION_FIELDS[FIELD_FOCUS_RESULT], [0x0091_904c, 132]);
        assert_eq!(QUALIFICATION_FIELDS[FIELD_PRESENT], [0x0091_90f0, 4]);
        assert_eq!(QUALIFICATION_FIELDS[FIELD_LAYOUT], [0x0091_90f4, 1]);
        assert_eq!(QUALIFICATION_FIELDS[FIELD_ENTRIES_POINTER], [0x0091_90f8, 4]);
        assert_eq!(QUALIFICATION_FIELDS[FIELD_DESCRIPTOR_WORD3], [0x0091_90fc, 4]);
        assert_eq!(QUALIFICATION_FIELDS[FIELD_CLASSIFICATION], [0x016d_8748, 4]);
        assert_eq!(QUALIFICATION_FIELDS[FIELD_FIRST_CURSOR], [0x016d_8778, 1]);
        assert_eq!(QUALIFICATION_FIELDS[FIELD_MODE], [0x016d_877c, 4]);
        assert_eq!(QUALIFICATION_FIELDS[FIELD_PHASE], [0x016d_8ff0, 1]);
        assert_eq!(QUALIFICATION_FIELDS[FIELD_SECOND_CURSOR], [0x016d_8ff1, 1]);
        assert_eq!(OFF_FOCUS_RESULT, 1269);
        assert_eq!(OFF_CLASSIFICATION, 1418);
        assert_eq!(OFF_SECOND_CURSOR, 1436);
        assert_eq!(record_hole_overlay_offset(0), 0);
        assert_eq!(record_hole_overlay_offset(5), 9);
    }

    #[test]
    fn the_measured_stock_values_round_trip_except_the_declared_gaps() {
        // One overlay carrying the captured wakes' values (all 30 wakes and 27 window
        // completions share them, except the planner cursors and phases that vary):
        // machine states 4,0,0,0,5,2,1,1; mask 0x25; zero done bytes, grid, holes; latch 0;
        // queue 44; present 1, layout 4, pointer 0x5ffee718, word 3 zero; mode 5; the
        // cursors 0 or 1 and the phases 0..3.
        let mut overlay = [0u8; OVERLAY_BYTES];
        for (index, state) in MEASURED.into_iter().enumerate() {
            overlay[OFF_MACHINE0 + 2 * index..OFF_MACHINE0 + 2 * index + 2].copy_from_slice(&state.to_le_bytes());
        }
        overlay[OFF_MASK] = 0x25;
        overlay[OFF_QUEUE..OFF_QUEUE + 4].copy_from_slice(&u32::from(peers::CALC.queue.number()).to_le_bytes());
        overlay[OFF_PRESENT..OFF_PRESENT + 4].copy_from_slice(&1u32.to_le_bytes());
        overlay[OFF_LAYOUT] = 4;
        overlay[OFF_ENTRIES_POINTER..OFF_ENTRIES_POINTER + 4].copy_from_slice(&AREA_ENTRIES_POINTER.to_le_bytes());
        overlay[OFF_MODE..OFF_MODE + 4].copy_from_slice(&5u32.to_le_bytes());
        for (cursor, phase) in [(0u8, 0u8), (0, 1), (0, 2), (0, 3), (1, 0), (1, 1), (1, 2), (1, 3)] {
            overlay[OFF_FIRST_CURSOR] = cursor;
            overlay[OFF_SECOND_CURSOR] = cursor;
            overlay[OFF_PHASE] = phase;
            let mut out = [0u8; OVERLAY_BYTES];
            let (code, gap) = unsafe { roundtrip_overlay(overlay.as_ptr(), out.as_mut_ptr()) };
            // The adjustment block's untyped words are the declared gaps; nothing else
            // refuses.
            assert_eq!(code, 2, "cursor {cursor} phase {phase}");
            assert_eq!(gap, Some(gap_word(FIELD_CLASSIFICATION, 2)), "cursor {cursor} phase {phase}");
            // Every non-gap field round-trips to itself.
            for (field, entry) in QUALIFICATION_FIELDS.iter().enumerate() {
                if matches!(field, FIELD_CLASSIFICATION | FIELD_ADJUSTMENT_48 | FIELD_ADJUSTMENT_58) { continue; }
                let (address, length) = (entry[0], entry[1]);
                let _ = address;
                let at = overlay_offset(field);
                let range = at..at + length as usize;
                assert_eq!(out[range.clone()], overlay[range], "field {field}");
            }
        }
    }

    #[test]
    fn the_adjustment_word_zero_is_the_stock_constant() {
        // `0x016d8708` is 0 in every state and no store reaches it; any other value refuses.
        let mut overlay = [0u8; OVERLAY_BYTES];
        overlay[OFF_QUEUE..OFF_QUEUE + 4].copy_from_slice(&u32::from(peers::CALC.queue.number()).to_le_bytes());
        let mut out = [0u8; OVERLAY_BYTES];
        let (code, gap) = unsafe { roundtrip_overlay(overlay.as_ptr(), out.as_mut_ptr()) };
        assert_eq!(code, 2);
        assert_eq!(gap, Some(gap_word(FIELD_CLASSIFICATION, 2)));
        assert_eq!(&out[OFF_ADJUSTMENT_00..OFF_ADJUSTMENT_00 + 4], &[0; 4]);
        overlay[OFF_ADJUSTMENT_00] = 1;
        let (code, gap) = unsafe { roundtrip_overlay(overlay.as_ptr(), out.as_mut_ptr()) };
        assert_eq!(code, 2);
        assert_eq!(gap, Some(gap_word(FIELD_ADJUSTMENT_00, 2)));
    }

    /// The overlay offset of a field index, from the offsets the codec uses.
    fn overlay_offset(field: usize) -> usize {
        let mut at = 0;
        for index in 0..field { at += QUALIFICATION_FIELDS[index][1] as usize; }
        at
    }
}

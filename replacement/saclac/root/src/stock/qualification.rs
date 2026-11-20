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
//! | 24 | 5 | 7 | window `round::JobView` (4 of 5) |
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
//! (`inputs/stock-step-exposed.json`, 1269 bytes) in overlay order. The eight machine-state
//! halfwords are fields of their own. The grid is two ranges because bytes 0..8 and 0x458
//! are written by the step before it reads them.

use core::ptr;
use xm1_focus_measurement_round::jobs::qualification::JobView;
use xm1_focus_measurement_round::round::qualification::{FanView, PhaseView};
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
/// The number of mapped fields.
pub const FIELD_COUNT: usize = 22;

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
/// The overlay's byte length.
pub const OVERLAY_BYTES: usize = 1269;

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

/// Machine 24: the window calculation job. Stock state 4 has no typed counterpart.
pub fn encode_m24(view: JobView) -> u16 {
    match view {
        JobView::Disabled => 0,
        JobView::Ready => 1,
        JobView::Running => 2,
        JobView::Stopping => 3,
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

pub fn decode_m24(state: u16) -> Result<JobView, u32> {
    match state {
        0 => Ok(JobView::Disabled),
        1 => Ok(JobView::Ready),
        2 => Ok(JobView::Running),
        3 => Ok(JobView::Stopping),
        _ => Err(1),
    }
}

pub fn decode_m29(state: u16) -> Result<JobView, u32> {
    match state {
        0 => Ok(JobView::Disabled),
        1 => Ok(JobView::Ready),
        2 => Ok(JobView::Running),
        3 => Ok(JobView::Stopping),
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
        24 => match view.measurement {
            Some(measurement) => Ok(encode_m24(measurement.window)),
            None => Err(4),
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
        24 => decode_m24(state).map(encode_m24),
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
    // 0x16be044/+3/+4: the selection category mask and the two pair stage indices. Root's
    // owned wake path does not write this block (the machine-8 body at
    // `power_lifecycle/src/record_task43_machines8.rs` writes it through the stock helpers
    // `0xf32530`/`0xf32558`/`0xf32dd8`, and the typed 0x3b4b path bypasses that body), so
    // there is no typed counterpart to encode.
    refusal.note(FIELD_MASK, 1);
    refusal.note(FIELD_STAGE_FIRST, 1);
    refusal.note(FIELD_STAGE_SECOND, 1);
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
    // The mask and stage bytes have no typed counterpart.
    refusal.note(FIELD_MASK, 1);
    refusal.note(FIELD_STAGE_FIRST, 1);
    refusal.note(FIELD_STAGE_SECOND, 1);
    if code == 0 { code = 2; }
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
        assert_eq!(decode_m24(1), Ok(JobView::Ready));
        assert_eq!(decode_m29(1), Ok(JobView::Ready));
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
        const JOBS: [JobView; 4] = [JobView::Disabled, JobView::Ready, JobView::Running, JobView::Stopping];
        for view in JOBS { assert!((encode_m24(view)) < 5); assert!((encode_m29(view)) < 4); }
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
        assert_eq!(record_hole_overlay_offset(0), 0);
        assert_eq!(record_hole_overlay_offset(5), 9);
    }
}

#![forbid(unsafe_code)]

//! The live search's intake (0xf40944): what it takes in with each frame it hands calc to
//! evaluate, as the calc record and as the values the strategy keeps for the evaluation.

use crate::contrast::{LensIdentity, Phase, StageRecord};
use xm1_saclac_interface::search::{ScanEvaluation, SearchRecord};

/// What the capture pipeline recorded with the frame (0xfea7ac(1)).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameSnapshot {
    pub status: u32,
    /// The window for a frame mode other than 1, then the one for mode 1.
    pub windows: [[u32; 2]; 2],
    pub size: [u32; 2],
    pub extent: [u32; 2],
    pub timing: u32,
    pub step: u32,
    pub levels: [u16; 2],
    pub adjustment: u32,
    pub level_offset: i16,
    /// The lens-table row the frame was taken with.
    pub lens: LensIdentity,
    pub selected: u32,
    pub frame_mode: u32,
    pub pinned_setting: u16,
    pub pinned: u32,
    pub pacing: [u32; 2],
    pub correction_source: i16,
    pub correction_class: u32,
    pub corrects_position: u8,
}

/// The record and profile fields the intake reads.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IntakeRecord {
    /// 0x74ec30.
    pub scale: u32,
    /// 0x74e5c0.
    pub selection: u32,
    /// 0x74fa2c.
    pub mode: u32,
    /// 0x74e648.
    pub profile_class: u32,
    /// The first word 0x758c98 copies.
    pub resource: u32,
    /// 0x757470 and 0x757510.
    pub range: [i16; 2],
    /// 0x757c60 and 0x757cb0.
    pub range_shift: [i16; 2],
    /// 0x7580ec.
    pub window_status: u32,
    /// 0x756ac4.
    pub second_result: u32,
    /// 0x754d9c.
    pub record_20b0: u32,
    /// 0x758278.
    pub program_source: u32,
    /// 0x74f784.
    pub record_step: u8,
    /// 0x7581e8.
    pub lens_kind: u8,
    /// 0x757e38.
    pub program_word: u32,
    /// 0x758068.
    pub stage: StageRecord,
}

/// What the exposure layer's selected program reads (0xf77c8c).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramInputs {
    pub selection: u32,
    pub second_result: u32,
    pub window_status: u32,
    pub profile_class: u32,
    pub resource: u32,
    pub record_20b0: u32,
    /// 0xfa9588 for record mode 6, else 0.
    pub planner: u32,
    /// 0xf784f8.
    pub level_word: u16,
}

pub trait IntakePorts {
    fn frame_snapshot(&mut self) -> FrameSnapshot;
    fn record(&mut self) -> IntakeRecord;
    /// 0xf5206c.
    fn selection_ratio(&mut self) -> [u32; 2];
    /// 0x746834.
    fn parameter_index(&mut self, parameter: u32) -> u32;
    /// 0x76d9c8.
    fn lens_identity(&mut self) -> LensIdentity;
    /// 0xef74e4.
    fn converted_decade(&mut self) -> u16;
    /// 0x772f28.
    fn zone_limit(&mut self) -> i16;
    /// The lens table's level for `step` on `lens` (0x746bf8).
    fn lens_level(&mut self, step: u32, lens: LensIdentity) -> i16;
    /// 0xf77ef8.
    fn lens_step(&mut self, kind: u8, lens: LensIdentity) -> u32;
    /// 0xf784f8.
    fn program_level_word(&mut self, kind: u8, far: i16, program_word: u32, source: u32) -> u16;
    /// 0xfa9588.
    fn planner_word(&mut self) -> u32;
    /// 0xf77c8c.
    fn selected_program(&mut self, inputs: ProgramInputs) -> u32;
    /// The program's level at `base` for `lens` (0xf78d24).
    fn program_level(&mut self, program: u32, base: i16, lens: LensIdentity) -> i32;
    /// Whether the latest measurement reached the peak threshold (0xf3daf0).
    fn peak_threshold_reached(&mut self) -> bool;
    /// Adjustments 0x8b16 and 0x8b1f in tenths of the focus value.
    fn scan_thresholds(&mut self) -> [i8; 2];
}

/// What the intake reads of the strategy's state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IntakeView {
    pub phase: Phase,
    pub accepted: bool,
    pub samples: u8,
    /// The search reads the scan pairs.
    pub scan: bool,
    /// The last evaluation's first focus value.
    pub focus_value: i16,
}

/// What the strategy keeps from the intake for the evaluation it requested.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IntakeFrame {
    pub lens: LensIdentity,
    pub frame_mode: u32,
    /// The near end of the evaluated range, when the snapshot was settled.
    pub settled_near: Option<i16>,
    pub step: u32,
    pub level: i16,
    pub stage_first_pass: u8,
    pub stage_last: u8,
    pub parameter_index: u32,
    /// The lens the lens layer reports at intake.
    pub current_lens: LensIdentity,
    pub selected: u32,
    pub pinned: u32,
    pub pinned_setting: u16,
    pub pacing: [u32; 2],
    pub correction_source: i16,
    pub correction_class: u32,
    pub corrects_position: u8,
    pub scan: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Intake {
    pub record: SearchRecord,
    pub frame: IntakeFrame,
}

pub fn take_in(view: IntakeView, ports: &mut impl IntakePorts) -> Intake {
    let snapshot = ports.frame_snapshot();
    let record = ports.record();
    let ratio = ports.selection_ratio();
    let zone = ports.zone_limit();
    let raised = snapshot.levels[1].wrapping_sub(zone as u16);
    let lens_level = ports.lens_level(snapshot.step, snapshot.lens);
    let level = snapshot.levels[0].wrapping_sub(raised).wrapping_add(lens_level as u16) as i16;
    let window = if snapshot.frame_mode == 1 { snapshot.windows[1] } else { snapshot.windows[0] };
    let decade = ports.converted_decade();
    let settled = snapshot.status & 5 == 5;
    let range = evaluated_range(&record, &snapshot);
    let parameter_index = ports.parameter_index(record.stage.words[0]);
    let current_lens = ports.lens_identity();
    let program = frame_program(&record, &snapshot, ports);
    let level_matched = ports.program_level(program, snapshot.level_offset, snapshot.lens) == i32::from(level);
    let scan = scan_after(view, snapshot.frame_mode, ports);
    let scan_evaluation = scan_evaluation(view.phase, scan, view.focus_value, ports.scan_thresholds());
    Intake {
        record: SearchRecord {
            first: !view.accepted,
            scale: [record.scale as u16, (record.scale >> 16) as u16],
            settled,
            level_matched,
            selection: record.selection,
            mode: record.mode,
            ratio,
            range,
            range_offset: 0,
            extent: snapshot.extent,
            window,
            frame_mode: snapshot.frame_mode,
            scan,
            profile_class: record.profile_class,
            resource: record.resource,
            scan_evaluation,
            size: snapshot.size,
            timing: snapshot.timing,
            decade,
            correction: snapshot.adjustment,
            limit: level,
        },
        frame: IntakeFrame {
            lens: snapshot.lens,
            frame_mode: snapshot.frame_mode,
            settled_near: settled.then_some(range[0]),
            step: snapshot.step,
            level: snapshot.levels[1] as i16,
            stage_first_pass: record.stage.first_pass,
            stage_last: record.stage.last,
            parameter_index,
            current_lens,
            selected: snapshot.selected,
            pinned: snapshot.pinned,
            pinned_setting: snapshot.pinned_setting,
            pacing: snapshot.pacing,
            correction_source: snapshot.correction_source,
            correction_class: snapshot.correction_class,
            corrects_position: snapshot.corrects_position,
            scan,
        },
    }
}

fn evaluated_range(record: &IntakeRecord, snapshot: &FrameSnapshot) -> [i16; 2] {
    let [shift_low, shift_high] = record.range_shift;
    let far = if shift_high > shift_low { record.range[1].wrapping_add(shift_low).wrapping_sub(shift_high) } else { record.range[1] };
    let extension = if snapshot.adjustment != 0 { snapshot.level_offset.wrapping_sub(100) } else { snapshot.level_offset };
    [record.range[0], far.wrapping_add(extension)]
}

fn frame_program(record: &IntakeRecord, snapshot: &FrameSnapshot, ports: &mut impl IntakePorts) -> u32 {
    let source = match record.program_source {
        1 => u32::from(record.record_step),
        2 => snapshot.step,
        _ => ports.lens_step(record.lens_kind, snapshot.lens),
    };
    let level_word = ports.program_level_word(record.lens_kind, record.range[1], record.program_word, source);
    let planner = if record.mode == 6 { ports.planner_word() } else { 0 };
    ports.selected_program(ProgramInputs {
        selection: record.selection,
        second_result: record.second_result,
        window_status: record.window_status,
        profile_class: record.profile_class,
        resource: record.resource,
        record_20b0: record.record_20b0,
        planner,
        level_word,
    })
}

fn scan_after(view: IntakeView, frame_mode: u32, ports: &mut impl IntakePorts) -> bool {
    if view.phase != Phase::PeakSearch {
        return view.scan;
    }
    if view.samples == 0 && frame_mode == 0 {
        view.scan || ports.peak_threshold_reached()
    } else if frame_mode == 1 {
        false
    } else {
        view.scan
    }
}

/// 0xf408b4.
fn scan_evaluation(phase: Phase, scan: bool, focus_value: i16, [upper, lower]: [i8; 2]) -> ScanEvaluation {
    if !matches!(phase, Phase::PeakSearch | Phase::Return | Phase::Approach) {
        return ScanEvaluation::Plain;
    }
    let value = i32::from(focus_value);
    if value <= 10 * i32::from(lower) {
        ScanEvaluation::Plain
    } else if value <= 10 * i32::from(upper) || scan {
        ScanEvaluation::Scanned
    } else {
        ScanEvaluation::Scan
    }
}

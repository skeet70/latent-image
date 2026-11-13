//! A platform for the contrast search's host tests: settings and port answers the test sets,
//! and the effects the search leaves, in order.

use crate::contrast::settings::{Setting, StepKind};
use crate::contrast::{
    CorrectedPosition, CorrectionQuery, FitPoint, FocusLog, FocusLogEntry, FocusOffer, FocusStatus, FocusTrace, LensIdentity, LensMove, LimitKind,
    LimitQuery, MonitorStepQuery, PeakSearchTrace, PinnedStatus, Platform, RetainedFocus, ScanPlan, ScanQuery, StageRecord, Step, StepQuery,
    TableOffset, TransitionTrace,
};
use crate::intake::{FrameSnapshot, IntakePorts, IntakeRecord, ProgramInputs, RecordReads, Zone};
use std::vec::Vec;
use xm1_saclac_controllers::search::LensPosition;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Effect {
    Stage(StageRecord),
    Status(FocusStatus),
    PinnedStatus(PinnedStatus),
    ClaimStatistics,
    ResetLog,
    ClaimOffer,
    Trace(LensPosition, LensPosition),
    Outer(LimitQuery),
    Offset(TableOffset),
    ScanRefresh(bool),
    PlanScan(ScanQuery),
    Log(FocusLog),
    Debug(Vec<u8>, u8, u8),
    PeakTrace(PeakSearchTrace),
    FocusTrace(FocusTrace),
    Transition(TransitionTrace),
    Bias(LensIdentity, LensMove, LensPosition),
}

pub(crate) struct Fake {
    pub effects: Vec<Effect>,
    pub settings: Vec<(Setting, u32)>,
    pub position: LensPosition,
    pub lens: LensIdentity,
    pub selection: u8,
    pub offer: FocusOffer,
    pub retained: RetainedFocus,
    pub selected: u32,
    pub profile: u32,
    pub kind: LimitKind,
    pub pinned: bool,
    pub tracing: bool,
    pub detailed_tracing: bool,
    pub steps: Vec<(StepKind, Step)>,
    /// What 0xfa9320 answers: a channel rose.
    pub rose: bool,
    /// The series length from which 0xfa936c reports a passed peak.
    pub passed_after: usize,
    pub lens_move: Option<LensMove>,
    pub bias: i16,
    pub newest_log: FocusLogEntry,
    pub snapshot: FrameSnapshot,
    pub record: IntakeRecord,
}

impl Fake {
    pub(crate) fn new() -> Self {
        Self {
            effects: Vec::new(),
            settings: Vec::new(),
            position: LensPosition(-30),
            lens: LensIdentity(0),
            selection: 1,
            offer: FocusOffer(0),
            retained: RetainedFocus { valid: 1, spread: 0, position: LensPosition(100), profile: 9 },
            selected: 0,
            profile: 9,
            kind: LimitKind(0),
            pinned: false,
            tracing: false,
            detailed_tracing: false,
            steps: Vec::new(),
            rose: false,
            passed_after: usize::MAX,
            lens_move: None,
            bias: 0,
            newest_log: FocusLogEntry::default(),
            snapshot: FrameSnapshot {
                status: 5,
                windows: [[0; 2]; 2],
                size: [0; 2],
                extent: [0; 2],
                timing: 0,
                step: 0,
                zone: Zone(0),
                levels: [0; 2],
                adjustment: 0,
                level_offset: 0,
                lens: LensIdentity(0),
                selected: 0,
                frame_mode: 0,
                pinned_setting: 0,
                pinned: 0,
                pacing: [0; 2],
                mark: 0,
                correction_source: 0,
                correction_class: 0,
                corrects_position: 0,
            },
            record: IntakeRecord::default(),
        }
    }

    pub(crate) fn set(&mut self, setting: Setting, value: u32) { self.settings.push((setting, value)); }

    pub(crate) fn transitions(&self) -> Vec<TransitionTrace> {
        self.effects.iter().filter_map(|effect| if let Effect::Transition(trace) = effect { Some(*trace) } else { None }).collect()
    }

    pub(crate) fn logs(&self) -> Vec<FocusLogEntry> {
        self.effects.iter().filter_map(|effect| if let Effect::Log(log) = effect { Some(log.entry) } else { None }).collect()
    }
}

impl Platform for Fake {
    fn lens_position(&mut self) -> LensPosition { self.position }
    fn lens_identity(&mut self) -> LensIdentity { self.lens }
    fn table_count(&mut self) -> u32 { 4 }
    fn limit_kind(&mut self) -> LimitKind { self.kind }
    fn pinned_table(&mut self) -> bool { self.pinned }
    fn table_position(&mut self) -> u16 { 7 }
    fn lens_flags(&mut self) -> u32 { 0 }
    fn record_auxiliary(&mut self) -> u32 { 0 }
    fn lens_state(&mut self) -> u32 { 2 }
    fn lens_bias(&mut self, _: LensIdentity) -> u16 { 5 }
    fn outer_limits(&mut self, query: LimitQuery) -> (LensPosition, LensPosition) {
        self.effects.push(Effect::Outer(query));
        (LensPosition(-200), LensPosition(90))
    }
    fn inner_limits(&mut self, _: LimitQuery) -> (LensPosition, LensPosition) { (LensPosition(-150), LensPosition(60)) }
    fn table_offset(&mut self, _: LensIdentity, kind: TableOffset) -> u16 {
        self.effects.push(Effect::Offset(kind));
        40
    }
    fn focus_offer(&mut self) -> FocusOffer { self.offer }
    fn claim_focus_offer(&mut self) { self.effects.push(Effect::ClaimOffer); }
    fn retained_focus(&mut self) -> RetainedFocus { self.retained }
    fn measurement_selection(&mut self) -> u8 { self.selection }
    fn record_selected(&mut self) -> u32 { self.selected }
    fn record_profile(&mut self) -> u32 { self.profile }
    fn publish_status(&mut self, status: FocusStatus) { self.effects.push(Effect::Status(status)); }
    fn store_stage(&mut self, stage: StageRecord) { self.effects.push(Effect::Stage(stage)); }
    fn claim_image_statistics(&mut self) { self.effects.push(Effect::ClaimStatistics); }
    fn reset_focus_log(&mut self) { self.effects.push(Effect::ResetLog); }
    fn tracing(&mut self) -> bool { self.tracing }
    fn trace_begin(&mut self, retained: LensPosition, focus: LensPosition) { self.effects.push(Effect::Trace(retained, focus)); }
    fn setting(&mut self, setting: Setting) -> u32 { self.settings.iter().rev().find(|(key, _)| *key == setting).map_or(0, |(_, value)| *value) }
    fn lens_mode(&mut self) -> u32 { 0 }
    fn lens_busy(&mut self) -> bool { false }
    fn lens_ready(&mut self) -> bool { true }
    fn area_levels(&mut self) -> [u16; 2] { [0; 2] }
    fn focus_correction(&mut self, _: CorrectionQuery) -> i16 { 0 }
    fn profile_period(&mut self, _: u32) -> u32 { 0 }
    fn record_class(&mut self) -> u32 { 0 }
    fn step(&mut self, query: StepQuery) -> Step {
        self.steps.iter().find(|(kind, _)| *kind == query.kind).map_or(Step { size: 0, rate: 0 }, |(_, step)| *step)
    }
    fn monitor_step(&mut self, query: MonitorStepQuery) -> u16 { query.step }
    fn request_scan_refresh(&mut self, requested: bool) { self.effects.push(Effect::ScanRefresh(requested)); }
    fn scan_refresh(&mut self) -> bool { false }
    fn plan_scan(&mut self, query: ScanQuery) -> ScanPlan {
        self.effects.push(Effect::PlanScan(query));
        ScanPlan { limits: [query.limits.low, query.limits.high], window: [0; 2], lead: 0, rate: query.rate }
    }
    fn corrected_position(&mut self, _: u16) -> CorrectedPosition { CorrectedPosition { frame: LensPosition(0), drive: LensPosition(0) } }
    fn drive_target(&mut self, _: LensIdentity, _: u16, low: LensPosition, _: LensPosition) -> LensPosition { low }
    fn below_threshold(&mut self, _: u32, _: u32, _: u32, _: u32) -> bool { self.rose }
    fn passed_peak(&mut self, series: &[u32], _: u32, _: bool) -> (bool, Option<u32>) {
        let passed = series.len() >= self.passed_after;
        (passed, passed.then(|| series.iter().copied().max().unwrap_or(0)))
    }
    fn fell_from_peak(&mut self, _: &[u32]) -> bool { false }
    fn contrast_log(&mut self, _: u32) -> u32 { 0 }
    fn fit_peak(&mut self, points: [FitPoint; 3]) -> LensPosition { LensPosition(points[1].position) }
    fn measurement_history(&mut self) -> Option<[Option<[u16; 2]>; 5]> { None }
    fn profile_focus(&mut self) -> i16 { 0 }
    fn detailed_tracing(&mut self) -> bool { self.detailed_tracing }
    fn trace_peak_search(&mut self, trace: PeakSearchTrace) { self.effects.push(Effect::PeakTrace(trace)); }
    fn focus_tracing(&mut self) -> bool { false }
    fn trace_focus(&mut self, trace: FocusTrace) { self.effects.push(Effect::FocusTrace(trace)); }
    fn minimum_step(&mut self, minimum: u32) -> u32 { minimum }
    fn profile_mode(&mut self) -> u32 { 0 }
    fn parameter_label(&mut self, _: u32) -> u16 { 0 }
    fn ticks(&mut self) -> u32 { 0 }
    fn append_focus_log(&mut self, log: &FocusLog) { self.effects.push(Effect::Log(*log)); }
    fn newest_focus_log(&mut self) -> FocusLogEntry { self.newest_log }
    fn show_debug_text(&mut self, text: &[u8], column: u8, row: u8) { self.effects.push(Effect::Debug(text.to_vec(), column, row)); }
    fn angular_rate(&mut self) -> u32 { 0 }
    fn request_angular_rate(&mut self) {}
    fn stage(&mut self) -> StageRecord { StageRecord::default() }
    fn status_snapshot(&mut self) -> [u32; 2] { [0; 2] }
    fn publish_pinned_status(&mut self, status: PinnedStatus) { self.effects.push(Effect::PinnedStatus(status)); }
    fn trace_transition(&mut self, trace: TransitionTrace) { self.effects.push(Effect::Transition(trace)); }
    fn lens_move(&mut self) -> Option<LensMove> { self.lens_move }
    fn interpolated_bias(&mut self, lens: LensIdentity, moved: LensMove, position: LensPosition) -> i16 {
        self.effects.push(Effect::Bias(lens, moved, position));
        self.bias
    }
}

impl RecordReads for Fake {
    fn selection_ratio(&mut self) -> [u32; 2] { [0; 2] }
    fn converted_decade(&mut self) -> u16 { 0 }
    fn zone_limit(&mut self, _: Zone) -> i16 { 0 }
    fn lens_level(&mut self, _: u32, _: LensIdentity) -> i16 { 0 }
}

impl IntakePorts for Fake {
    fn frame_snapshot(&mut self) -> FrameSnapshot { FrameSnapshot { lens: self.lens, ..self.snapshot } }
    fn record(&mut self) -> IntakeRecord { self.record }
    fn imaging_profile_class(&mut self) -> u32 { 0 }
    fn parameter_index(&mut self, parameter: u32) -> u32 { parameter }
    fn lens_identity(&mut self) -> LensIdentity { self.lens }
    fn lens_step(&mut self, _: u8, _: LensIdentity) -> u32 { 0 }
    fn program_level_word(&mut self, _: u8, _: i16, _: u32, _: u32) -> u16 { 0 }
    fn planner_word(&mut self) -> u32 { 0 }
    fn selected_program(&mut self, _: ProgramInputs) -> u32 { 0 }
    fn program_level(&mut self, _: u32, _: i16, _: LensIdentity) -> i32 { 0 }
    fn peak_threshold_reached(&mut self) -> bool { false }
    fn scan_thresholds(&mut self) -> [i8; 2] { [0; 2] }
}

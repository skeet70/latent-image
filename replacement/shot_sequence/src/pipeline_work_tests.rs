use std::vec::Vec;
use crate::boundary::{Client, ClientRef, Frame, Lens, LensPoint, LensReading, LensReference};
use crate::finish_work::{AreaPositions, CorrectionRecord, FinishPipeline};
use crate::pipeline_work::*;
use crate::ports::{HalfPressMode, PipelineEffect as E, PipelinePhase, PipelineQuery as Q, PipelineRequest as R};
use crate::preparation::{FrameHeader, HalfPressMask, HalfPressParameters, Prepared};
use crate::summary::FrameSummary;
use xm1_saclac_interface::pipeline::*;
use xm1_saclac_interface::stock::pipeline as payloads;
use xm1_saclac_interface::{stock, Completed, Completion, RecordHandle};

/// Offsets of the motion library's blocks the fixtures fill, as stock memory lays them out.
const SCAN_SAMPLING: usize = 0x30;
const SCAN_THRESHOLD: usize = 0x2c;
const SCAN_SELECTION_WORDS: usize = 0x24;
const SCAN_DECISION: usize = 0x94;
const SCAN_TONE: usize = 0x9c;
const SCAN_LIMITS: usize = 0x174;
const SCAN_SELECTION: usize = 0x180;
const SCAN_AREA_COUNT: usize = 0x134;
const SUBJECT_SCALE: usize = 0x1c;
const CAPTURE_WEIGHTING: usize = 0x18;

const FRAME: Frame = Frame(0x0100_0000);
const MASK_RELEASED: u32 = 1 << 2;

fn record() -> RecordHandle { stock::record_handle_from_a2(0x16a_0000) }

fn put(bytes: &mut [u8], offset: usize, value: u32) { bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }

fn plan() -> Plan {
    Plan {
        frame_flags: FrameFlags(0b11),
        subject: PlanBlock(1),
        capture: PlanBlock(2),
        lens: PlanBlock(3),
        scan: PlanBlock(4),
        image_buffer: 0x500,
        raw_result: PlanBlock(9),
        ..Plan::default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Call {
    Plan,
    Scan(ScanStore),
    LensDriven,
    RawResult(PlanBlock, RawCalculation),
    Work(WorkArea, WorkStore),
    Flush(WorkArea),
    Trace(PipelineTrace),
    RawFrame(RawFrameRequest),
    Frame(FrameRequest),
    ScanRequest(ScanRequest),
    Selection(SelectionRequest),
    ScanSelection,
    Output,
    Commit,
    Statistics(StatisticsSetup),
    StatisticsCommit,
    Weights(u32, StatisticsWeights),
    Tone(ToneRequest),
    Gradation(GradationRequest),
    Raw,
    Release,
    History(OutcomeHistory),
    Correction(CorrectionRecord),
    Outcome(OutcomeValue),
    AreaBlock(AreaSlot, u8),
    Positions(AreaPositions),
    Register(u8, u8),
    BoundFrame(Option<Frame>),
    Stamp(Frame),
    PipelineFrame(PipelineFrame),
    ClearFrameWork(Frame),
    MarkRestart,
    SelectChannel(u8),
    ChannelSample(i16, u8),
    ChannelSamples,
    BeginMode,
    FinishMode(u8),
    ConfigureExtended,
    ConfigureSecondary,
    Converted,
}

pub(super) struct World {
    calls: Vec<Call>,
    plan: Option<Plan>,
    subject: SubjectBlock,
    capture: CaptureBlock,
    lens: LensBlock,
    scan: ScanBlock,
    built: ScanBlock,
    settings: Vec<(PipelineSetting, u32)>,
    load_test: (u32, u32),
    registers: [u8; 256],
    restart: bool,
    work_marked: bool,
    resource: u32,
    channels: u8,
    samples: [i16; 8],
    sample: Option<i16>,
    snapshot: SampleSnapshot,
    focus: FocusResultOut,
    gated: bool,
    idle: bool,
    lens_position: u32,
    direction: u32,
}

impl World {
    pub(super) fn new() -> Self {
        Self {
            calls: Vec::new(),
            plan: Some(plan()),
            subject: SubjectBlock::default(),
            capture: CaptureBlock::default(),
            lens: LensBlock::default(),
            scan: ScanBlock::default(),
            built: ScanBlock::default(),
            settings: Vec::new(),
            load_test: (0, 0xff),
            registers: [0; 256],
            restart: false,
            work_marked: false,
            resource: 0,
            channels: 0,
            samples: [0; 8],
            sample: None,
            snapshot: SampleSnapshot::default(),
            focus: FocusResultOut::default(),
            gated: false,
            idle: false,
            lens_position: 0,
            direction: 0,
        }
    }

    fn setting_value(&self, setting: PipelineSetting) -> u32 { self.settings.iter().find(|(known, _)| *known == setting).map_or(0, |(_, value)| *value) }

    fn take(&mut self) -> Vec<Call> { core::mem::take(&mut self.calls) }

    fn has(&self, call: &Call) -> bool { self.calls.contains(call) }

    fn work_stores(&self) -> Vec<(WorkArea, WorkStore)> {
        self.calls.iter().filter_map(|call| if let Call::Work(area, store) = call { Some((*area, *store)) } else { None }).collect()
    }
}

impl PipelineLibrary for World {
    fn plan(&mut self, _input: &PlanInput, _candidates: &crate::subjects::Candidates, _selection_reference: u32) -> Option<Plan> {
        self.calls.push(Call::Plan);
        self.plan
    }
    fn refresh_channels(&mut self) {}
    fn channel_item_ready(&mut self, _record: RecordHandle) -> bool { true }
    fn apply_adjustment(&mut self, _adjustment: &[PlanBlock; 7], _pass: AdjustmentPass) {}
    fn adjustment_gated(&mut self) -> bool { self.gated }
    fn build_scan_plan(&mut self, _scan: PlanBlock) -> u32 {
        let phase = self.built.phase();
        if phase != 3 {
            self.scan = self.built;
            self.scan.0[SCAN_SELECTION..SCAN_SELECTION + 4].copy_from_slice(&self.built.0[0x20..0x24]);
        }
        phase
    }
    fn focus_result(&mut self, _scan: PlanBlock, _kind: ResultKind) -> FocusResultOut { self.focus }
    fn selected_value(&mut self) -> u32 { 0x77 }
    fn needs_raw(&mut self) -> bool { false }
    fn conclude_raw(&mut self) {}
    fn store_converted_input(&mut self) { self.calls.push(Call::Converted); }
    fn begin_scan(&mut self) {}
    fn active_channels(&mut self) -> u8 { self.channels }
    fn release_second_channel(&mut self) {}
    fn position_correction(&mut self) -> u32 { 0 }
    fn scaled_value(&mut self, _scan: PlanBlock, index: u8) -> u16 { u16::from(index) + 1000 }
    fn scaled_levels(&mut self, _scan: PlanBlock, value: u16, index: u8) -> [i16; TONE_LEVELS] { [value as i16 + i16::from(index); TONE_LEVELS] }
    fn store_channel_samples(&mut self, _scan: PlanBlock, _snapshot: &SampleSnapshot) { self.calls.push(Call::ChannelSamples); }
    fn store_channel_sample(&mut self, _scan: PlanBlock, sample: i16, index: u8) { self.calls.push(Call::ChannelSample(sample, index)); }
    fn select_channel(&mut self, channel: u8) { self.calls.push(Call::SelectChannel(channel)); }
    fn indexed_selection(&mut self, _record: RecordHandle) -> u32 { 0x33 }
    fn adjust_direction(&mut self, point: &LensPoint) -> i16 { point.position + 5 }
    fn phase_attribute(&mut self, phase: u32, _attribute: PhaseAttribute) -> bool { phase == 7 }
    fn request_raw(&mut self, _plan: &Plan, completion: Completion) {
        assert_eq!(completion, Completion::Calculation91);
        self.calls.push(Call::Raw);
    }
    fn request_tone(&mut self, request: &ToneRequest, _completion: Completion) { self.calls.push(Call::Tone(*request)); }
    fn request_gradation(&mut self, request: &GradationRequest, _completion: Completion) { self.calls.push(Call::Gradation(*request)); }
    fn release_calculation(&mut self) { self.calls.push(Call::Release); }
}

impl PlanMemory for World {
    fn subject(&mut self, _block: PlanBlock) -> SubjectBlock { self.subject }
    fn capture(&mut self, _block: PlanBlock) -> CaptureBlock { self.capture }
    fn lens(&mut self, _block: PlanBlock) -> LensBlock { self.lens }
    fn scan(&mut self, _block: PlanBlock) -> ScanBlock { self.scan }
    fn raw_selection(&mut self, _block: PlanBlock) -> u32 { 0 }
    fn raw_first_word(&mut self, _block: PlanBlock) -> u32 { 0x1f0 }
    fn store_scan(&mut self, _block: PlanBlock, store: ScanStore) {
        match store {
            ScanStore::Tone(tone) => self.scan.0[SCAN_TONE..SCAN_TONE + TONE_CALCULATION_BYTES].copy_from_slice(&tone.0),
            ScanStore::Gradation(gradation) => self.scan.0[0x108..0x168].copy_from_slice(&gradation.0),
            ScanStore::Selection(selection) => put(&mut self.scan.0, SCAN_SELECTION, selection),
            _ => {}
        }
        self.calls.push(Call::Scan(store));
    }
    fn mark_lens_driven(&mut self, _block: PlanBlock) { self.calls.push(Call::LensDriven); }
    fn store_raw_calculation(&mut self, block: PlanBlock, result: &RawCalculation) { self.calls.push(Call::RawResult(block, *result)); }
}

impl PipelineRecords for World {
    fn stamp_frame(&mut self, frame: Frame, _lens_reference: u32) { self.calls.push(Call::Stamp(frame)); }
    fn resource(&mut self, _record: RecordHandle) -> u32 { self.resource }
    fn resource_bank(&mut self, _record: RecordHandle) -> u32 { 0xb }
    fn outcome_history(&mut self, _record: RecordHandle) -> OutcomeHistory { OutcomeHistory([0xee; 0x4c]) }
    fn store_outcome_history(&mut self, _record: RecordHandle, history: &OutcomeHistory) { self.calls.push(Call::History(*history)); }
    fn store_outcome(&mut self, _record: RecordHandle, value: OutcomeValue) { self.calls.push(Call::Outcome(value)); }
    fn store_outcome_correction(&mut self, _record: RecordHandle, correction: &CorrectionRecord) { self.calls.push(Call::Correction(*correction)); }
    fn store_area_block(&mut self, _record: RecordHandle, slot: AreaSlot, areas: &AreaBlock) { self.calls.push(Call::AreaBlock(slot, areas.count)); }
    fn store_outcome_positions(&mut self, _record: RecordHandle, positions: &AreaPositions) { self.calls.push(Call::Positions(*positions)); }
    fn statistics_values(&mut self, _record: RecordHandle) -> StatisticsRecordValues { StatisticsRecordValues { selection: 0x44, ..StatisticsRecordValues::default() } }
    fn lens_value(&mut self, value: PipelineLens) -> u32 {
        match value {
            PipelineLens::Position => self.lens_position,
            PipelineLens::Drive => 0,
            PipelineLens::Direction => self.direction,
        }
    }
    fn position_code(&mut self, point: &LensPoint, _drive: u32) -> u16 { point.position as u16 + 1 }
    fn tone_level(&mut self, level: i16, _reference: u32, _state: u32, _geometry: u32, _mode: u32) -> u32 { (level + 100) as u32 }
    fn level_bounds(&mut self, _state: u32, _reference: u32) -> (u32, u32) { (3, 4) }
    fn level_value(&mut self, level: u32) -> u32 { level * 2 }
    fn channel_threshold(&mut self, _reference: u32, _state: u32, scaled: u32) -> u32 { scaled }
    fn correction_base(&mut self, _tolerance_kind: u32) -> u16 { 40 }
}

impl PipelineProfiles for World {
    fn requires_adjustment(&mut self, _profile: u32) -> bool { true }
    fn record_mode_available(&mut self, _mode: u32) -> bool { false }
    fn begin_record_mode(&mut self, _mode: u32) { self.calls.push(Call::BeginMode); }
    fn finish_record_mode(&mut self, threshold: u8) { self.calls.push(Call::FinishMode(threshold)); }
    fn configure_extended_operations(&mut self) { self.calls.push(Call::ConfigureExtended); }
    fn configure_secondary_operations(&mut self) { self.calls.push(Call::ConfigureSecondary); }
}

impl PipelinePeers for World {
    fn request_raw_frame(&mut self, request: &RawFrameRequest, _completion: Completion) { self.calls.push(Call::RawFrame(*request)); }
    fn request_frame(&mut self, request: &FrameRequest, _completion: Completion) { self.calls.push(Call::Frame(*request)); }
    fn request_scan(&mut self, request: &ScanRequest, progress: Completion, status: Completion) {
        assert_eq!((progress, status), (Completion::Image50, Completion::Image51));
        self.calls.push(Call::ScanRequest(*request));
    }
    fn request_selection(&mut self, request: SelectionRequest) { self.calls.push(Call::Selection(request)); }
    fn request_scan_selection(&mut self) { self.calls.push(Call::ScanSelection); }
    fn request_output(&mut self, _completion: Completion) { self.calls.push(Call::Output); }
    fn request_commit(&mut self, _completion: Completion) { self.calls.push(Call::Commit); }
    fn request_statistics(&mut self, setup: &StatisticsSetup, _completion: Completion) { self.calls.push(Call::Statistics(*setup)); }
    fn request_statistics_commit(&mut self, _completion: Completion) { self.calls.push(Call::StatisticsCommit); }
    fn store_statistics_weights(&mut self, target: u32, weights: &StatisticsWeights) { self.calls.push(Call::Weights(target, *weights)); }
    fn statistics_target(&mut self, _lens: PlanBlock) -> u32 { 0x7000 }
    fn capture_idle(&mut self) -> bool { self.idle }
    fn take_sample(&mut self, _level: u16, _targets: &[PlanBlock; 5]) -> Option<i16> { self.sample }
    fn channel_sample(&mut self, index: u8) -> i16 { self.samples[usize::from(index)] }
    fn sample_snapshot(&mut self) -> SampleSnapshot { self.snapshot }
}

impl PipelinePlatform for World {
    fn setting(&mut self, setting: PipelineSetting) -> u32 { self.setting_value(setting) }
    fn load_test_state(&mut self) -> u32 { self.load_test.0 }
    fn load_test_setting(&mut self) -> u32 { self.load_test.1 }
    fn load_test_register(&mut self, register: u8) -> u8 { self.registers[usize::from(register)] }
    fn set_load_test_register(&mut self, register: u8, value: u8) { self.calls.push(Call::Register(register, value)); }
    fn trace(&mut self, trace: PipelineTrace) { self.calls.push(Call::Trace(trace)); }
    fn stamp_pipeline_frame(&mut self, _frame: Frame, words: &PipelineFrame) { self.calls.push(Call::PipelineFrame(*words)); }
    fn clear_frame_work(&mut self, frame: Frame) { self.calls.push(Call::ClearFrameWork(frame)); }
    fn store_bound_frame(&mut self, frame: Option<Frame>) { self.calls.push(Call::BoundFrame(frame)); }
    fn restart_marked(&mut self) -> bool { self.restart }
    fn mark_restart(&mut self) {
        self.restart = true;
        self.calls.push(Call::MarkRestart);
    }
    fn store_work(&mut self, area: WorkArea, store: WorkStore) { self.calls.push(Call::Work(area, store)); }
    fn work_marked(&mut self, _area: WorkArea) -> bool { self.work_marked }
    fn flush_work(&mut self, area: WorkArea) { self.calls.push(Call::Flush(area)); }
}

impl Client for World {
    fn area_count(&mut self, _area_count: ClientRef) -> u32 { 0 }
    fn area(&mut self, _areas: ClientRef, _index: usize) -> u32 { 0 }
    fn write_lens_target(&mut self, _target: ClientRef, _point: LensPoint, _step: u16, _distance: u32) {}
    fn frame(&mut self, _slot: ClientRef) -> Option<Frame> { Some(FRAME) }
    fn clear_frame_progress(&mut self, _slot: ClientRef) {}
    fn announce_frame(&mut self, _frame: Frame, _header: FrameHeader) {}
    fn write_summary(&mut self, _frame: Frame, _summary: &FrameSummary) {}
}

impl Lens for World {
    fn read(&mut self, _reading: LensReading) -> u32 { 0 }
    fn take_change(&mut self) -> u32 { 0 }
    fn designated_point(&mut self) -> Option<LensPoint> { None }
    fn target_distance(&mut self, _point: LensPoint, _control: u32) -> u32 { 0 }
    fn target_step(&mut self, _point: LensPoint) -> u16 { 0 }
}

fn prepared(mode: u32, mask: u32) -> Prepared {
    Prepared {
        parameters: Some(HalfPressParameters {
            mode: HalfPressMode(mode),
            mask: HalfPressMask(mask),
            plan_option: 0x14,
            area_count: Some(ClientRef(0x18)),
            frame_slot: Some(ClientRef(0x2c)),
            ..HalfPressParameters::default()
        }),
        pipeline_gate: 2,
        ..Prepared::default()
    }
}

/// Runs `effect` with a shot state whose bound lens reference is 0x42.
fn effect(work: &mut PipelineWork, effect: E, prepared: &mut Prepared, world: &mut World) {
    use crate::state::StateMemory;
    let mut state = crate::state::ShotState::new(crate::state_tests::Memory::default());
    state.memory().write(crate::stock::state::REQUEST_WORD, &0x42u32.to_le_bytes());
    work.effect(effect, prepared, &mut state, world);
}

fn prepare(work: &mut PipelineWork, prepared: &mut Prepared, world: &mut World) {
    effect(work, E::BindRecord(record()), prepared, world);
    effect(work, E::Prepare(record()), prepared, world);
}

fn deliver(work: &mut PipelineWork, world: &mut World, completion: Completion, payload: &[u8]) {
    work.receive(Completed { completion, parameters: stock::parameters(payload) }, world);
}

fn work_area() -> WorkArea { crate::stock::pipeline::work_area(FRAME) }

#[test]
fn only_a_planned_half_press_has_a_ready_frame() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    let mut prepared = prepared(1, MASK_RELEASED);
    prepare(&mut work, &mut prepared, &mut world);
    assert!(world.has(&Call::Trace(PipelineTrace::Prepare)));
    assert!(work.query(Q::FrameReady, &mut world));
    assert!(!work.query(Q::FinishOnly, &mut world));

    let (mut unplanned, mut world) = (PipelineWork::new(), World::new());
    prepare(&mut unplanned, &mut self::prepared(1, 0), &mut world);
    assert!(!world.calls.iter().any(|call| matches!(call, Call::Plan)));
    assert!(!unplanned.query(Q::FrameReady, &mut world));
}

#[test]
fn the_bind_stamps_the_frame_names_it_and_closes_the_preparation_gate() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    world.lens_position = 0x1234;
    let mut prepared = prepared(0, 0);
    effect(&mut work, E::BindRecord(record()), &mut prepared, &mut world);
    assert_eq!(world.take(), [
        Call::BoundFrame(Some(FRAME)),
        Call::Stamp(FRAME),
        Call::PipelineFrame(PipelineFrame { resource: 0, bank: 0xb, lens_position: 0x1234, lens_reference: 0x42 }),
    ]);
    assert_eq!(prepared.pipeline_gate, 0);
    assert_eq!(work.pipeline_record(), Some(record()));

    world.resource = 1;
    effect(&mut work, E::BindRecord(record()), &mut prepared, &mut world);
    assert!(world.has(&Call::ClearFrameWork(FRAME)));

    let mut finish_only = self::prepared(6, 0);
    world.calls.clear();
    effect(&mut work, E::BindRecord(record()), &mut finish_only, &mut world);
    assert!(world.calls.is_empty());
    assert_eq!(finish_only.pipeline_gate, 0);
}

#[test]
fn a_finish_only_half_press_keeps_the_previous_pipeline_state() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    let mut planned = prepared(1, MASK_RELEASED);
    prepare(&mut work, &mut planned, &mut world);
    effect(&mut work, E::SetUploadDone(true), &mut planned, &mut world);

    let mut finish_only = prepared(6, 0);
    effect(&mut work, E::Prepare(record()), &mut finish_only, &mut world);
    assert!(work.query(Q::FinishOnly, &mut world));
    assert!(work.upload_done());
    assert!(work.relative_first());

    let mut next = prepared(5, 0);
    effect(&mut work, E::Prepare(record()), &mut next, &mut world);
    assert!(!work.upload_done());
    assert!(work.query(Q::SkipLensFinish, &mut world));
}

#[test]
fn the_finish_reads_pending_and_done_statistics_and_the_reset_keeps_the_pending_one() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    let mut prepared = prepared(1, MASK_RELEASED);
    prepare(&mut work, &mut prepared, &mut world);
    assert!(!work.relative_first());
    effect(&mut work, E::SetStatisticsPending(true), &mut prepared, &mut world);
    effect(&mut work, E::SetUploadDone(true), &mut prepared, &mut world);
    effect(&mut work, E::SetFinishDone, &mut prepared, &mut world);
    assert!(work.statistics_pending() && work.upload_done() && work.query(Q::FinishDone, &mut world));

    effect(&mut work, E::Reset, &mut prepared, &mut world);
    assert!(world.has(&Call::Converted));
    assert!(work.statistics_pending());
    assert!(!work.upload_done() && !work.query(Q::FinishDone, &mut world) && !work.query(Q::FrameReady, &mut world));
    assert!(work.relative_first());
}

#[test]
fn completion_words_decide_the_raw_and_gradation_calculations() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    prepare(&mut work, &mut prepared(1, MASK_RELEASED), &mut world);
    deliver(&mut work, &mut world, Completion::Image4e, &payloads::raw_check_payload(RawCheck(0)));
    deliver(&mut work, &mut world, Completion::Image51, &payloads::tone_status_payload(ToneStatus(3)));
    assert!(work.query(Q::RawCalculationNeeded, &mut world));
    assert!(!work.query(Q::GradationNeeded, &mut world));
    deliver(&mut work, &mut world, Completion::Image4e, &payloads::raw_check_payload(RawCheck(2)));
    deliver(&mut work, &mut world, Completion::Image51, &payloads::tone_status_payload(ToneStatus(0)));
    assert!(!work.query(Q::RawCalculationNeeded, &mut world));
    assert!(work.query(Q::GradationNeeded, &mut world));

    deliver(&mut work, &mut world, Completion::Calculation91, &[7; 20]);
    assert!(world.has(&Call::RawResult(PlanBlock(9), RawCalculation([7; 20]))));
}

#[test]
fn the_tone_progress_takes_the_capture_sample_or_skips_the_tone_calculation() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    prepare(&mut work, &mut prepared(1, MASK_RELEASED), &mut world);
    deliver(&mut work, &mut world, Completion::Image50, &payloads::tone_progress_payload(ToneProgress { status: 1, level: 9 }));
    world.sample = Some(-40);
    assert!(work.query(Q::ToneSkipped, &mut world));
    deliver(&mut work, &mut world, Completion::Image50, &payloads::tone_progress_payload(ToneProgress { status: 0, level: 9 }));
    assert!(!work.query(Q::ToneSkipped, &mut world));
    world.sample = None;
    assert!(work.query(Q::ToneSkipped, &mut world));
    assert!(!work.query(Q::ToneRequested, &mut world));
}

#[test]
fn the_tone_result_keeps_reached_flags_and_marks_a_scanned_tone_once() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    let mut prepared = prepared(1, MASK_RELEASED);
    prepare(&mut work, &mut prepared, &mut world);
    put(&mut world.scan.0, SCAN_LIMITS, 1);
    put(&mut world.scan.0, SCAN_TONE + 0x2c, 1);
    deliver(&mut work, &mut world, Completion::Image50, &payloads::tone_progress_payload(ToneProgress { status: 0, level: 3 }));
    world.sample = Some(-12);
    work.query(Q::ToneSkipped, &mut world);
    world.take();

    let mut result = [0u8; TONE_CALCULATION_BYTES];
    put(&mut result, 0x04, 1);
    put(&mut result, 0x20, 0x55);
    put(&mut result, 0x24, 0x66);
    deliver(&mut work, &mut world, Completion::Calculation92, &result);
    let mut merged = result;
    put(&mut merged, 0x2c, 1);
    assert_eq!(world.take(), [
        Call::Scan(ScanStore::SecondToneLimit(0x66)),
        Call::Scan(ScanStore::Tone(ToneCalculation(merged))),
        Call::Work(work_area(), WorkStore::ToneMark(-12)),
    ]);

    world.work_marked = true;
    deliver(&mut work, &mut world, Completion::Calculation92, &result);
    assert!(world.work_stores().is_empty());
}

#[test]
fn a_gradation_result_reaches_the_threshold_from_the_selected_snapshot_sample() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    let mut prepared = prepared(1, MASK_RELEASED);
    prepare(&mut work, &mut prepared, &mut world);
    world.snapshot.0[0xa0] = 2;
    put(&mut world.snapshot.0, 4 + 8 * 2, 500);
    put(&mut world.scan.0, SCAN_SAMPLING, 1);
    put(&mut world.scan.0, SCAN_THRESHOLD, 500);
    effect(&mut work, E::Request(R::Calc05), &mut prepared, &mut world);
    world.take();
    deliver(&mut work, &mut world, Completion::Calculation93, &[1; 96]);
    assert_eq!(world.take(), [Call::Scan(ScanStore::Gradation(GradationCalculation([1; 96]))), Call::Scan(ScanStore::GradationReached)]);
    put(&mut world.scan.0, SCAN_THRESHOLD, 501);
    deliver(&mut work, &mut world, Completion::Calculation93, &[1; 96]);
    assert!(!world.has(&Call::Scan(ScanStore::GradationReached)));
}

#[test]
fn the_released_frame_is_flushed_traced_after_and_later_stores_use_its_other_view() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    let mut prepared = prepared(1, MASK_RELEASED);
    prepare(&mut work, &mut prepared, &mut world);
    world.take();
    effect(&mut work, E::SetPhase(PipelinePhase::First), &mut prepared, &mut world);
    effect(&mut work, E::ReleaseFrame, &mut prepared, &mut world);
    effect(&mut work, E::FrameReleased, &mut prepared, &mut world);
    effect(&mut work, E::SetPhase(PipelinePhase::Third), &mut prepared, &mut world);
    let calls = world.take();
    let released = crate::stock::pipeline::released(work_area());
    assert_eq!(calls, [
        Call::Work(work_area(), WorkStore::Phase(PipelinePhase::First)),
        Call::Flush(work_area()),
        Call::Trace(PipelineTrace::FrameReleased),
        Call::Work(released, WorkStore::Phase(PipelinePhase::Third)),
    ]);
    assert_ne!(released, work_area());
}

#[test]
fn the_configured_outcome_is_published_to_the_record_only_when_the_client_asked() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    let point = LensPoint { position: 300, detail: 2, reference: LensReference(4), state: 5, kind: 6 };
    world.focus = FocusResultOut { status: 0, point, pending: 9 };
    let mut asked = prepared(1, MASK_RELEASED);
    prepare(&mut work, &mut asked, &mut world);
    world.take();
    effect(&mut work, E::Configure { record: record(), first: true }, &mut asked, &mut world);
    assert_eq!(work.frame_point(), point);
    let calls = world.take();
    let Some(Call::History(history)) = calls.first() else { panic!("history first") };
    assert_eq!(history.0[8..12], 1u32.to_le_bytes());
    assert!(calls.contains(&Call::Outcome(OutcomeValue::PositionCode(301))));
    assert!(calls.contains(&Call::Outcome(OutcomeValue::AdjustedPoint(LensPoint { position: 305, ..point }))));
    assert!(calls.contains(&Call::AreaBlock(AreaSlot::First, 1)));
    assert!(!calls.contains(&Call::AreaBlock(AreaSlot::Second, 1)));
    let Some(Call::Correction(correction)) = calls.iter().find(|call| matches!(call, Call::Correction(_))) else { panic!("correction stored") };
    assert_eq!((correction.enabled, correction.pending), (1, 9));
    let Some(Call::Positions(positions)) = calls.last() else { panic!("positions last") };
    assert_eq!((positions.last, positions.positions[0], positions.positions[1]), (1, 300, 300));

    let mut unasked = prepared(1, 0);
    effect(&mut work, E::Configure { record: record(), first: true }, &mut unasked, &mut world);
    assert!(matches!(world.take()[..], [Call::History(_)]));
}

#[test]
fn a_published_outcome_has_the_reply_notify_the_lens_when_the_client_asked_for_a_notification() {
    use crate::state::StateMemory;
    for (mask, notice) in [(MASK_RELEASED, 1), (1 << 3, 1), (0, 0)] {
        let (mut work, mut world) = (PipelineWork::new(), World::new());
        prepare(&mut work, &mut prepared(1, MASK_RELEASED), &mut world);
        let mut state = crate::state::ShotState::new(crate::state_tests::Memory::default());
        state.memory().write(crate::stock::state::REQUEST_WORD, &0x42u32.to_le_bytes());
        work.effect(E::Configure { record: record(), first: true }, &mut prepared(1, mask), &mut state, &mut world);
        assert_eq!(crate::state_tests::reply_lens_notice(&mut state), notice, "mask {mask:#x}");
    }
}

#[test]
fn a_missed_focus_has_no_position_code() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    world.focus = FocusResultOut { status: 2, ..FocusResultOut::default() };
    let mut asked = prepared(1, MASK_RELEASED);
    prepare(&mut work, &mut asked, &mut world);
    effect(&mut work, E::Configure { record: record(), first: false }, &mut asked, &mut world);
    assert!(world.has(&Call::Outcome(OutcomeValue::PositionCode(0xffff))));
}

#[test]
fn the_load_test_window_reports_whether_the_lens_stands_inside() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    world.load_test = (1, 99);
    world.focus = FocusResultOut { status: 0, point: LensPoint { position: 0x1234, ..LensPoint::default() }, pending: 0 };
    world.registers[0x84..0x88].copy_from_slice(&[0x01, 0x00, 0xff, 0x9c]);
    world.settings.push((PipelineSetting::LoadTestTolerance, 5));
    world.lens_position = 0x120;
    let mut prepared = prepared(1, MASK_RELEASED);
    prepare(&mut work, &mut prepared, &mut world);
    let registers = |world: &mut World| world.take().into_iter().filter(|call| matches!(call, Call::Register(..))).collect::<Vec<_>>();
    registers(&mut world);
    effect(&mut work, E::Configure { record: record(), first: true }, &mut prepared, &mut world);
    assert_eq!(registers(&mut world), [Call::Register(0x88, 0x12), Call::Register(0x89, 0x34), Call::Register(0x8a, 0)]);
    world.lens_position = 0x200;
    effect(&mut work, E::Configure { record: record(), first: true }, &mut prepared, &mut world);
    assert_eq!(registers(&mut world).last(), Some(&Call::Register(0x8a, 2)));
}

#[test]
fn the_fallback_publishes_up_to_three_positions_and_every_area_block() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    let mut prepared = prepared(1, MASK_RELEASED);
    prepare(&mut work, &mut prepared, &mut world);
    world.scan.0[SCAN_AREA_COUNT] = 4;
    for (index, position) in [10u16, 20, 30, 40].iter().enumerate() { world.scan.0[0x138 + 8 * index..0x13a + 8 * index].copy_from_slice(&position.to_le_bytes()); }
    world.take();
    effect(&mut work, E::PrepareFallback, &mut prepared, &mut world);
    assert!(world.has(&Call::AreaBlock(AreaSlot::Third, 4)));
    let Some(Call::Positions(positions)) = world.calls.last() else { panic!("positions last") };
    assert_eq!((positions.last, positions.positions), (3, [10, 20, 30, 10]));
}

#[test]
fn scan_requests_choose_initial_speed_and_lens_correction() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    let mut prepared = prepared(1, MASK_RELEASED);
    prepare(&mut work, &mut prepared, &mut world);
    put(&mut world.built.0, 0x4, 1);
    put(&mut world.built.0, 0x8, 5);
    put(&mut world.built.0, 0x10, 100 | 90 << 16);
    put(&mut world.built.0, 0x20, 1);
    put(&mut world.built.0, SCAN_SELECTION_WORDS + 4, 0xabc);
    world.settings.push((PipelineSetting::BoundedSelection, 9));
    world.settings.push((PipelineSetting::CorrectionTolerance, 30));
    world.lens_position = 80;
    world.direction = 0;
    effect(&mut work, E::PrepareFrame, &mut prepared, &mut world);
    effect(&mut work, E::Request(R::Image1f), &mut prepared, &mut world);
    let Some(Call::ScanRequest(request)) = world.calls.last() else { panic!("scan requested") };
    assert_eq!((request.start, request.positions[0], request.selection_word, request.selection_setting), (1, 100, 0xabc, 0));
    assert_eq!(request.correction, 40 + 30);
    world.lens_position = 120;
    effect(&mut work, E::PrepareFrame, &mut prepared, &mut world);
    effect(&mut work, E::Request(R::Image1f), &mut prepared, &mut world);
    let Some(Call::ScanRequest(request)) = world.calls.last() else { panic!("scan requested") };
    assert_eq!(request.correction, 40 + 30 - 20);
    world.direction = 1;
    effect(&mut work, E::PrepareFrame, &mut prepared, &mut world);
    effect(&mut work, E::Request(R::Image1f), &mut prepared, &mut world);
    let Some(Call::ScanRequest(request)) = world.calls.last() else { panic!("scan requested") };
    assert_eq!(request.correction, 0);
    assert!(work.query(Q::ToneResultSet, &mut world));

}

#[test]
fn the_tone_request_takes_scaled_levels_when_the_scan_starts_and_the_matched_level_otherwise() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    let mut prepared = prepared(1, MASK_RELEASED);
    prepare(&mut work, &mut prepared, &mut world);
    deliver(&mut work, &mut world, Completion::Image50, &payloads::tone_progress_payload(ToneProgress { status: 0, level: 0 }));
    world.sample = Some(-7);
    work.query(Q::ToneSkipped, &mut world);
    put(&mut world.scan.0, 0x38, 0x11);
    world.gated = true;
    effect(&mut work, E::ArmFrame, &mut prepared, &mut world);
    effect(&mut work, E::Request(R::Calc04), &mut prepared, &mut world);
    let Some(Call::Tone(request)) = world.calls.last() else { panic!("tone requested") };
    assert_eq!((request.levels, request.offset, request.window[0]), ([-7; TONE_LEVELS], 0, 0));

    put(&mut world.built.0, 0x4, 1);
    effect(&mut work, E::PrepareFrame, &mut prepared, &mut world);
    world.settings.push((PipelineSetting::ToneOffset, 1));
    effect(&mut work, E::Request(R::Calc04), &mut prepared, &mut world);
    let Some(Call::Tone(request)) = world.calls.last() else { panic!("tone requested") };
    assert_eq!((request.levels[0], request.offset), (1000, (93 * 2 - 7) as u16));
}

#[test]
fn the_gradation_request_narrows_still_channels_and_hands_on_the_samples() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    let mut prepared = prepared(1, MASK_RELEASED);
    prepare(&mut work, &mut prepared, &mut world);
    put(&mut world.built.0, 0x4, 1);
    effect(&mut work, E::PrepareFrame, &mut prepared, &mut world);
    put(&mut world.scan.0, SCAN_SAMPLING, 1);
    put(&mut world.subject.0, SUBJECT_SCALE, 10);
    world.settings.push((PipelineSetting::ExposureScale, 0x30));
    world.settings.push((PipelineSetting::ChannelThreshold, 2));
    world.snapshot.0[0xa0] = 0;
    put(&mut world.snapshot.0, 4, 1_000_000);
    world.channels = 4;
    world.samples = [100, 150, 200, 205, 0, 0, 0, 0];
    world.take();
    effect(&mut work, E::Request(R::Calc05), &mut prepared, &mut world);
    let calls = world.take();
    assert!(calls.contains(&Call::SelectChannel(3)));
    assert!(calls.contains(&Call::Work(work_area(), WorkStore::ChannelWindow { last: 3, count: 4 })));
    assert!(calls.contains(&Call::Scan(ScanStore::FirstSample(100))));
    assert!(calls.contains(&Call::ChannelSample(205, 3)));
    let Some(Call::Gradation(request)) = calls.last() else { panic!("gradation requested") };
    assert_eq!((request.sampled, request.exposure, request.gated), (true, 4, false));
}

#[test]
fn weighted_statistics_send_their_weights_before_the_request() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    let mut prepared = prepared(1, MASK_RELEASED);
    prepare(&mut work, &mut prepared, &mut world);
    world.settings.extend([(PipelineSetting::ChannelStatistics, 1), (PipelineSetting::WeightDivisor, 1), (PipelineSetting::Weight(0), 10)]);
    put(&mut world.capture.0, CAPTURE_WEIGHTING, 1);
    put(&mut world.built.0, 0x58, 4 << 8);
    effect(&mut work, E::PrepareFrame, &mut prepared, &mut world);
    effect(&mut work, E::StartFrame, &mut prepared, &mut world);
    assert!(work.query(Q::StatisticsReady, &mut world));
    world.take();
    effect(&mut work, E::Request(R::Statistics03), &mut prepared, &mut world);
    let calls = world.take();
    let mut weights = [0; STATISTICS_WEIGHTS];
    weights[0] = 20;
    assert_eq!(calls[0], Call::Work(work_area(), WorkStore::Weights(weights)));
    assert!(matches!(calls[1], Call::Weights(0x7000, sent) if sent.weights == weights));
    let Call::Statistics(setup) = calls[2] else { panic!("statistics requested") };
    assert_eq!((setup.weighted, setup.selection[0], setup.capture.words[3]), (true, 0x33, 0x44));

    effect(&mut work, E::SetToneResult(true), &mut prepared, &mut world);
    work.statistics_progress(&mut world);
    assert_eq!(world.take(), [Call::Release]);
    effect(&mut work, E::SetToneResult(false), &mut prepared, &mut world);
    work.statistics_progress(&mut world);
    assert!(world.take().is_empty());
}

#[test]
fn the_selection_request_follows_a_changed_selection_and_drives_the_lens_after_a_long_move() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    let mut prepared = prepared(1, MASK_RELEASED);
    prepare(&mut work, &mut prepared, &mut world);
    put(&mut world.scan.0, SCAN_SAMPLING, 1);
    put(&mut world.scan.0, SCAN_TONE + 4, 1);
    put(&mut world.scan.0, SCAN_SELECTION_WORDS + 4, 0x210);
    world.settings.push((PipelineSetting::SelectionOverride, 2));
    world.channels = 2;
    world.take();
    effect(&mut work, E::Request(R::Image20), &mut prepared, &mut world);
    let calls = world.take();
    assert_eq!(calls[..2], [
        Call::Work(work_area(), WorkStore::ChannelMarks { channel: 1, marks: [3; 4] }),
        Call::Work(work_area(), WorkStore::ChannelMarks { channel: 2, marks: [0xfd; 4] }),
    ]);
    assert_eq!(calls[2..], [Call::Scan(ScanStore::Selection(1)), Call::Selection(SelectionRequest(0x210))]);

    effect(&mut work, E::Request(R::Image20), &mut prepared, &mut world);
    assert!(!world.calls.iter().any(|call| matches!(call, Call::Selection(_))));

    put(&mut world.scan.0, SCAN_TONE + 4, 0);
    world.scan.0[0x12..0x14].copy_from_slice(&50u16.to_le_bytes());
    world.scan.0[0x1c..0x1e].copy_from_slice(&10u16.to_le_bytes());
    put(&mut world.scan.0, SCAN_SELECTION_WORDS + 8, 0x220);
    world.settings.extend([(PipelineSetting::SelectionTable(0), 2 << 2), (PipelineSetting::DrivingDistance, 60)]);
    world.take();
    effect(&mut work, E::Request(R::Image20), &mut prepared, &mut world);
    assert!(!world.calls.iter().any(|call| matches!(call, Call::Selection(_))));
    world.settings.retain(|(setting, _)| *setting != PipelineSetting::DrivingDistance);
    world.take();
    effect(&mut work, E::Request(R::Image20), &mut prepared, &mut world);
    assert_eq!(world.calls[world.calls.len() - 3..], [Call::Scan(ScanStore::Selection(2)), Call::Selection(SelectionRequest(0x220)), Call::LensDriven]);
}

#[test]
fn the_tone_decision_records_a_final_decision_or_continues() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    let mut prepared = prepared(1, MASK_RELEASED);
    prepare(&mut work, &mut prepared, &mut world);
    assert!(work.query(Q::ToneCalculationFinal, &mut world));

    put(&mut world.scan.0, SCAN_TONE, 1);
    put(&mut world.scan.0, SCAN_DECISION, 1);
    world.settings.push((PipelineSetting::ToneMask, 1));
    assert!(work.query(Q::ToneCalculationFinal, &mut world));
    assert!(world.has(&Call::Scan(ScanStore::Decision(3))));

    put(&mut world.scan.0, SCAN_TONE + 0x20, 1);
    world.take();
    assert!(!work.query(Q::ToneCalculationFinal, &mut world));
    assert_eq!(world.take(), [Call::Scan(ScanStore::Decision(3)), Call::Scan(ScanStore::Decision(2))]);
}

#[test]
fn a_restart_keeps_the_tone_state_only_in_the_keeping_phases() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    let mut prepared = prepared(1, MASK_RELEASED);
    prepare(&mut work, &mut prepared, &mut world);
    assert!(!work.query(Q::ToneRestart, &mut world));
    put(&mut world.built.0, 0x8, 5);
    effect(&mut work, E::PrepareFrame, &mut prepared, &mut world);
    assert!(work.query(Q::ToneRestart, &mut world));
    assert!(!work.query(Q::Restarted, &mut world));
    effect(&mut work, E::MarkRestart, &mut prepared, &mut world);
    assert!(work.query(Q::Restarted, &mut world));
}

#[test]
fn image_requests_carry_the_plan_blocks_and_their_configuration_steps() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    let mut prepared = prepared(1, MASK_RELEASED);
    prepare(&mut work, &mut prepared, &mut world);
    world.take();
    effect(&mut work, E::Request(R::Image1d), &mut prepared, &mut world);
    effect(&mut work, E::Request(R::Image1e), &mut prepared, &mut world);
    effect(&mut work, E::Request(R::Image21), &mut prepared, &mut world);
    effect(&mut work, E::Request(R::Image22), &mut prepared, &mut world);
    effect(&mut work, E::Request(R::Calc03), &mut prepared, &mut world);
    let calls = world.take();
    assert_eq!(calls[0], Call::Trace(PipelineTrace::RawRequested));
    assert!(matches!(calls[1], Call::RawFrame(request) if request.image_buffer == 0x500 && request.raw_word == 0x1f0));
    assert_eq!(calls[2], Call::ConfigureExtended);
    assert!(matches!(calls[3], Call::Frame(request) if request.image_buffer == 0x500));
    assert_eq!(calls[4..], [Call::Scan(ScanStore::ScanRequested), Call::ScanSelection, Call::ConfigureSecondary, Call::Output, Call::Raw]);
}

#[test]
fn the_frame_synchronization_follows_the_profile_record_mode() {
    let (mut work, mut world) = (PipelineWork::new(), World::new());
    let mut prepared = prepared(1, MASK_RELEASED);
    world.idle = true;
    assert!(work.query(Q::CaptureIdle, &mut world));
    effect(&mut work, E::SynchronizeFrame, &mut prepared, &mut world);
    effect(&mut work, E::ResumeFrame, &mut prepared, &mut world);
    assert!(!world.has(&Call::BeginMode));
    assert!(!world.calls.iter().any(|call| matches!(call, Call::FinishMode(_))));
}

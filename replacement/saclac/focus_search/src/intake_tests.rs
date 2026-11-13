use crate::contrast::{LensIdentity, Phase, StageRecord};
use crate::intake::{take_in, FrameSnapshot, IntakePorts, IntakeRecord, IntakeView, ProgramInputs, RecordReads, Zone};
use crate::stock::intake::{frame_snapshot, FRAME_SNAPSHOT_BYTES};
use std::vec::Vec;
use xm1_saclac_interface::search::ScanEvaluation;

struct Fake {
    snapshot: FrameSnapshot,
    record: IntakeRecord,
    zone: i16,
    lens_level: i16,
    program_level: i32,
    threshold_reached: bool,
    thresholds: [i8; 2],
    level_words: Vec<(u8, i16, u32, u32)>,
    programs: Vec<ProgramInputs>,
    threshold_reads: u32,
}

impl Fake {
    fn new() -> Self {
        Self {
            snapshot: FrameSnapshot {
                status: 5,
                windows: [[1, 2], [3, 4]],
                size: [640, 480],
                extent: [7, 8],
                timing: 0x99,
                step: 12,
                zone: Zone(4),
                levels: [500, 300],
                adjustment: 0,
                level_offset: 40,
                lens: LensIdentity(3),
                frame_mode: 0,
                selected: 0,
                pinned_setting: 0,
                pinned: 0,
                pacing: [0; 2],
                mark: 0x0f8,
                correction_source: 0,
                correction_class: 0,
                corrects_position: 0,
            },
            record: IntakeRecord {
                scale: 0x0002_0001,
                selection: 1,
                mode: 2,
                profile_class: 4,
                resource: 5,
                range: [100, 900],
                range_shift: [10, 10],
                program_source: 2,
                lens_kind: 6,
                program_word: 77,
                stage: StageRecord { first_pass: 1, words: [33, 0], last: 9, ..StageRecord::default() },
                ..IntakeRecord::default()
            },
            zone: 20,
            lens_level: 5,
            program_level: 0,
            threshold_reached: true,
            thresholds: [50, 10],
            level_words: Vec::new(),
            programs: Vec::new(),
            threshold_reads: 0,
        }
    }
}

impl RecordReads for Fake {
    fn selection_ratio(&mut self) -> [u32; 2] { [11, 22] }
    fn converted_decade(&mut self) -> u16 { 0x1234 }
    fn zone_limit(&mut self, zone: Zone) -> i16 { if zone == Zone(4) { self.zone } else { 0 } }
    fn lens_level(&mut self, _: u32, _: LensIdentity) -> i16 { self.lens_level }
}

impl IntakePorts for Fake {
    fn frame_snapshot(&mut self) -> FrameSnapshot { self.snapshot }
    fn record(&mut self) -> IntakeRecord { self.record }
    fn imaging_profile_class(&mut self) -> u32 { 0x55 }
    fn parameter_index(&mut self, parameter: u32) -> u32 { parameter + 1000 }
    fn lens_identity(&mut self) -> LensIdentity { LensIdentity(8) }
    fn lens_step(&mut self, kind: u8, lens: LensIdentity) -> u32 { u32::from(kind) * 100 + lens.0 as u32 }
    fn program_level_word(&mut self, kind: u8, far: i16, program_word: u32, source: u32) -> u16 {
        self.level_words.push((kind, far, program_word, source));
        42
    }
    fn planner_word(&mut self) -> u32 { 0xabc }
    fn selected_program(&mut self, inputs: ProgramInputs) -> u32 {
        self.programs.push(inputs);
        3
    }
    fn program_level(&mut self, _: u32, _: i16, _: LensIdentity) -> i32 { self.program_level }
    fn peak_threshold_reached(&mut self) -> bool {
        self.threshold_reads += 1;
        self.threshold_reached
    }
    fn scan_thresholds(&mut self) -> [i8; 2] { self.thresholds }
}

fn view(phase: Phase) -> IntakeView {
    IntakeView { phase, accepted: false, samples: 0, scan: false, focus_value: 0 }
}

#[test]
fn the_record_carries_the_snapshot_record_and_level_the_intake_took_in() {
    let mut fake = Fake::new();
    let intake = take_in(view(Phase::Direction), &mut fake);
    let record = intake.record;
    assert!(record.first && record.settled);
    assert_eq!(record.scale, [1, 2]);
    assert_eq!((record.selection, record.mode, record.ratio), (1, 2, [11, 22]));
    assert_eq!(record.window, [1, 2]);
    assert_eq!((record.extent, record.size, record.timing, record.decade), ([7, 8], [640, 480], 0x99, 0x1234));
    assert_eq!(record.range, [100, 940]);
    assert_eq!(record.limit, 500 - (300 - 20) + 5);
    assert_eq!((record.profile_class, record.resource, record.correction), (4, 5, 0));
    assert_eq!(intake.frame.settled_near, Some(100));
    assert_eq!((intake.frame.lens, intake.frame.current_lens), (LensIdentity(3), LensIdentity(8)));
    assert_eq!((intake.frame.parameter_index, intake.frame.stage_first_pass, intake.frame.stage_last), (1033, 1, 9));
    assert_eq!(intake.frame.levels, [500, 300]);
    assert_eq!((intake.frame.mark, intake.frame.imaging_profile_class), (0x0f8, 0x55));
}

#[test]
fn an_accepted_search_an_unsettled_snapshot_and_frame_mode_one_change_the_record() {
    let mut fake = Fake::new();
    fake.snapshot.status = 4;
    fake.snapshot.frame_mode = 1;
    let intake = take_in(IntakeView { accepted: true, ..view(Phase::Direction) }, &mut fake);
    assert!(!intake.record.first && !intake.record.settled);
    assert_eq!(intake.record.window, [3, 4]);
    assert_eq!(intake.frame.settled_near, None);
}

#[test]
fn the_far_end_is_pulled_in_by_a_rising_shift_and_loses_100_under_an_adjustment() {
    let mut fake = Fake::new();
    fake.record.range_shift = [10, 70];
    fake.snapshot.adjustment = 1;
    let intake = take_in(view(Phase::Direction), &mut fake);
    assert_eq!(intake.record.range, [100, 900 + 10 - 70 + 40 - 100]);
    assert_eq!(intake.record.correction, 1);
}

#[test]
fn the_level_matches_when_the_frame_program_gives_the_intake_level() {
    let mut fake = Fake::new();
    fake.program_level = i32::from(500 - (300 - 20) + 5);
    assert!(take_in(view(Phase::Direction), &mut fake).record.level_matched);
    fake.program_level += 1;
    assert!(!take_in(view(Phase::Direction), &mut fake).record.level_matched);
}

#[test]
fn the_frame_program_takes_its_level_source_from_the_record_and_the_planner_only_in_mode_six() {
    let mut fake = Fake::new();
    for (source, expected) in [(1, 7), (2, 12), (0, 603)] {
        fake.record.program_source = source;
        fake.record.record_step = 7;
        take_in(view(Phase::Direction), &mut fake);
        assert_eq!(fake.level_words.pop(), Some((6, 900, 77, expected)));
    }
    assert_eq!(fake.programs.last().map(|inputs| (inputs.planner, inputs.level_word)), Some((0, 42)));
    fake.record.mode = 6;
    take_in(view(Phase::Direction), &mut fake);
    assert_eq!(fake.programs.last().map(|inputs| inputs.planner), Some(0xabc));
}

#[test]
fn a_peak_search_starts_reading_the_scan_pairs_at_the_threshold_and_mode_one_stops_it() {
    let mut fake = Fake::new();
    assert!(take_in(view(Phase::PeakSearch), &mut fake).record.scan);
    fake.threshold_reached = false;
    assert!(!take_in(view(Phase::PeakSearch), &mut fake).record.scan);
    let reads = fake.threshold_reads;
    assert!(take_in(IntakeView { scan: true, ..view(Phase::PeakSearch) }, &mut fake).frame.scan);
    assert_eq!(fake.threshold_reads, reads);
    assert!(take_in(IntakeView { scan: true, samples: 1, ..view(Phase::PeakSearch) }, &mut fake).record.scan);
    fake.snapshot.frame_mode = 1;
    assert!(!take_in(IntakeView { scan: true, samples: 1, ..view(Phase::PeakSearch) }, &mut fake).record.scan);
    assert!(take_in(IntakeView { scan: true, ..view(Phase::Direction) }, &mut fake).record.scan);
}

#[test]
fn the_scan_evaluation_follows_the_focus_value_against_the_thresholds() {
    let mut fake = Fake::new();
    let scan = |fake: &mut Fake, phase, scan, focus_value| {
        take_in(IntakeView { scan, focus_value, samples: 1, ..view(phase) }, fake).record.scan_evaluation
    };
    assert_eq!(scan(&mut fake, Phase::Direction, false, 1000), ScanEvaluation::Plain);
    assert_eq!(scan(&mut fake, Phase::PeakSearch, false, 100), ScanEvaluation::Plain);
    assert_eq!(scan(&mut fake, Phase::Return, false, 101), ScanEvaluation::Scanned);
    assert_eq!(scan(&mut fake, Phase::Approach, false, 500), ScanEvaluation::Scanned);
    assert_eq!(scan(&mut fake, Phase::PeakSearch, false, 501), ScanEvaluation::Scan);
    assert_eq!(scan(&mut fake, Phase::PeakSearch, true, 501), ScanEvaluation::Scanned);
}

#[test]
fn the_snapshot_layout_names_the_fields_the_intake_reads() {
    let mut bytes = [0u8; FRAME_SNAPSHOT_BYTES];
    let mut put = |offset: usize, value: &[u8]| bytes[offset..offset + value.len()].copy_from_slice(value);
    put(0x000, &5u32.to_le_bytes());
    put(0x034, &[1, 0, 0, 0, 2, 0, 0, 0]);
    put(0x0dc, &[3, 0, 0, 0, 4, 0, 0, 0]);
    put(0x0fe, &0x0123u16.to_le_bytes());
    put(0x108, &(-7i16).to_le_bytes());
    put(0x0f4, &6u32.to_le_bytes());
    put(0x110, &2u32.to_le_bytes());
    put(0x570, &1u32.to_le_bytes());
    put(0x584, &9u32.to_le_bytes());
    put(0x58a, &(-3i16).to_le_bytes());
    put(0x592, &[1]);
    let snapshot = frame_snapshot(&bytes);
    assert_eq!((snapshot.status, snapshot.windows[1], snapshot.extent), (5, [1, 2], [3, 4]));
    assert_eq!((snapshot.levels[1], snapshot.level_offset, snapshot.lens, snapshot.zone), (0x0123, -7, LensIdentity(2), Zone(6)));
    assert_eq!((snapshot.frame_mode, snapshot.pacing[1], snapshot.correction_source, snapshot.corrects_position), (1, 9, -3, 1));
}

#[test]
fn an_evaluation_completion_reads_the_result_at_the_address_it_names() {
    use crate::stock::evaluation::evaluated;
    use xm1_saclac_interface::search::ResultAddress;
    use xm1_saclac_interface::stock::search::EVALUATION_BYTES;
    let payload = [0, 0, 0, 0, 0x40, 0x0f, 0x6c, 0x01];
    let mut asked = Vec::new();
    let evaluation = evaluated(&payload, |address| {
        asked.push(address);
        let mut result = [0u8; EVALUATION_BYTES];
        result[0x60..0x64].copy_from_slice(&7u32.to_le_bytes());
        result
    });
    assert_eq!(asked, [ResultAddress(0x016c_0f40)]);
    assert_eq!(evaluation.map(|evaluation| evaluation.reference), Some(7));
    assert_eq!(evaluated(&payload[..4], |address| {
        asked.push(address);
        [0; EVALUATION_BYTES]
    }), None);
    assert_eq!(asked.len(), 1);
}

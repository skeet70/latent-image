use super::drive::{DriveKind, DriveLimits, DriveMode};
use super::plan::{AreaLayout, Motion, PlanKind};
use super::plan_ports::{PlanServices, PlanSpeedRequest, PositionRemap};
use super::plan_result::FocusPublication;
use super::ports::{AreaGrid, SearchBounds};
use super::preparation::PreviousFocus;
use super::search::{AreaCell, SearchMode};
use super::state::{MotionState, SampleSchedule};
use super::world::{MotionWorld, blank_state};
use crate::adjust::lens::{LensControl, LensSteps, Step, StepRequest};
use crate::exposure::ports::Property;
use crate::scene_recognition::ports::Overlay;
use std::vec::Vec;
use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_profile_records::stock::shared::group_17;

pub struct Planner {
    pub rates: [u32; 3],
    pub by_depth: bool,
    pub side: u32,
    pub checks_speed: bool,
    pub registers: [u8; 256],
    pub cached: Option<[u32; 4]>,
    pub text: Vec<(Vec<u8>, u8)>,
    pub released: usize,
}
impl Default for Planner {
    fn default() -> Self {
        Self {
            rates: [100, 200, 300],
            by_depth: false,
            side: 0,
            checks_speed: false,
            registers: [0; 256],
            cached: None,
            text: Vec::new(),
            released: 0,
        }
    }
}
impl PlanServices for MotionWorld {
    fn plan_speed(&mut self, request: PlanSpeedRequest) -> Step {
        let rate = if self.planner.by_depth {
            u32::from(request.depth) * 100
        } else {
            self.planner.rates[usize::from(request.selector)]
        };
        Step {
            size: (u64::from(rate) * u64::from(request.period) / 10_000) as u16,
            rate,
        }
    }
    fn remap_position(&mut self, request: PositionRemap) -> i16 {
        request.position
    }
    fn minimum_speed(&mut self) -> u32 {
        1
    }
    fn current_side(&mut self) -> u32 {
        self.planner.side
    }
    fn checks_drive_speed(&mut self) -> bool {
        self.planner.checks_speed
    }
    fn load_register(&mut self, register: u8) -> u8 {
        self.planner.registers[usize::from(register)]
    }
    fn snapshot_mode(&mut self) -> u32 {
        0
    }
    fn cached_focus(&mut self) -> Option<[u32; 4]> {
        self.planner.cached
    }
    fn release_aux_text(&mut self) {
        self.planner.released += 1;
    }
}
impl LensControl for MotionWorld {
    fn position(&mut self) -> u16 {
        self.world.lens_configuration.position
    }
    fn travel_margin(&mut self) -> u16 {
        self.world.lens_configuration.margin
    }
}
impl LensSteps for MotionWorld {
    fn step(&mut self, _request: StepRequest) -> Step {
        Step { size: 1, rate: 1 }
    }
    fn travel_distance(&mut self, _resource: u32, _index: u32, travel: i16) -> u16 {
        travel as u16
    }
}
impl DriveLimits for MotionWorld {
    fn first_ramp_speed(&mut self, _kind: DriveKind, _mode: DriveMode) -> u32 {
        0
    }
    fn first_ramp_acceleration(&mut self, _kind: DriveKind, _mode: DriveMode) -> u32 {
        0
    }
    fn second_ramp_acceleration(&mut self, _kind: DriveKind, _mode: DriveMode) -> u32 {
        0
    }
    fn slowest_ramped_speed(&mut self) -> u32 {
        0
    }
}
impl Overlay for MotionWorld {
    fn text(&mut self, text: &[u8], _column: u8, row: u8, _foreground: u8, _background: u8) {
        self.planner.text.push((text.to_vec(), row));
    }
}
fn byte(w: &mut MotionWorld, id: u32, value: u32) {
    w.property(Property(id), value);
}
fn word(w: &mut MotionWorld, id: u32, value: u16) {
    w.property_word(Property(id), value);
}
fn fixture() -> (MotionState, MotionWorld) {
    // Synthetic lens with unit depth conversion and constant-speed ramps; not a camera capture.
    let mut s = blank_state(SharedAddress(0x100000), SharedAddress(0x200000), 10_000);
    s.bounds = SearchBounds { low: 0, high: 1000 };
    s.readings.class = 2;
    s.exposures.second.change_scale = 10;
    s.exposures.second.gain_scale = 10;
    s.exposures.second.frames = 4;
    s.layout.grids[0] = AreaGrid {
        cell_height: 10,
        cell_width: 20,
        columns: 3,
        rows: 3,
        ..AreaGrid::default()
    };
    s.search.frames[0] = AreaGrid {
        cell_height: 20,
        cell_width: 40,
        ..AreaGrid::default()
    };
    s.sampling = SampleSchedule::new(10_000, &[1000], &[], 0, 0);
    let mut w = MotionWorld::default();
    w.world.lens_configuration.limits = [0, 1000];
    w.world.lens_configuration.position = 50;
    w.bounds.insert(
        0,
        SearchBounds {
            low: 100,
            high: 300,
        },
    );
    byte(&mut w, 0x928a, 10);
    byte(&mut w, 0x949a, 15);
    byte(&mut w, 0x949b, 30);
    byte(&mut w, 0x9602, 1);
    byte(&mut w, 0x9607, 1);
    byte(&mut w, 0x9608, 2);
    byte(&mut w, 0x960a, 4);
    (s, w)
}

#[test]
fn planned_grid_dimensions_govern_targets_and_measurement_after_sample_layout() {
    for (initial_side, alternate, expected_layout, count) in [(1, 1, AreaLayout::Grid, 9), (3, 0, AreaLayout::GridAlternate, 1)] {
        let (mut s, mut w) = fixture();
        s.readings.class = 1;
        s.readings.channel = 1;
        s.search.mode = SearchMode::Channel;
        let primary = AreaGrid { cell_height: 10, cell_width: 10, columns: 7, rows: 7, ..AreaGrid::default() };
        w.sample_frames = [primary, AreaGrid { columns: initial_side, rows: initial_side, ..primary }];
        s.search.frames = [AreaGrid { columns: 3, rows: 3, ..primary }; 2];
        w.bounds.insert(0, SearchBounds { low: 100, high: 800 });
        w.bounds.insert(1, SearchBounds { low: 100, high: 900 });
        byte(&mut w, 0x94d2, alternate);
        byte(&mut w, 0x3e1213, 64);
        byte(&mut w, 0x94dd, 100);
        byte(&mut w, 0x9618, 100);
        s.lay_out_samples(&mut w);
        assert_eq!(s.schedule().secondary().len(), usize::from(initial_side));
        assert_ne!(s.plan_next(&mut w), PlanKind::SETTLED);
        assert_eq!(s.plan().layout, expected_layout);
        let targets = s.targets(&mut w, 500, 1);
        let settings = s.measurement_settings(&mut w);
        assert_eq!(settings.secondary_areas, count);
        let centre = targets.primary[s.schedule().primary().len() / 2];
        assert_ne!(centre, 0);
        assert_eq!(targets.secondary, core::array::from_fn(|index| if index < usize::from(count) { centre } else { 0 }));
    }
}

#[test]
fn a_new_pass_publishes_drive_sampling_and_depth_scaled_contrast() {
    let (mut s, mut w) = fixture();
    byte(&mut w, 0xde, 1);
    for id in [0x92e0, 0x92e6, 0x92ec, 0x92e2, 0x92e8, 0x92ee] {
        word(&mut w, id, 400);
    }
    for id in [0x92de, 0x92e4, 0x92ea] {
        word(&mut w, id, 200);
    }
    assert_eq!(s.plan_next(&mut w), PlanKind(1));
    let pass = s.plan();
    assert_eq!(
        (
            pass.profile.start,
            pass.profile.end,
            pass.step,
            pass.last_frame
        ),
        (0, 1000, 8, 250)
    );
    assert_eq!(
        (pass.motion, pass.speed, pass.frame_travel, pass.frame_depth),
        (Motion::Continuous, 200, 20, 20)
    );
    assert_eq!(pass.weights, [1, 2, 4]);
    assert_eq!((pass.primary_share, pass.secondary_share), (2500, 2500));
    assert_eq!(s.thresholds().rows[0], [20, 40, 60]);
    assert_eq!(s.thresholds().rows[1], [10, 20, 30]);
    assert_eq!(s.thresholds().words()[12..18], [20, 40, 60, 20, 40, 60]);
    assert_eq!(pass.selected_speed(), Some(200));
    assert!(!pass.tone_window().channel);
    assert!(
        pass.tone_window().enabled
            && pass.tone_window().untreated
            && pass.tone_window().single_cell
    );
}

#[test]
fn load_test_corrects_step_direction_and_uses_previous_depth_settings() {
    let (mut s, mut w) = fixture();
    s.readings.load_test = true;
    s.plan.frame_depth = 151;
    byte(&mut w, 0x926e, 2);
    for (register, value) in [(0x82, 10i16), (0x84, 110), (0x86, -7)] {
        w.planner.registers[register..register + 2].copy_from_slice(&value.to_be_bytes());
    }
    for (id, value) in [(0x943b, 11), (0x9440, 12), (0x9442, 13)] {
        word(&mut w, id, value);
    }
    for (id, value) in [(0x943d, 21), (0x943e, 22), (0x943f, 23)] {
        byte(&mut w, id, value);
    }
    assert_eq!(s.plan_next(&mut w), PlanKind(1));
    assert_eq!(
        (
            s.plan().profile.start,
            s.plan().profile.end,
            s.plan().step,
            s.plan().last_frame
        ),
        (10, 110, 7, 15)
    );
    assert_eq!(s.plan().motion, Motion::Stepped);
    assert_eq!(s.depth_settings().halves, [11, 12, 13]);
    assert_eq!(s.depth_settings().bytes, [21, 22, 23]);
}

#[test]
fn a_failed_side_pass_reverses_and_covers_the_exposure_midpoint() {
    let (mut s, mut w) = fixture();
    s.plan.kind = PlanKind(9);
    s.plan.result.kind = 2;
    assert_eq!(s.plan_next(&mut w), PlanKind(27));
    assert_eq!(
        (s.plan().profile.start, s.plan().profile.end, s.plan().step),
        (303, 98, -2)
    );
    assert!(s.plan().profile.reverse);
}

#[test]
fn scan_override_retains_the_decided_kind_and_selects_the_full_span() {
    let (mut s, mut w) = fixture();
    s.plan.kind = PlanKind(5);
    s.plan.decided = PlanKind(15);
    s.plan.result.kind = 2;
    s.search.mode = SearchMode::Area;
    byte(&mut w, 0x9290, 1);
    byte(&mut w, 0x9291, 2);
    byte(&mut w, 0x9293, 1);
    word(&mut w, 0x9294, 1);
    assert_eq!(s.plan_next(&mut w), PlanKind(18));
    assert_eq!(s.plan().decided, PlanKind(15));
    assert_eq!((s.plan().profile.start, s.plan().profile.end), (1000, 0));
    assert!(!s.plan().channel_window);
}

#[test]
fn an_admitted_confirmation_starts_a_target_window_instead_of_a_full_scan() {
    let (mut s, mut w) = fixture();
    s.previous = PreviousFocus::Admitted;
    s.readings.class = 0;
    s.readings.confirmation = [1, 0, 900, 0, 0, 0, 0, 0];
    byte(&mut w, 0x96cf, 2);
    byte(&mut w, 0x92b3, 1);
    byte(&mut w, 0x92c8, 1);
    byte(&mut w, 0x96d8, 50);
    byte(&mut w, 0x92b8, 30);
    assert_eq!(s.plan_next(&mut w), PlanKind(11));
    assert_eq!((s.plan().target.source, s.plan().target.position), (2, 900));
    assert_eq!((s.plan().profile.start, s.plan().profile.end), (887, 1000));
    assert_eq!(s.plan().layout, AreaLayout::GridAlternate);
}

#[test]
fn settled_keeps_the_installed_drive_and_reports_result_and_pending_state() {
    let (mut s, mut w) = fixture();
    s.plan.kind = PlanKind(10);
    s.plan.profile.start = 123;
    s.plan.result.kind = 1;
    s.plan.result.class = 9;
    s.search.area = AreaCell { column: 5, row: 2 };
    assert_eq!(s.plan_next(&mut w), PlanKind::SETTLED);
    assert_eq!(
        (s.plan().kind, s.plan().profile.start, s.plan().result.area),
        (PlanKind(10), 123, 19)
    );
    assert!(s.plan().flags.found);
    assert!(!s.plan().flags.searching);
    s.plan.result.pending = 1;
    assert_eq!(s.plan_next(&mut w), PlanKind::SETTLED);
    assert!(!s.plan().flags.found);
    assert!(s.plan().flags.searching);
}

#[test]
fn scan_results_and_slow_retry_requests_choose_distinct_followup_passes() {
    for (kind, result, missing, expected) in [
        (17, 0, 0, 19),
        (17, 1, 0, 21),
        (18, 0, 0, 20),
        (18, 1, 0, 22),
        (17, 2, 0, 3),
        (18, 2, 1, 22),
    ] {
        let (mut s, mut w) = fixture();
        s.plan.kind = PlanKind(kind);
        s.plan.result.kind = result;
        byte(&mut w, 0x9292, missing);
        assert_eq!(s.plan_next(&mut w), PlanKind(expected));
    }
    for (pending, fast, area_status, retry_missing, expected) in [
        (0, 1, 0, false, 24),
        (0, 0, 1, false, 25),
        (0, 0, 0, true, 26),
    ] {
        let (mut s, mut w) = fixture();
        s.plan.kind = PlanKind(10);
        s.plan.result.kind = if retry_missing { 2 } else { 0 };
        s.plan.result.pending = pending;
        s.plan.result.fast = fast;
        s.plan.result.area_status = area_status;
        s.plan.checks_speed = true;
        s.search.retry_missing = retry_missing;
        byte(&mut w, 0x96cb, 1);
        assert_eq!(s.plan_next(&mut w), PlanKind(expected));
        if expected == 25 {
            assert_eq!(s.plan().speed, 100);
            assert_eq!(s.plan().selection, 0);
        }
    }
}

#[test]
fn depth_period_interpolation_changes_the_driven_speed_at_its_boundaries() {
    for (period, expected) in [(10_000, 1000), (15_000, 2000), (20_000, 3000)] {
        let (mut s, mut w) = fixture();
        s.exposures.second.time = period;
        w.planner.by_depth = true;
        byte(&mut w, 0x3e144d, 1);
        byte(&mut w, 0x949f, 1);
        byte(&mut w, 0x94a7, 3);
        byte(&mut w, 0x94ac, 10);
        byte(&mut w, 0x94ad, 20);
        assert_eq!(s.plan_next(&mut w), PlanKind(1));
        assert_eq!(s.plan().speed, expected);
    }
}

#[test]
fn designation_and_cached_confirmation_obey_publication_precedence() {
    let (mut s, mut w) = fixture();
    s.readings.designation = [1, 2, 3, 0xfffe, 7, 8, 9];
    s.readings.confirmation[2] = 44;
    w.planner.cached = Some([12, 13, 14, 1]);
    let mut out = FocusPublication::default();
    s.publish_existing(&mut w, &mut out);
    assert_eq!(
        (s.plan().result.position, s.plan().result.area),
        (0xfffe, 23)
    );
    assert_eq!(out.snapshot, [0xfffe, 7, 8, 9]);
    assert!(s.plan().flags.found);
    byte(&mut w, 0x3e0009, 1);
    s.publish_existing(&mut w, &mut out);
    assert_eq!(
        (out.status, out.snapshot[2], s.plan().result.position),
        (2, 0x80, 0)
    );
    assert!(s.plan().flags.searching);
    byte(&mut w, 0x3e0009, 0);
    s.readings.designation[0] = 0;
    byte(&mut w, 0x9278, 1);
    s.publish_existing(&mut w, &mut out);
    assert_eq!(
        (
            s.plan().result.position,
            out.snapshot,
            out.pending,
            s.plan().flags.side
        ),
        (44, [12, 13, 14, 1], 1, 1)
    );
}

#[test]
fn fallback_publishes_the_snapshot_but_marks_its_state_unavailable() {
    let (mut s, mut w) = fixture();
    let mut bytes = [0u8; 48];
    for (chunk, value) in bytes[..16].chunks_mut(4).zip([0x12345678u32, 9, 10, 11]) {
        chunk.copy_from_slice(&value.to_le_bytes());
    }
    w.world.set_shared(&group_17::LENS_SNAPSHOT, bytes);
    s.search.area = AreaCell {
        column: 0xff,
        row: 1,
    };
    let mut out = FocusPublication::default();
    s.publish_fallback(&mut w, &mut out);
    assert_eq!(out.snapshot, [0x12345678, 9, 0x80, 11]);
    assert_eq!((out.enabled, out.status, out.pending), (1, 2, 0));
    assert_eq!(s.plan().result.area, 6);
    assert_eq!(s.plan().result.diagnostic_status, [4, 3]);
    assert!(s.plan().flags.searching);
}

#[test]
fn pipeline_results_keep_reached_tone_limits_and_mark_the_fast_speed_boundary() {
    use super::plan::{FocusResult, Plan, ToneResult};
    let mut pass = Plan {
        checks_speed: true,
        speeds: [100, 200, 300],
        ..Plan::default()
    };
    pass.reset_results();
    let mut first = ToneResult::default();
    first.words[8] = 1;
    first.words[9] = 2;
    first.words[11] = 1;
    pass.record_tone(first);
    let mut second = ToneResult::default();
    second.words[8] = 0;
    second.words[9] = 1;
    second.words[11] = 0;
    second.words[12] = 2;
    pass.record_tone(second);
    assert_eq!(pass.tone_limits, [1, 1]);
    assert_eq!((pass.tone.words[11], pass.tone.words[12]), (1, 2));
    pass.record_focus(
        FocusResult {
            position: 120,
            ..FocusResult::default()
        },
        299,
    );
    assert_eq!((pass.result.position, pass.result.fast), (120, 0));
    pass.record_focus(
        FocusResult {
            position: 130,
            ..FocusResult::default()
        },
        300,
    );
    assert_eq!((pass.result.position, pass.result.fast), (130, 1));
}

#[test]
fn dynamic_weights_use_the_retained_zero_baseline_and_round_the_level_adjustment() {
    let (mut s, mut w) = fixture();
    byte(&mut w, 0x9602, 0);
    s.exposures.second.step = 1;
    s.exposures.second.gain = 3;
    s.exposures.second.first_level = 200;
    s.program[1].first_base = 999;
    for (id, value) in [
        (0x9045, 10),
        (0x9046, 20),
        (0x9048, 30),
        (0x9126, 50),
        (0x9127, 100),
        (0x9129, 150),
    ] {
        byte(&mut w, id, value);
    }
    s.plan_next(&mut w);
    assert_eq!(s.plan().weights, [20, 40, 60]);
}

#[test]
fn settled_area_debug_text_refuses_a_three_digit_index() {
    let (mut s, mut w) = fixture();
    s.readings.scene = 50;
    s.plan.kind = PlanKind(10);
    s.plan.result.kind = 0;
    s.plan.result.area_count = 2;
    s.plan.result.areas[0].index = 7;
    s.plan.result.areas[1].index = 103;
    byte(&mut w, 0x4907, 1);
    assert_eq!(s.plan_next(&mut w), PlanKind::SETTLED);
    assert_eq!(
        w.planner.text,
        [
            (b"07".to_vec(), 7),
            (b"--".to_vec(), 8),
            (b"--".to_vec(), 9)
        ]
    );
    assert!(s.plan().flags.found);
}

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::stock::shared::{group_27, group_33};
use xm1_saclac_profile_records::MeasurementSelection;

use super::detectors::{enabled, filter, on_tripod, with_landscape, with_person, with_shake_reduction, CaptureChange, CaptureDetector, ChangeDetector, SceneDetector};
use super::fakes::Scenery;
use super::history::Code;
use super::{Features, FilterOutcome, LevelBounds, Mode, Recognition, Scene, SceneFlags, SceneRecognition};
use crate::exposure::fakes::handle;
use crate::exposure::ports::Property;

const SOURCE: SharedAddress = SharedAddress(0x0180_0000);

fn property(scenery: &mut Scenery, id: u32, value: u32) { scenery.world.property(Property(id), value); }

fn ask(scenery: &mut Scenery, detector: SceneDetector, features: &Features, mode: Mode) -> Scene { detector.ask(scenery, handle(), features, mode) }

fn portrait_features() -> Features { Features { at_005d: 1, subjects: 2, ..Features::ZERO } }

fn frame_bytes(set: impl FnOnce(&mut [u8; 76])) -> [u8; 76] {
    let mut bytes = [0; 76];
    set(&mut bytes);
    bytes
}

fn put_source(scenery: &mut Scenery, cell: usize, level: i16) { scenery.world.put_u16(SharedAddress(SOURCE.0 + 2 * cell as u32), level as u16); }

#[test]
fn portraits_need_compared_subjects() {
    let mut scenery = Scenery::default();
    assert_eq!(ask(&mut scenery, SceneDetector::Portrait, &Features { subjects: 0, ..portrait_features() }, Mode::Live), Scene::AUTO);
    assert_eq!(ask(&mut scenery, SceneDetector::Portrait, &Features { at_005d: 0, ..portrait_features() }, Mode::Live), Scene::AUTO);
    assert_eq!(ask(&mut scenery, SceneDetector::Portrait, &portrait_features(), Mode::Live), Scene::PORTRAIT);
}

#[test]
fn a_dark_portrait_is_a_night_portrait_and_a_spread_one_backlit() {
    let mut scenery = Scenery::default();
    property(&mut scenery, 0x75fb, 1);
    property(&mut scenery, 0x75fd, 5);
    property(&mut scenery, 0x75fa, 1);
    property(&mut scenery, 0x75fc, 3);
    let dark_and_spread = Features { level: 49, upper_level: 100, lower_level: 69, ..portrait_features() };
    assert_eq!(ask(&mut scenery, SceneDetector::Portrait, &dark_and_spread, Mode::Live), Scene::BACKLIT_PORTRAIT);
    property(&mut scenery, 0x75f9, 1);
    assert_eq!(ask(&mut scenery, SceneDetector::Portrait, &dark_and_spread, Mode::Live), Scene::NIGHT_PORTRAIT);
    let even = Features { level: 50, upper_level: 100, lower_level: 70, ..portrait_features() };
    assert_eq!(ask(&mut scenery, SceneDetector::Portrait, &even, Mode::Live), Scene::PORTRAIT);
}

#[test]
fn the_shot_answers_the_spread_test_itself_and_record_selection_switches_limits() {
    let mut scenery = Scenery::default();
    property(&mut scenery, 0x75fa, 1);
    let answered = Features { difference_answer: 1, ..portrait_features() };
    assert_eq!(ask(&mut scenery, SceneDetector::Portrait, &answered, Mode::Capture), Scene::BACKLIT_PORTRAIT);
    assert_eq!(ask(&mut scenery, SceneDetector::Portrait, &Features { difference_answer: 2, ..answered }, Mode::Capture), Scene::PORTRAIT);
    property(&mut scenery, 0x75fb, 1);
    property(&mut scenery, 0x7600, 2);
    scenery.world.select(MeasurementSelection::Record);
    assert_eq!(ask(&mut scenery, SceneDetector::Portrait, &Features { level: 19, ..portrait_features() }, Mode::Capture), Scene::NIGHT_PORTRAIT);
}

#[test]
fn a_landscape_needs_a_near_lens_and_its_level() {
    let mut scenery = Scenery::default();
    property(&mut scenery, 0x75f0, (-2i8) as u8 as u32);
    let features = Features { level: -20, position: 5, response: 5, ..Features::ZERO };
    assert_eq!(ask(&mut scenery, SceneDetector::Landscape, &features, Mode::Live), Scene::LANDSCAPE);
    assert_eq!(ask(&mut scenery, SceneDetector::Landscape, &Features { level: -21, ..features }, Mode::Live), Scene::AUTO);
    assert_eq!(ask(&mut scenery, SceneDetector::Landscape, &Features { position: 6, ..features }, Mode::Live), Scene::AUTO);
    assert_eq!(ask(&mut scenery, SceneDetector::Landscape, &Features { status: 2, ..features }, Mode::Capture), Scene::AUTO);
}

#[test]
fn the_live_gate_reads_the_lens_state_by_the_count() {
    let mut scenery = Scenery::default();
    scenery.world.property_word(Property(0x75e9), 10);
    property(&mut scenery, 0x75ec, 1);
    property(&mut scenery, 0x75eb, 2);
    let features = Features { at_4c: 9, at_3c: 2, ..Features::ZERO };
    assert_eq!(ask(&mut scenery, SceneDetector::Landscape, &features, Mode::Live), Scene::AUTO);
    assert_eq!(ask(&mut scenery, SceneDetector::Landscape, &Features { at_4c: 10, ..features }, Mode::Live), Scene::LANDSCAPE);
    assert_eq!(ask(&mut scenery, SceneDetector::Landscape, &Features { at_4c: 10, at_3c: 1, ..features }, Mode::Live), Scene::AUTO);
}

#[test]
fn night_scenes_by_mode() {
    let mut scenery = Scenery::default();
    property(&mut scenery, 0x75f2, 3);
    let features = Features { level: 30, ..Features::ZERO };
    assert_eq!(ask(&mut scenery, SceneDetector::Night, &features, Mode::Live), Scene::AUTO);
    property(&mut scenery, 0x75f4, 1);
    assert_eq!(ask(&mut scenery, SceneDetector::Night, &features, Mode::Live), Scene::HANDHELD_NIGHT);
    assert_eq!(ask(&mut scenery, SceneDetector::Night, &Features { level: 31, ..features }, Mode::Live), Scene::AUTO);
    assert_eq!(ask(&mut scenery, SceneDetector::Night, &features, Mode::Capture), Scene::HANDHELD_NIGHT);
    property(&mut scenery, 0x75f3, 1);
    assert_eq!(ask(&mut scenery, SceneDetector::Night, &features, Mode::Capture), Scene::AUTO);
    property(&mut scenery, 0x75f5, 1);
    assert_eq!(ask(&mut scenery, SceneDetector::Night, &Features { selected: 1, status: 1, ..features }, Mode::Capture), Scene::HANDHELD_NIGHT);
    assert_eq!(ask(&mut scenery, SceneDetector::Night, &Features { selected: 1, status: 2, ..features }, Mode::Capture), Scene::AUTO);
}

#[test]
fn a_closeup_needs_the_lens_past_its_distance() {
    let mut scenery = Scenery::default();
    let features = Features { position: 40, distance: 40, ..Features::ZERO };
    assert_eq!(ask(&mut scenery, SceneDetector::Closeup, &features, Mode::Capture), Scene::CLOSEUP);
    assert_eq!(ask(&mut scenery, SceneDetector::Closeup, &Features { position: 39, ..features }, Mode::Capture), Scene::AUTO);
    assert_eq!(ask(&mut scenery, SceneDetector::Silent, &features, Mode::Capture), Scene::AUTO);
}

#[test]
fn detector_indexes_cap_at_the_last() {
    assert_eq!(SceneDetector::from_index(0), None);
    assert_eq!(SceneDetector::from_index(9), Some(SceneDetector::Silent));
    assert_eq!(SceneDetector::naming(Scene(26)), Some(SceneDetector::Portrait));
    assert_eq!(SceneDetector::naming(Scene(37)), Some(SceneDetector::Landscape));
    assert_eq!(SceneDetector::naming(Scene::NIGHT), None);
    assert_eq!(ChangeDetector::from_slot(7), Some(ChangeDetector::Stationary));
    assert_eq!(CaptureDetector::from_slot(3), Some(CaptureDetector::Silent));
}

#[test]
fn promotions_and_switches() {
    let mut scenery = Scenery::default();
    property(&mut scenery, 0x75f0, 1);
    let bright = Features { level: 10, stationary: true, ..Features::ZERO };
    assert_eq!(with_landscape(&mut scenery, handle(), &bright, Scene::BACKLIT_PORTRAIT), Scene::LANDSCAPE_BACKLIT_PORTRAIT);
    assert_eq!(with_landscape(&mut scenery, handle(), &Features { level: 9, ..bright }, Scene::PORTRAIT), Scene::PORTRAIT);
    assert_eq!(on_tripod(&mut scenery, &bright, Scene::HANDHELD_NIGHT), Scene::HANDHELD_NIGHT);
    property(&mut scenery, 0x76dd, 1);
    assert_eq!(on_tripod(&mut scenery, &bright, Scene::NIGHT_PORTRAIT), Scene::NIGHT_PORTRAIT_TRIPOD);
    assert_eq!(on_tripod(&mut scenery, &Features { stationary: false, ..bright }, Scene::HANDHELD_NIGHT), Scene::HANDHELD_NIGHT);
    property(&mut scenery, 0x768d, 1);
    assert!(enabled(&mut scenery, Scene::CLOSEUP));
    assert!(!enabled(&mut scenery, Scene::NIGHT));
}

#[test]
fn a_person_moves_a_group_scene_to_its_variant() {
    assert_eq!(with_person(Scene::PORTRAIT, Scene(18)), Scene(19));
    assert_eq!(with_person(Scene::LANDSCAPE_PORTRAIT, Scene(27)), Scene(27));
    assert_eq!(with_person(Scene::BACKLIT_PORTRAIT, Scene(34)), Scene(36));
    assert_eq!(with_person(Scene::LANDSCAPE_BACKLIT_PORTRAIT, Scene(38)), Scene(38));
    assert_eq!(with_person(Scene::NIGHT_PORTRAIT, Scene(21)), Scene(21));
    assert_eq!(with_person(Scene::PORTRAIT, Scene::LANDSCAPE), Scene::LANDSCAPE);
}

#[test]
fn the_rate_filter_admits_a_group_that_fits() {
    let mut scenery = Scenery::default();
    let features = Features { level: 40, position: 3, response: 3, ..Features::ZERO };
    assert_eq!(filter(&mut scenery, &features, Scene(25), Scene(5)), (Scene(5), FilterOutcome::default()));
    property(&mut scenery, 0x7698, 1);
    assert_eq!(filter(&mut scenery, &features, Scene(25), Scene(5)), (Scene(25), FilterOutcome::default()));
    property(&mut scenery, 0x769e, 1);
    property(&mut scenery, 0x76a4, 1);
    property(&mut scenery, 0x76aa, 4);
    property(&mut scenery, 0x76ab, 3);
    assert_eq!(filter(&mut scenery, &features, Scene(25), Scene(5)), (Scene(5), FilterOutcome { position: true, level: false }));
    property(&mut scenery, 0x76ab, 4);
    assert_eq!(filter(&mut scenery, &features, Scene(25), Scene(5)), (Scene(25), FilterOutcome { position: true, level: true }));
    assert_eq!(filter(&mut scenery, &Features { position: 4, ..features }, Scene(25), Scene(5)), (Scene(5), FilterOutcome::default()));
    assert_eq!(filter(&mut scenery, &features, Scene::NO_HINT, Scene(5)), (Scene(5), FilterOutcome::default()));
}

#[test]
fn the_reference_grid_detector_weighs_cells_and_keeps_its_measure() {
    let mut scenery = Scenery::default();
    let mut state = SceneRecognition::new();
    assert!(!state.changed_from_reference(&mut scenery, handle(), ChangeDetector::Grid));
    state.set_source(&mut scenery, SOURCE);
    put_source(&mut scenery, 0, 100);
    put_source(&mut scenery, 119, 80);
    property(&mut scenery, 0x75c4, 1);
    property(&mut scenery, 0x75c3, 2);
    assert!(state.changed_from_reference(&mut scenery, handle(), ChangeDetector::Grid));
    assert_eq!(state.evaluation().against_reference.mean, 20);
    assert_eq!(state.evaluation().against_reference.compared[0], 100);
    property(&mut scenery, 0x75c3, 1);
    assert!(!state.changed_from_reference(&mut scenery, handle(), ChangeDetector::Grid));
    state.settled = Recognition { scene: Scene(20), flags: SceneFlags::NONE };
    property(&mut scenery, 0x7612, 0);
    assert!(state.changed_from_reference(&mut scenery, handle(), ChangeDetector::Grid));
}

#[test]
fn position_detectors_measure_the_reported_position() {
    let mut scenery = Scenery::default();
    let mut state = SceneRecognition::new();
    state.report_position(12);
    state.evaluation.against_reference.reference.note.position = 9;
    state.evaluation.snapshot.position = 2;
    assert!(state.changed_from_reference(&mut scenery, handle(), ChangeDetector::Position));
    assert_eq!(state.evaluation().against_reference.position_change, 3);
    scenery.world.set_shared(&group_33::AT_07A0, frame_bytes(|bytes| bytes[0x38] = 1));
    scenery.world.property_word(Property(0x7629), 10);
    assert!(!state.changed_from_snapshot(&mut scenery, handle(), ChangeDetector::Position));
    assert_eq!(state.evaluation().against_snapshot.position_change, 10);
    scenery.world.set_shared(&group_33::AT_07A0, frame_bytes(|bytes| bytes[0x44] = 2));
    state.report_position(40);
    assert!(!state.changed_from_reference(&mut scenery, handle(), ChangeDetector::Position));
    assert_eq!(state.evaluation().against_reference.position_change, 3);
}

#[test]
fn the_subject_detector_keeps_the_last_count_when_the_query_refuses() {
    let mut scenery = Scenery::default();
    let mut state = SceneRecognition::new();
    scenery.world.set(&group_00::AT_005D, 1);
    scenery.subjects = Some(2);
    assert!(state.changed_from_reference(&mut scenery, handle(), ChangeDetector::Subject));
    scenery.subjects = None;
    state.evaluation.snapshot.subject = true;
    assert!(!state.changed_from_snapshot(&mut scenery, handle(), ChangeDetector::Subject));
    assert!(state.evaluation().against_reference.subject_present);
    assert_eq!(scenery.requests.len(), 2);
}

#[test]
fn motion_classes_two_and_three_count_as_the_same() {
    let mut scenery = Scenery::default();
    let mut state = SceneRecognition::new();
    let motion = |class: u32| {
        let mut bytes = [0u8; 40];
        bytes[0x10..0x14].copy_from_slice(&class.to_le_bytes());
        bytes
    };
    state.evaluation.against_reference.reference.note.motion = 2;
    scenery.world.set_shared(&group_27::AT_07EC, motion(3));
    assert!(!state.changed_from_reference(&mut scenery, handle(), ChangeDetector::Motion));
    assert_eq!(state.evaluation().against_reference.motion, 3);
    scenery.world.set_shared(&group_27::AT_07EC, motion(1));
    assert!(state.changed_from_reference(&mut scenery, handle(), ChangeDetector::Motion));
    scenery.world.select(MeasurementSelection::Record);
    assert!(!state.changed_from_reference(&mut scenery, handle(), ChangeDetector::Motion));
}

#[test]
fn the_stationary_decision_counts_unsteady_frames_and_draws_its_line() {
    let mut scenery = Scenery::default();
    let mut state = SceneRecognition::new();
    property(&mut scenery, 0x76de, 2);
    property(&mut scenery, 0xeb, 1);
    scenery.steady = true;
    assert!(state.decide_stationary(&mut scenery, true));
    scenery.steady = false;
    assert!(state.decide_stationary(&mut scenery, true));
    assert!(state.decide_stationary(&mut scenery, false));
    assert!(!state.decide_stationary(&mut scenery, true));
    assert_eq!(scenery.lines[0], (b"GETSTATIC:1".to_vec(), [2, 4, 2, 9]));
    assert_eq!(scenery.lines[1].0, b"GETSTATIC:".to_vec());
}

#[test]
fn the_snapshot_stationary_detector_records_its_decision() {
    let mut scenery = Scenery::default();
    let mut state = SceneRecognition::new();
    scenery.steady = true;
    assert!(state.changed_from_snapshot(&mut scenery, handle(), ChangeDetector::Stationary));
    assert!(state.stationary_history()[0].stationary && state.stationary_history()[0].steady);
    assert!(state.evaluation().features.index_recorded);
}

#[test]
fn the_shot_compares_its_grid_and_lens_with_the_reference() {
    let mut scenery = Scenery::default();
    let mut state = SceneRecognition::new();
    property(&mut scenery, 0x75c3, 1);
    property(&mut scenery, 0x75c4, 1);
    state.shift_grid(&mut scenery, &[20; 256], 0);
    assert_eq!(state.capture_change(&mut scenery, handle(), CaptureDetector::GridMatch), CaptureChange::Toward(true));
    state.evaluation.against_reference.reference.note.grid = [40; 256];
    assert_eq!(state.capture_change(&mut scenery, handle(), CaptureDetector::GridMatch), CaptureChange::Toward(false));
    state.evaluation.against_reference.reference.note.grid = [15; 256];
    assert_eq!(state.capture_change(&mut scenery, handle(), CaptureDetector::GridMatch), CaptureChange::None);
    state.evaluation.against_reference.reference.note.position = 7;
    scenery.world.set_shared(&group_33::AT_07A0, frame_bytes(|bytes| bytes[0x10] = 5));
    assert_eq!(state.capture_change(&mut scenery, handle(), CaptureDetector::LensMoved), CaptureChange::Toward(true));
    state.evaluation.against_reference.reference.note.reference = 0xfe;
    scenery.world.set_shared(&group_33::AT_07A0, frame_bytes(|bytes| bytes[0x14] = 2));
    property(&mut scenery, 0x7618, 4);
    assert_eq!(state.capture_change(&mut scenery, handle(), CaptureDetector::Reference), CaptureChange::Changed);
    property(&mut scenery, 0x7618, 5);
    assert_eq!(state.capture_change(&mut scenery, handle(), CaptureDetector::Reference), CaptureChange::None);
}

#[test]
fn shake_reduction_applies_within_the_scene_bounds() {
    let mut scenery = Scenery::default();
    scenery.world.set(&group_00::AT_0065, 1);
    property(&mut scenery, 0x78d0, 1);
    let features = Features { primary_level: 5, alternate_level: 50, bounds: [LevelBounds { high: 10, low: 0 }, LevelBounds { high: 60, low: 40 }], ..Features::ZERO };
    assert_eq!(with_shake_reduction(&mut scenery, handle(), &features, Scene::AUTO, SceneFlags::MOVING), SceneFlags::MOVING.with(SceneFlags::SHAKE_REDUCTION));
    assert_eq!(with_shake_reduction(&mut scenery, handle(), &features, Scene::HANDHELD_NIGHT, SceneFlags::NONE), SceneFlags::SHAKE_REDUCTION);
    assert_eq!(with_shake_reduction(&mut scenery, handle(), &Features { primary_level: 11, ..features }, Scene::CLOSEUP, SceneFlags::NONE), SceneFlags::NONE);
    assert_eq!(with_shake_reduction(&mut scenery, handle(), &features, Scene::LANDSCAPE, SceneFlags::NONE), SceneFlags::NONE);
    property(&mut scenery, 0x76dc, 1);
    assert_eq!(with_shake_reduction(&mut scenery, handle(), &Features { stationary: true, ..features }, Scene::AUTO, SceneFlags::NONE), SceneFlags::NONE);
}

#[test]
fn noting_a_reference_rebuilds_the_primary_codes_and_records_the_hint() {
    let mut scenery = Scenery::default();
    let mut state = SceneRecognition::new();
    state.reset(&mut scenery);
    state.record_code(Recognition { scene: Scene::LANDSCAPE, flags: SceneFlags::NONE });
    assert_eq!(state.secondary_codes().entries()[0], Some(Code::of(Recognition { scene: Scene::LANDSCAPE, flags: SceneFlags::NONE })));
    state.set_source(&mut scenery, SOURCE);
    put_source(&mut scenery, 3, 77);
    state.note_reference(&mut scenery, Recognition { scene: Scene::CLOSEUP, flags: SceneFlags::MOVING });
    let closeup = Code::of(Recognition { scene: Scene::CLOSEUP, flags: SceneFlags::MOVING });
    assert_eq!(state.primary_codes().entries()[0], Some(closeup));
    assert_eq!(state.primary_codes().entries()[1], None);
    assert_eq!(state.references().entries()[0].unwrap().note.grid[3], 77);
    assert_eq!(state.live_trace().hints.entries()[0], Some(Code::ZERO));
    state.note_reference(&mut scenery, Recognition { scene: Scene(21), flags: SceneFlags(3) });
    assert_eq!(state.references().entries()[1], None);
}

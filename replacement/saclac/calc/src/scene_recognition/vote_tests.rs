use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_profile_records::stock::shared::{group_27, group_33};
use xm1_saclac_profile_records::MeasurementSelection;

use super::fakes::{Scenery, TraceLine};
use super::{Mode, Recognition, Scene, SceneFlags, SceneRecognition};
use crate::exposure::fakes::handle;
use crate::exposure::ports::Property;
use crate::stock::scene_recognition::LOG;

const LANDSCAPE: Recognition = Recognition { scene: Scene::LANDSCAPE, flags: SceneFlags::NONE };

fn property(scenery: &mut Scenery, id: u32, value: u32) { scenery.world.property(Property(id), value); }

/// A world whose frame query names a landscape from its configured detectors, without scene
/// overrides.
fn landscape_scenery() -> Scenery {
    let mut scenery = Scenery::default();
    for (id, value) in [(0x75e7, 1), (0x75e1, 2), (0x4a12, 0xff), (0x4a13, 0xff)] {
        property(&mut scenery, id, value);
    }
    scenery
}

fn reset(scenery: &mut Scenery) -> SceneRecognition {
    let mut state = SceneRecognition::new();
    state.reset(scenery);
    state
}

fn published(scenery: &Scenery) -> (u32, u16) {
    let records = &scenery.world.records;
    (records.read_shared(handle(), &group_27::AT_073C).unwrap(), records.read_shared(handle(), &group_27::AT_0740).unwrap())
}

#[test]
fn live_view_publishes_the_majority_of_its_queries_and_calls_back_on_a_change() {
    let mut scenery = landscape_scenery();
    let mut state = reset(&mut scenery);
    assert_eq!(state.vote(&mut scenery, handle(), Mode::Live), LANDSCAPE);
    assert_eq!(state.vote(&mut scenery, handle(), Mode::Live), LANDSCAPE);
    assert_eq!(scenery.changes, [LANDSCAPE]);
    assert_eq!(published(&scenery), (5, 0));
}

#[test]
fn live_view_settles_on_its_recording_and_holds_until_released() {
    let mut scenery = landscape_scenery();
    let mut state = reset(&mut scenery);
    state.take_adjustments_changed();
    state.choose_group_hint(&mut scenery, Scene(18));
    property(&mut scenery, 0x75b0, 1);
    property(&mut scenery, 0x75b1, 2);
    assert_eq!(state.vote(&mut scenery, handle(), Mode::Live), Recognition::AUTO);
    assert!(scenery.changes.is_empty());
    assert_eq!(state.vote(&mut scenery, handle(), Mode::Live), LANDSCAPE);
    assert_eq!((scenery.changes.as_slice(), scenery.snapshots), ([LANDSCAPE].as_slice(), 1));
    assert!(state.evaluation().choices.holding);
    property(&mut scenery, 0x75b2, 1);
    property(&mut scenery, 0x75b5, 1);
    assert_eq!(state.vote(&mut scenery, handle(), Mode::Live), LANDSCAPE);
    assert!(!state.evaluation().choices.holding);
}

#[test]
fn a_frame_after_a_reset_is_not_recorded_until_a_group_is_hinted() {
    let mut scenery = landscape_scenery();
    let mut state = reset(&mut scenery);
    property(&mut scenery, 0x75b0, 1);
    property(&mut scenery, 0x75b1, 1);
    assert_eq!(state.vote(&mut scenery, handle(), Mode::Live), Recognition::AUTO);
    assert_eq!(state.references().entries()[0], None);
    scenery.world.select(MeasurementSelection::Record);
    assert_eq!(state.vote(&mut scenery, handle(), Mode::Live), LANDSCAPE);
}

fn capture_scenery() -> Scenery {
    let mut scenery = landscape_scenery();
    for (id, value) in [(0x75e6, 1), (0x7684, 1), (0x7686, 1), (0x76bf, 1), (0x7697, 1)] {
        property(&mut scenery, id, value);
    }
    scenery
}

#[test]
fn the_shot_keeps_the_published_group_while_its_frame_is_unchanged() {
    let mut scenery = capture_scenery();
    let mut state = reset(&mut scenery);
    state.published = Recognition { scene: Scene(18), flags: SceneFlags::NONE };
    assert_eq!(state.vote(&mut scenery, handle(), Mode::Capture).scene, Scene(18));
    assert_eq!(state.capture_trace().group, Scene(18));
    assert_eq!(published(&scenery), (18, 0));
    property(&mut scenery, 0x76b3, 4);
    property(&mut scenery, 0x76b9, 1);
    assert_eq!(state.vote(&mut scenery, handle(), Mode::Capture), LANDSCAPE);
    assert!(state.capture_trace().detectors.lens_changed && state.live_trace().detectors.lens_changed);
    assert!(scenery.changes.is_empty());
}

#[test]
fn overrides_reach_the_record_and_the_on_screen_text() {
    let mut scenery = Scenery::default();
    let mut state = reset(&mut scenery);
    property(&mut scenery, 0x4a13, 21);
    property(&mut scenery, 0x65a8, 0x20);
    property(&mut scenery, 0xeb, 1);
    assert_eq!(state.vote(&mut scenery, handle(), Mode::Capture), Recognition { scene: Scene(21), flags: SceneFlags::MOVING });
    assert_eq!(published(&scenery), (21, 1));
    assert_eq!(scenery.lines, [(b"SNOW                ".to_vec(), [2, 8, 2, 9]), (b"&MOTION".to_vec(), [2, 9, 2, 9])]);
}

#[test]
fn a_record_measurement_logs_each_vote_in_turn() {
    let mut scenery = Scenery::default();
    let mut state = reset(&mut scenery);
    scenery.world.select(MeasurementSelection::Record);
    property(&mut scenery, 0xa8, 0x80);
    let mut frame = [0u8; 76];
    frame[0x38] = 7;
    scenery.world.set_shared(&group_33::AT_07A0, frame);
    state.vote(&mut scenery, handle(), Mode::Capture);
    state.vote(&mut scenery, handle(), Mode::Capture);
    assert_eq!(scenery.world.u32_at(SharedAddress(LOG.0 + 12)), 7);
    assert_eq!(scenery.world.u32_at(SharedAddress(LOG.0 + 60 + 12)), 7);
}

#[test]
fn the_trace_names_the_published_scene_and_keeps_the_response_magnitude() {
    let mut scenery = Scenery::default();
    let mut state = reset(&mut scenery);
    scenery.world.property_word(Property(0xe8), 0x400);
    state.evaluation.features.response = -5;
    state.vote(&mut scenery, handle(), Mode::Capture);
    let TraceLine::Measurements(values) = scenery.trace[2] else { panic!("no measurements line") };
    assert_eq!(&scenery.trace[..2], [TraceLine::Name(Scene::AUTO), TraceLine::Flags(SceneFlags::NONE)]);
    assert_eq!((values[13], state.evaluation().features.response), (5, 5));
}

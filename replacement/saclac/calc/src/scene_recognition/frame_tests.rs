use xm1_saclac_profile_records::stock::shared::{group_27, group_33};

use super::fakes::Scenery;
use super::{Mode, Recognition, Scene, SceneFlags, SceneRecognition};
use crate::exposure::fakes::handle;
use crate::exposure::ports::Property;
use crate::stock::property::level_step;

const LENS: u32 = 4;
const STEP: u32 = 6;

fn put_half(bytes: &mut [u8; 76], at: usize, value: i16) { bytes[at..at + 2].copy_from_slice(&value.to_le_bytes()) }

fn put_word(bytes: &mut [u8; 76], at: usize, value: u32) { bytes[at..at + 4].copy_from_slice(&value.to_le_bytes()) }

fn live_scenery() -> Scenery {
    let mut scenery = Scenery::default();
    let mut frame = [0; 76];
    put_half(&mut frame, 0x28, 100);
    put_half(&mut frame, 0x2a, 40);
    put_half(&mut frame, 0x2c, 7);
    put_half(&mut frame, 0x2e, 8);
    put_half(&mut frame, 0x30, 9);
    put_word(&mut frame, 0x38, LENS);
    put_word(&mut frame, 0x3c, STEP);
    put_half(&mut frame, 0x48, 12);
    scenery.world.set_shared(&group_33::AT_07A0, frame);
    scenery.world.reference_offsets.insert(LENS, 5);
    scenery.lens.reference = 0x55;
    scenery.world.property(Property(0x75c7), 1);
    scenery
}

fn property(scenery: &mut Scenery, id: u32, value: u32) { scenery.world.property(Property(id), value); }

#[test]
fn live_levels_are_measured_against_the_lens() {
    let mut scenery = live_scenery();
    let mut state = SceneRecognition::new();
    state.query_frame(&mut scenery, handle(), Mode::Live);
    let features = state.evaluation().features;
    assert_eq!((features.level, features.upper_level, features.lower_level), (128, 123, 63));
    assert_eq!((features.primary_level, features.alternate_level, features.third_level), (35, 36, 32));
    assert_eq!((features.reference, features.state, features.position), (LENS, STEP, 12));
}

#[test]
fn the_shot_raises_its_levels_by_level_step_two() {
    let mut scenery = Scenery::default();
    scenery.world.property_word(level_step(2), 10);
    let mut frame = [0; 76];
    put_half(&mut frame, 0x00, 50);
    put_half(&mut frame, 0x02, 1);
    put_half(&mut frame, 0x10, 3);
    scenery.world.set_shared(&group_33::AT_07A0, frame);
    let mut state = SceneRecognition::new();
    state.query_frame(&mut scenery, handle(), Mode::Capture);
    let features = state.evaluation().features;
    assert_eq!((features.level, features.upper_level, features.primary_level, features.position), (60, 60, 11, 3));
}

#[test]
fn the_response_follows_the_configured_line() {
    let mut scenery = live_scenery();
    scenery.lens.primary.insert(LENS, 150);
    for (id, value) in [(0x75c9, 1), (0x75c7, 2), (0x75c8, 3), (0x75c6, 4)] {
        property(&mut scenery, id, value);
    }
    let mut state = SceneRecognition::new();
    state.query_frame(&mut scenery, handle(), Mode::Live);
    assert_eq!(state.evaluation().features.response, 250 + STEP as i16);
    scenery.lens.primary.insert(LENS, 400);
    state.query_frame(&mut scenery, handle(), Mode::Live);
    assert_eq!(state.evaluation().features.response, 400 + STEP as i16);
}

#[test]
fn the_majoritys_detector_answers_first() {
    let mut scenery = live_scenery();
    let mut state = SceneRecognition::new();
    state.reset(&mut scenery);
    state.record_code(Recognition { scene: Scene::LANDSCAPE, flags: SceneFlags::NONE });
    let answer = state.query_frame(&mut scenery, handle(), Mode::Live);
    assert_eq!(answer, Recognition { scene: Scene::LANDSCAPE, flags: SceneFlags::NONE });
    assert_eq!(state.evaluation().features.tried, [2, 0, 0, 0, 0, 0]);
    assert_eq!(state.evaluation().features.selection, answer);
}

#[test]
fn the_majoritys_flags_answer_even_when_its_detector_names_none() {
    let mut scenery = live_scenery();
    let mut state = SceneRecognition::new();
    state.reset(&mut scenery);
    state.record_code(Recognition { scene: Scene::CLOSEUP, flags: SceneFlags::MOVING });
    let answer = state.query_frame(&mut scenery, handle(), Mode::Live);
    assert_eq!(answer, Recognition { scene: Scene::AUTO, flags: SceneFlags::MOVING });
    assert_eq!(state.evaluation().features.tried, [4, 0, 0, 0, 0, 0]);
}

#[test]
fn the_configured_detectors_follow_in_order_with_the_motion_record() {
    let mut scenery = live_scenery();
    property(&mut scenery, 0x75e7, 1);
    property(&mut scenery, 0x75e1, 4);
    property(&mut scenery, 0x75e3, 9);
    property(&mut scenery, 0x75e4, 2);
    let mut motion = [0u8; 40];
    motion[0x10] = 3;
    scenery.world.set_shared(&group_27::AT_07EC, motion);
    let mut state = SceneRecognition::new();
    let answer = state.query_frame(&mut scenery, handle(), Mode::Live);
    assert_eq!(answer.scene, Scene::LANDSCAPE);
    assert_eq!(state.evaluation().features.tried, [4, 5, 2, 0, 0, 0]);
    assert_eq!(state.evaluation().features.motion.dominant, 3);
}

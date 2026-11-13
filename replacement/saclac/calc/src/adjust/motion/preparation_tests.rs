use xm1_saclac_profile_records::stock::record::group_01;
use xm1_saclac_profile_records::stock::shared::group_15;
use xm1_saclac_profile_records::SharedHeaderByte;

use crate::adjust::motion::preparation::{searches_in_full, FrameSize, PreviousFocus};
use crate::adjust::motion::readings::{AreaList, FocusRequest, ListedArea, PreparationInput, RecordReadings};
use crate::adjust::motion::world::{Lists, MotionWorld};
use crate::exposure::fakes::handle;
use crate::stock::motion::{self as stock};
use crate::stock::property;

const LENS: u32 = 7;

fn confirmation(words: [u32; 8]) -> [u8; 32] {
    let mut bytes = [0; 32];
    for (index, word) in words.into_iter().enumerate() {
        bytes[4 * index..4 * index + 4].copy_from_slice(&word.to_le_bytes());
    }
    bytes
}

fn confirmed_world(kept: u32, held: u32) -> MotionWorld {
    let mut world = MotionWorld::default();
    world.world.set_shared(&group_15::FOCUS_CONFIRMATION, confirmation([1, kept, 0, LENS, 0, 0, held, 0]));
    world
}

fn readings(world: &mut MotionWorld) -> RecordReadings {
    RecordReadings::read(world, &mut Lists::default(), &PreparationInput { record: handle(), lens: LENS, load_test: false, request: FocusRequest::default() })
}

fn decide(world: &mut MotionWorld, treated: bool) -> PreviousFocus {
    let readings = readings(world);
    PreviousFocus::decide(world, &readings, treated)
}

#[test]
fn a_confirmation_for_another_lens_or_none_is_ignored() {
    let mut world = MotionWorld::default();
    world.property(stock::PREVIOUS_FOCUS_MODE, 2);
    assert_eq!(decide(&mut world, false), PreviousFocus::Ignored);
    world.world.set_shared(&group_15::FOCUS_CONFIRMATION, confirmation([1, 0, 0, LENS + 1, 0, 0, 0, 0]));
    assert_eq!(decide(&mut world, false), PreviousFocus::Ignored);
}

#[test]
fn mode_two_reports_a_confirmation_or_checks_a_kept_one() {
    let mut world = confirmed_world(0, 2);
    world.property(stock::PREVIOUS_FOCUS_MODE, 2);
    assert_eq!(decide(&mut world, false), PreviousFocus::Reported);
    let mut world = confirmed_world(1, 2);
    world.property(stock::PREVIOUS_FOCUS_MODE, 2);
    assert_eq!(decide(&mut world, false), PreviousFocus::Checked);
}

#[test]
fn a_confirmation_held_for_the_other_search_is_only_checked_in_mode_two() {
    for (held, treated) in [(0, true), (1, false)] {
        let mut world = confirmed_world(0, held);
        world.property(stock::PREVIOUS_FOCUS_MODE, 1);
        assert_eq!(decide(&mut world, treated), PreviousFocus::Ignored);
        world.property(stock::PREVIOUS_FOCUS_MODE, 2);
        assert_eq!(decide(&mut world, treated), PreviousFocus::Checked);
    }
}

#[test]
fn otherwise_the_record_class_must_be_admitted_at_a_high_enough_level() {
    let mut world = confirmed_world(1, 2);
    world.property(stock::PREVIOUS_FOCUS_LEVEL, 50);
    world.property(stock::PREVIOUS_FOCUS_CLASSES, 4);
    world.world.header(SharedHeaderByte::At47, 5);
    world.world.set(&group_01::AT_0092, 499);
    assert_eq!(decide(&mut world, false), PreviousFocus::Ignored, "below the configured level");
    world.world.set(&group_01::AT_0092, 500);
    assert_eq!(decide(&mut world, false), PreviousFocus::Admitted);
    world.world.header(SharedHeaderByte::At46, 1);
    assert_eq!(decide(&mut world, false), PreviousFocus::Ignored, "the other channels read their own mask");
    world.property(stock::PREVIOUS_FOCUS_CHANNEL_CLASSES, 4);
    assert_eq!(decide(&mut world, false), PreviousFocus::Admitted);
    world.property_word(property::level_step(2), (-1i16) as u16);
    assert_eq!(decide(&mut world, false), PreviousFocus::Ignored, "level step 2 counts toward the level");
}

#[test]
fn listed_areas_scale_from_the_selection_frame_to_the_modes_frame() {
    let mut world = MotionWorld::default();
    world.frames.insert(stock::SELECTION_MODE, FrameSize { height: 500, width: 1000 });
    world.frames.insert(0x22, FrameSize { height: 1500, width: 2000 });
    let area = ListedArea { x: 100, y: 100, height: 40, width: 50, centre_x: 125, centre_y: 120, at_18: 9, at_22: 3, ..ListedArea::default() };
    let mut list = AreaList { count: 1, ..AreaList::default() };
    list.areas[0] = area;
    list.areas[1] = area;
    let converted = list.in_mode(&mut world, 0x22);
    assert_eq!(converted.areas[0], ListedArea { x: 200, y: 300, height: 120, width: 100, centre_x: 250, centre_y: 360, ..area });
    assert_eq!(converted.areas[1], area, "areas past the count stay as they were");
}

#[test]
fn a_run_searches_in_full_unless_it_starts_from_a_result() {
    let mut world = MotionWorld::default();
    let plain = readings(&mut world);
    assert!(searches_in_full(&mut world, &plain, PreviousFocus::Admitted));
    assert!(!searches_in_full(&mut world, &plain, PreviousFocus::Reported));
    assert!(!searches_in_full(&mut world, &plain, PreviousFocus::Checked));
    world.control_state = 2;
    assert!(!searches_in_full(&mut world, &plain, PreviousFocus::Ignored));
    world.control_state = 0;
    world.property(stock::NO_FULL_SEARCH, 1);
    assert!(!searches_in_full(&mut world, &plain, PreviousFocus::Ignored));
    world.property(stock::NO_FULL_SEARCH, 0);
    world.world.header(SharedHeaderByte::At31, stock::SCENE_WITHOUT_SEARCH);
    let scene = readings(&mut world);
    assert!(!searches_in_full(&mut world, &scene, PreviousFocus::Ignored));
}

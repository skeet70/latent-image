use xm1_saclac_interface::calc::SharedAddress;

use super::history::{majority, Code};
use super::{Recognition, Scene, SceneFlags, SceneRecognition};
use crate::exposure::fakes::World;
use crate::exposure::ports::StockMemory;
use crate::stock::scene_recognition::{self as stock, workspace_field, Width, Workspace};

fn recognition(scene: u32, flags: u16) -> Recognition { Recognition { scene: Scene(scene), flags: SceneFlags(flags) } }

fn code(scene: u32, flags: u16) -> Option<Code> { Some(Code::of(recognition(scene, flags))) }

fn reset() -> (SceneRecognition, World) {
    let mut state = SceneRecognition::new();
    let mut world = World::default();
    state.reset(&mut world);
    (state, world)
}

#[test]
fn coded_scenes_and_flags_round_trip() {
    for scene in [0, 1, 4, 5, 11, 18, 27, 42, 48] {
        for flags in 0..=2 {
            assert_eq!(Code::of(recognition(scene, flags)).recognition(), recognition(scene, flags), "scene {scene} flags {flags}");
        }
    }
}

#[test]
fn an_uncoded_scene_codes_as_auto_without_flags() {
    assert_eq!(Code::of(recognition(2, 1)).recognition(), Recognition::AUTO);
    assert_eq!(Code::of(recognition(57, 2)).recognition(), Recognition::AUTO);
}

#[test]
fn both_flags_decode_as_auto() {
    assert_eq!(Code::of(recognition(11, 3)).recognition(), Recognition::AUTO);
}

#[test]
fn snow_with_both_flags_is_not_recordable() {
    assert!(!Code::of(recognition(21, 3)).recordable());
    assert!(Code::of(recognition(21, 2)).recordable());
}

#[test]
fn majority_waits_for_a_full_window() {
    let entries = [code(5, 0), code(5, 0), None, None];
    assert_eq!(majority(&entries, 3), Recognition::AUTO);
    assert_eq!(majority(&entries, 2), recognition(5, 0));
}

#[test]
fn majority_takes_the_most_frequent_code() {
    let entries = [code(1, 0), code(5, 0), code(5, 0), code(1, 1)];
    assert_eq!(majority(&entries, 4), recognition(5, 0));
}

#[test]
fn majority_prefers_the_newer_of_equal_counts() {
    let entries = [code(1, 0), code(5, 0), code(5, 0), code(1, 0)];
    assert_eq!(majority(&entries, 4), recognition(1, 0));
}

#[test]
fn majority_counts_codes_with_both_flags() {
    let entries = [code(48, 3), code(48, 3), code(5, 0)];
    assert_eq!(majority(&entries, 3), Recognition::AUTO);
}

#[test]
fn reset_empties_the_code_lists_and_the_references() {
    let (state, _) = reset();
    for workspace in [Workspace::PrimaryCodes, Workspace::SecondaryCodes, Workspace::Changes, Workspace::LiveTrace] {
        let at = if workspace == Workspace::LiveTrace { 0x10 } else { 0 };
        for index in 0..20 {
            assert_eq!(workspace_field(&state, workspace, at + index, Width::Byte), 100, "{workspace:?} {index}");
        }
    }
    for index in 0..20 {
        assert_eq!(workspace_field(&state, Workspace::References, 0x214 * index, Width::Byte), 100);
    }
}

#[test]
fn before_any_reset_the_lists_hold_code_zero() {
    let state = SceneRecognition::new();
    assert_eq!(workspace_field(&state, Workspace::PrimaryCodes, 0, Width::Byte), 0);
    assert_eq!(workspace_field(&state, Workspace::References, 0x214, Width::Byte), 0);
}

#[test]
fn reset_clears_the_whole_log() {
    let mut state = SceneRecognition::new();
    let mut world = World::default();
    let last = SharedAddress(stock::LOG.0 + stock::LOG_BYTES - 4);
    world.write_u32(last, 0x1234_5678);
    state.reset(&mut world);
    assert_eq!(world.u32_at(last), 0);
    assert_eq!(world.u32_at(stock::LOG), 0);
}

#[test]
fn reset_publishes_auto() {
    let (state, _) = reset();
    assert_eq!(state.published(), Recognition::AUTO);
}

#[test]
fn the_dump_section_carries_marks_settings_and_the_log() {
    let (state, mut world) = reset();
    for (index, id) in stock::DUMP_SETTINGS.into_iter().enumerate() {
        world.property(id, 3 + index as u32);
    }
    world.write_u32(stock::log_entry(0), 0x0102_0304);
    world.write_u32(SharedAddress(stock::log_entry(1).0 + 4), 0xaaaa_5555);
    let frame = SharedAddress(0x0200_0000);
    state.store_dump_section(&mut world, frame);
    let word = |offset: u32| world.u32_at(SharedAddress(frame.0 + offset));
    assert_eq!(word(0x1e00), 0x5354_534d);
    assert_eq!([word(0x1e04), word(0x1e08), word(0x1e0c), word(0x1e10), word(0x1e14)], [30, 40, 50, 60, 70]);
    assert_eq!(word(0x1e18 + 60 + 4), 0xaaaa_5555);
    assert_eq!(word(0x1e18), 0, "the entry the next log write takes");
    assert_eq!(word(0x1e00 + 0x17c4), 0x4c41_534d);
}

#[test]
fn the_corrected_grid_reads_the_source_when_corrected() {
    let (mut state, mut world) = reset();
    let source = SharedAddress(0x0300_0000);
    world.put_u16(SharedAddress(source.0 + 2 * 7), 500);
    state.set_source(&mut world, source);
    world.put_u16(SharedAddress(source.0 + 2 * 7), 600);
    state.correct_grid(&mut world);
    assert_eq!(state.corrected_grid()[7], 600);
}

#[test]
fn workspace_fields_zero_extend_at_the_selected_record_boundary() {
    let (state, _) = reset();
    assert_eq!(workspace_field(&state, Workspace::References, 0x213, Width::Word), 0);
    assert_eq!(workspace_field(&state, Workspace::References, 0x214, Width::Byte), 100);
    assert_eq!(workspace_field(&state, Workspace::References, 20 * 0x214, Width::Word), 0);
    for workspace in [Workspace::PrimaryCodes, Workspace::SecondaryCodes, Workspace::Changes] {
        assert_eq!(workspace_field(&state, workspace, 19, Width::Word), 100);
        assert_eq!(workspace_field(&state, workspace, 20, Width::SignedHalf), 0);
    }
    assert_eq!(workspace_field(&state, Workspace::Stationary, 20 * 0x24, Width::Word), 0);
    assert_eq!(workspace_field(&state, Workspace::Comparisons, 5 * 0x450, Width::Word), 0);
}

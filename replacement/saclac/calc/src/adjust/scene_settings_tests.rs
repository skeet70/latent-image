use xm1_saclac_profile_records::MeasurementSelection;

use crate::adjust::scene_settings::*;
use crate::exposure::fakes::World;
use crate::exposure::ports::Property;

fn world(settings: &[(u32, u32)]) -> World {
    let mut world = World::default();
    for &(id, value) in settings {
        world.property(Property(id), value);
    }
    world
}

fn scene(scene: u32) -> SceneSelection { SceneSelection { scene, ..SceneSelection::default() } }

#[test]
fn a_scene_reads_its_nibble_byte_low_nibble_first() {
    let mut world = world(&[(0x4a54, 0xa7)]);
    assert_eq!(nibbles(&mut world, NibbleInputs { scene: 3, ..NibbleInputs::default() }), Nibbles { low: 7, high: 0xa });
}

#[test]
fn flags_selector_arming_and_choice_take_precedence_in_that_order() {
    let mut world = world(&[(0x4a63, 0x11), (0x4a69, 0x22), (0x4a68, 0x33), (0x1_787c, 0x44), (0x4a4e, 0x55)]);
    let base = NibbleInputs { scene: 0, flags: 2, selector: 4, armed: true, choice: 13, ..NibbleInputs::default() };
    assert_eq!(nibbles(&mut world, base).low, 1);
    assert_eq!(nibbles(&mut world, NibbleInputs { flags: 0, ..base }).low, 2);
    assert_eq!(nibbles(&mut world, NibbleInputs { flags: 0, selector: 0, ..base }).low, 3);
    assert_eq!(nibbles(&mut world, NibbleInputs { flags: 0, selector: 0, armed: false, ..base }).low, 4);
    assert_eq!(nibbles(&mut world, NibbleInputs { flags: 0, selector: 0, armed: false, choice: 0, ..base }).low, 5);
}

#[test]
fn arming_only_replaces_scenes_0_and_48() {
    let mut world = world(&[(0x4a68, 0x33), (0x4a53, 0x66)]);
    assert_eq!(nibbles(&mut world, NibbleInputs { scene: 1, armed: true, ..NibbleInputs::default() }).low, 6);
    assert_eq!(nibbles(&mut world, NibbleInputs { scene: 48, armed: true, ..NibbleInputs::default() }).low, 3);
}

#[test]
fn the_policy_chooses_the_nibbles_only_until_a_level_is_measured() {
    let mut world = world(&[(0x4a4b, 0x01), (0x4a4f, 0x02), (0x4a50, 0x03)]);
    let unmeasured = NibbleInputs { scene: 45, policy: 3, ..NibbleInputs::default() };
    assert_eq!(nibbles(&mut world, unmeasured).low, 1);
    assert_eq!(nibbles(&mut world, NibbleInputs { measured_level: 40, ..unmeasured }).low, 2);
    assert_eq!(nibbles(&mut world, NibbleInputs { scene: 47, measured_level: 40, ..unmeasured }).low, 3);
}

#[test]
fn a_scene_enables_its_adjustment_by_one_bit() {
    let mut world = world(&[(0x6594, 0x04)]);
    let inputs = |scene_kind| EnableInputs { selection: scene(scene_kind), ..EnableInputs::default() };
    assert!(adjustment_enabled(&mut world, inputs(10)));
    assert!(!adjustment_enabled(&mut world, inputs(11)));
    assert!(!adjustment_enabled(&mut world, inputs(13)));
    assert!(adjustment_enabled(&mut world, inputs(17)));
}

#[test]
fn an_auxiliary_byte_or_a_measurement_selection_disables_the_adjustment() {
    let mut world = world(&[]);
    let selected = SceneSelection { measurement_selection: MeasurementSelection::Record, ..scene(17) };
    assert!(!adjustment_enabled(&mut world, EnableInputs { selection: selected, ..EnableInputs::default() }));
    assert!(!adjustment_enabled(&mut world, EnableInputs { selection: scene(17), auxiliary: 1, ..EnableInputs::default() }));
}

#[test]
fn flags_bit_1_reads_the_flagged_bit_for_every_scene() {
    let mut world = world(&[(0x6595, 0x20)]);
    let flagged = SceneSelection { flags: 2, ..scene(13) };
    assert!(adjustment_enabled(&mut world, EnableInputs { selection: flagged, ..EnableInputs::default() }));
}

#[test]
fn scenes_44_and_57_read_the_bit_of_their_selector_unless_held() {
    let mut world = world(&[(0x65a3, 0x02)]);
    let inputs = EnableInputs { selection: scene(57), selector: 2, ..EnableInputs::default() };
    assert!(adjustment_enabled(&mut world, inputs));
    assert!(!adjustment_enabled(&mut world, EnableInputs { selector: 3, ..inputs }));
    assert!(!adjustment_enabled(&mut world, EnableInputs { selector: 1, ..inputs }));
    assert!(!adjustment_enabled(&mut world, EnableInputs { hold: 1, ..inputs }));
}

#[test]
fn the_curve_table_follows_the_scene_unless_flagged_or_selected() {
    assert_eq!(curve_table(scene(17)), Property(0x6504));
    assert_eq!(curve_table(scene(24)), Property(0x64e0));
    assert_eq!(curve_table(SceneSelection { flags: 2, ..scene(17) }), Property(0x64bc));
    assert_eq!(curve_table(SceneSelection { measurement_selection: MeasurementSelection::Record, ..scene(17) }), Property(0x64bc));
}

fn range_world() -> World {
    let mut world = World::default();
    for (index, id) in [0x60c2, 0x608b, 0x60f1, 0x60ba, 0x60bc, 0x6085, 0x60e0, 0x60a9, 0x60c4, 0x608d].into_iter().enumerate() {
        world.property(Property(id), index as u32 + 1);
    }
    for (id, value) in [(0x6018_u32, -5_i16), (0x6082, 7), (0x600c, 9), (0x6060, -300), (0x601c, 11)] {
        world.property_word(Property(id), value as u16);
    }
    world
}

#[test]
fn the_range_reads_tenths_and_a_signed_offset() {
    let mut world = range_world();
    let inputs = RangeInputs { selection: scene(2), ..RangeInputs::default() };
    assert_eq!(range(&mut world, inputs), SceneRange { above: 90, below: 100, offset: 11 });
}

#[test]
fn flags_selection_and_arming_replace_the_scene_range() {
    let mut world = range_world();
    let base = RangeInputs { selection: SceneSelection { flags: 2, measurement_selection: MeasurementSelection::Record, scene: 2 }, class: 9, armed: true, ..RangeInputs::default() };
    assert_eq!(range(&mut world, base), SceneRange { above: 10, below: 20, offset: -5 });
    let selected = RangeInputs { selection: SceneSelection { flags: 0, ..base.selection }, ..base };
    assert_eq!(range(&mut world, selected), SceneRange { above: 30, below: 40, offset: 7 });
    let armed = RangeInputs { selection: scene(2), ..base };
    assert_eq!(range(&mut world, armed), SceneRange { above: 50, below: 60, offset: 9 });
}

#[test]
fn an_unmeasured_level_takes_the_range_of_the_records_policy() {
    let mut world = range_world();
    let inputs = RangeInputs { selection: scene(46), unmeasured: true, policy: 2, variant: 1, ..RangeInputs::default() };
    assert_eq!(range(&mut world, inputs), SceneRange { above: 70, below: 80, offset: -300 });
}

use xm1_saclac_profile_records::MeasurementSelection;

use crate::adjust::scene_policy::*;
use crate::exposure::fakes::World;
use crate::stock::property;

fn inputs(scene: u32, class: u32) -> TreatmentInputs {
    TreatmentInputs { active: 1, subjects: 2, scene, class, ..TreatmentInputs::default() }
}

fn both(inputs: TreatmentInputs) -> (bool, bool) { (inputs.admits(Treatment::First), inputs.admits(Treatment::Second)) }

#[test]
fn no_treatment_applies_without_subjects_or_without_activity_and_hold() {
    assert_eq!(both(inputs(1, 1)), (true, true));
    assert_eq!(both(TreatmentInputs { subjects: 0, ..inputs(1, 1) }), (false, false));
    assert_eq!(both(TreatmentInputs { active: 0, ..inputs(1, 1) }), (false, false));
    assert_eq!(both(TreatmentInputs { active: 0, held: 1, ..inputs(0, 1) }), (false, true));
}

#[test]
fn the_class_selects_the_column_of_a_standard_scene() {
    assert_eq!(both(inputs(1, 0)), (false, false));
    for class in [4, 5, 6, 3, 7] {
        assert_eq!(both(inputs(1, class)), (true, true), "class {class}");
    }
    assert_eq!(both(TreatmentInputs { single_subject: 1, ..inputs(1, 1) }), (false, true));
    assert_eq!(both(TreatmentInputs { held: 1, ..inputs(1, 1) }), (false, false));
}

#[test]
fn scenes_differ_in_what_they_admit() {
    assert_eq!(both(TreatmentInputs { held: 1, ..inputs(45, 1) }), (false, true));
    assert_eq!(both(inputs(11, 1)), (false, true));
    assert_eq!(both(inputs(11, 0)), (false, false));
    assert_eq!(both(inputs(6, 1)), (false, false));
    assert_eq!(both(inputs(49, 0)), (true, false));
    assert_eq!(both(inputs(57, 5)), (true, true));
    assert_eq!(both(inputs(58, 5)), (false, false));
}

#[test]
fn selection_and_arming_replace_the_scene_row() {
    let closed = inputs(6, 1);
    assert_eq!(both(TreatmentInputs { measurement_selection: MeasurementSelection::Other(2), ..closed }), (true, true));
    assert_eq!(both(TreatmentInputs { callback_armed: 1, class: 0, ..closed }), (true, false));
    assert_eq!(both(TreatmentInputs { measurement_selection: MeasurementSelection::Record, callback_armed: 1, class: 0, ..closed }), (false, false));
}

#[test]
fn any_treatment_is_either_one() {
    assert!(inputs(11, 1).admits_any());
    assert!(inputs(49, 0).admits_any());
    assert!(!inputs(6, 1).admits_any());
}

/// Settings with one scene mask bit on and both treatment settings on.
fn mask(byte: u8, bit: u8) -> World {
    let mut world = World::default();
    for each in 0..5 {
        world.property(property::scene_mask(each), 0);
    }
    world.property(property::scene_mask(byte), u32::from(bit));
    world.property(property::DUMP_FIRST, 1);
    world.property(property::DUMP_SECOND, 1);
    world
}

fn mask_inputs(scene: u32) -> SceneMaskInputs { SceneMaskInputs { scene, subjects: 1, ..SceneMaskInputs::default() } }

#[test]
fn each_scene_reads_its_own_mask_bit() {
    assert!(scene_enabled(&mut mask(0, 1), mask_inputs(0)));
    assert!(!scene_enabled(&mut mask(0, 1), mask_inputs(1)));
    assert!(scene_enabled(&mut mask(2, 1), mask_inputs(1)));
    assert!(!scene_enabled(&mut mask(2, 2), mask_inputs(1)));
    assert!(scene_enabled(&mut mask(4, 0x80), mask_inputs(17)));
    assert!(scene_enabled(&mut mask(4, 0x80), mask_inputs(200)));
    assert!(scene_enabled(&mut mask(3, 0x20), mask_inputs(29)));
    assert!(scene_enabled(&mut mask(1, 0x10), mask_inputs(56)));
}

#[test]
fn a_selected_or_armed_record_reads_its_own_bit() {
    let selected = SceneMaskInputs { measurement_selection: MeasurementSelection::Record, callback_armed: 1, ..mask_inputs(0) };
    assert!(scene_enabled(&mut mask(1, 0x40), selected));
    assert!(!scene_enabled(&mut mask(0, 1), selected));
    let armed = SceneMaskInputs { callback_armed: 1, ..mask_inputs(0) };
    assert!(scene_enabled(&mut mask(1, 0x80), armed));
    assert!(!scene_enabled(&mut mask(1, 0x40), armed));
}

#[test]
fn a_treated_exposure_needs_both_settings_and_a_subject() {
    let treated = SceneMaskInputs { treated: true, ..mask_inputs(0) };
    assert!(scene_enabled(&mut mask(0, 1), treated));
    assert!(!scene_enabled(&mut mask(0, 1), SceneMaskInputs { subjects: 0, ..treated }));
    let mut first_off = mask(0, 1);
    first_off.property(property::DUMP_FIRST, 0);
    assert!(!scene_enabled(&mut first_off, treated));
    assert!(scene_enabled(&mut first_off, SceneMaskInputs { subjects: 0, ..mask_inputs(0) }));
    let mut second_off = mask(0, 1);
    second_off.property(property::DUMP_SECOND, 0);
    assert!(!scene_enabled(&mut second_off, treated));
}

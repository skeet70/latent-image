use std::cell::Cell;

use crate::adjust::scene_policy::*;

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
    assert_eq!(both(TreatmentInputs { measurement_selection: 2, ..closed }), (true, true));
    assert_eq!(both(TreatmentInputs { callback_armed: 1, class: 0, ..closed }), (true, false));
    assert_eq!(both(TreatmentInputs { measurement_selection: 1, callback_armed: 1, class: 0, ..closed }), (false, false));
}

#[test]
fn any_treatment_is_either_one() {
    assert!(inputs(11, 1).admits_any());
    assert!(inputs(49, 0).admits_any());
    assert!(!inputs(6, 1).admits_any());
}

struct Mask {
    enabled: [u8; 5],
    level_one: bool,
    asked: Cell<Option<u8>>,
}

impl Mask {
    fn with(byte: usize, bit: u8) -> Self {
        let mut enabled = [0; 5];
        enabled[byte] = bit;
        Self { enabled, level_one: true, asked: Cell::new(None) }
    }
}

impl SceneMaskSettings for Mask {
    fn enabled(&self, flag: SceneFlag) -> bool {
        self.asked.set(Some(flag.0));
        self.enabled[usize::from(flag.0 / 8)] & (1 << (flag.0 % 8)) != 0
    }
    fn level_one_admitted(&self) -> bool { self.level_one }
}

fn mask_inputs(scene: u32) -> SceneMaskInputs { SceneMaskInputs { scene, subjects: 1, ..SceneMaskInputs::default() } }

#[test]
fn each_scene_reads_its_own_mask_bit() {
    assert!(scene_enabled(&Mask::with(0, 1), mask_inputs(0)));
    assert!(!scene_enabled(&Mask::with(0, 1), mask_inputs(1)));
    assert!(scene_enabled(&Mask::with(2, 1), mask_inputs(1)));
    assert!(scene_enabled(&Mask::with(4, 0x80), mask_inputs(17)));
    assert!(scene_enabled(&Mask::with(4, 0x80), mask_inputs(200)));
    assert!(scene_enabled(&Mask::with(3, 0x20), mask_inputs(29)));
    assert!(scene_enabled(&Mask::with(1, 0x10), mask_inputs(56)));
}

#[test]
fn a_selected_or_armed_record_reads_its_own_bit() {
    let mask = Mask::with(1, 0x40);
    assert!(scene_enabled(&mask, SceneMaskInputs { measurement_selection: 1, callback_armed: 1, ..mask_inputs(0) }));
    assert_eq!(mask.asked.get(), Some(14));
    let mask = Mask::with(1, 0x80);
    assert!(scene_enabled(&mask, SceneMaskInputs { callback_armed: 1, ..mask_inputs(0) }));
    assert_eq!(mask.asked.get(), Some(15));
}

#[test]
fn level_one_needs_both_settings_and_subjects() {
    let mut mask = Mask::with(0, 1);
    assert!(scene_enabled(&mask, SceneMaskInputs { level: 1, ..mask_inputs(0) }));
    assert!(!scene_enabled(&mask, SceneMaskInputs { level: 1, subjects: 0, ..mask_inputs(0) }));
    mask.level_one = false;
    assert!(!scene_enabled(&mask, SceneMaskInputs { level: 1, ..mask_inputs(0) }));
    assert!(scene_enabled(&mask, SceneMaskInputs { level: 2, subjects: 0, ..mask_inputs(0) }));
}

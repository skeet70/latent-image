use super::{ApertureIndex, ApertureValue, ShutterValue, f_63ba38, f_72ccac, nearest_aperture, shutter_code};
use crate::mode_initialization::CaptureModeId;

#[test]
fn aperture_mode_gate_accepts_only_the_two_manual_modes() {
    for mode in [0, 1, 0x22, 0x25, u32::MAX] { assert!(!f_63ba38(CaptureModeId::new(mode))); }
    for mode in [0x23, 0x24] { assert!(f_63ba38(CaptureModeId::new(mode))); }
}

#[test]
fn aperture_selection_uses_nearest_value_and_first_tie() {
    let values = [100, 140, 200, 280];
    assert_eq!(nearest_aperture(ApertureValue(0), &values), ApertureIndex(1));
    assert_eq!(nearest_aperture(ApertureValue(170), &values), ApertureIndex(2));
    assert_eq!(nearest_aperture(ApertureValue(201), &values), ApertureIndex(3));
    assert_eq!(nearest_aperture(ApertureValue(1000), &values), ApertureIndex(4));
    assert_eq!(nearest_aperture(ApertureValue(100), &[]), ApertureIndex(1));
}

#[test]
fn lens_value_conversion_retains_only_the_low_halfword() {
    assert_eq!(f_72ccac(-1), 65535);
    assert_eq!(f_72ccac(0x12345678), 0x5678);
    assert_eq!(f_72ccac(-32768), 32768);
}

#[test]
fn shutter_encoding_preserves_alias_sentinel_and_unknown_behavior() {
    assert_eq!(shutter_code(ShutterValue(-3000)), 13);
    assert_eq!(shutter_code(ShutterValue(-100)), 28);
    assert_eq!(shutter_code(ShutterValue(100)), 28);
    assert_eq!(shutter_code(ShutterValue(130)), 29);
    assert_eq!(shutter_code(ShutterValue(400000)), 64);
    assert_eq!(shutter_code(ShutterValue(-1)), -1);
    assert_eq!(shutter_code(ShutterValue(0)), 0);
    assert_eq!(shutter_code(ShutterValue(131)), 0);
}

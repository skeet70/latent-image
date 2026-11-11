extern crate std;
use std::vec;
use std::vec::Vec;
use super::{GeometryContext, PositionGeometry};
use crate::af_motion_runtime::{FocusDelta, FocusPosition, PositionSelection, ZoomPosition};
use xm1_calibration_core::{Calibration, CurveIndex};
use xm1_calibration_core::configuration::{ConfigAddress, Configuration, ConfigurationError, ConfigurationRegion, CONFIGURATION_RAM_SIZE};

fn half(bytes: &mut [u8], key: usize, value: i16) { bytes[key..key + 2].copy_from_slice(&value.to_le_bytes()); }
fn fixture() -> Vec<u8> {
    let mut bytes = vec![0; CONFIGURATION_RAM_SIZE];
    bytes[0xa8] = 0x20;
    bytes[0x3e0f82] = 2;
    bytes[0x3e1213] = 1;
    bytes[0x3e1214] = 1;
    bytes[0x3e0313] = 1;
    for (key, value) in [(0x3e0317,100),(0x3e0572,200),(0x3e063b,400),
        (0x3e0704,300),(0x3e07cd,600),(0x3e07f7,150),(0x3e08c0,350),
        (0x3e0b40,11),(0x3e0c09,13),(0x3e0cd2,17),(0x3e0d9b,19),
        (0x3e2cf4,50),(0x3e2dbd,60),(0x3e2e86,70),
        (0x3e2800,40),(0x3e2992,80),(0x3e2b24,120),
        (0x3e28c9,-40),(0x3e2a5b,-80),(0x3e2bed,-120)] { half(&mut bytes,key,value); }
    for key in [0x3e2cb5 + 27,0x3e2cd4 + 25,0x3e2f6d,0x3e2fab,0x3e2fe9] { bytes[key] = 255; }
    bytes
}
fn calibration(bytes: &[u8]) -> Calibration<'_> {
    Calibration::new(Configuration::new(ConfigurationRegion::new(ConfigAddress(0), bytes).unwrap(), ConfigAddress(0)))
}
fn target(bytes: &[u8], selection: i32, context: GeometryContext, current: i16) -> Result<FocusDelta, ConfigurationError<core::convert::Infallible>> {
    PositionGeometry::new(&mut calibration(bytes)).target_delta(FocusDelta(1234), FocusPosition(current), ZoomPosition(0), PositionSelection(selection), context)
}

#[test]
fn disabled_override_preserves_the_entire_requested_delta_without_profile_data() {
    let mut bytes = [0; 0xa9];
    for flags in [0, 0xdf] {
        bytes[0xa8] = flags;
        let mut calibration = calibration(&bytes);
        let mut geometry = PositionGeometry::new(&mut calibration);
        for delta in [i16::MIN, -1, 0, i16::MAX] {
            assert_eq!(geometry.target_delta(FocusDelta(delta), FocusPosition(i16::MAX), ZoomPosition(-1), PositionSelection(-1), GeometryContext::Default), Ok(FocusDelta(delta)));
        }
    }
}

#[test]
fn every_target_selector_uses_its_profile_bound_or_midpoint() {
    let mut bytes = fixture();
    for (mode, position) in [(0,89),(1,100),(2,250),(3,300),(4,313),
        (0x10,383),(0x11,400),(0x12,450),(0x13,500),(0x14,519),(0xff,89)] {
        bytes[0xbd] = mode;
        assert_eq!(target(&bytes,-1,GeometryContext::Default,7), Ok(FocusDelta(position - 7)));
    }
}

#[test]
fn all_other_selector_bytes_use_the_primary_expanded_lower_bound() {
    let mut bytes = fixture();
    for mode in 0..=u8::MAX {
        if matches!(mode,1..=4 | 0x10..=0x14) { continue; }
        bytes[0xbd] = mode;
        assert_eq!(target(&bytes,-1,GeometryContext::Default,0), Ok(FocusDelta(89)));
    }
}

#[test]
fn context_changes_both_bank_queries() {
    let mut bytes = fixture();
    for (context, lower, upper) in [(GeometryContext::Default,400,500),(GeometryContext::Enabled,100,500),
        (GeometryContext::ModeOne,100,300),(GeometryContext::ModeTwo,300,500)] {
        bytes[0xbd] = 0x11;
        assert_eq!(target(&bytes,-1,context,0),Ok(FocusDelta(lower)));
        bytes[0xbd] = 0x13;
        assert_eq!(target(&bytes,-1,context,0),Ok(FocusDelta(upper)));
    }
    assert_eq!(GeometryContext::from_settings(false,1),GeometryContext::Default);
    assert_eq!(GeometryContext::from_settings(true,1),GeometryContext::ModeOne);
    assert_eq!(GeometryContext::from_settings(true,2),GeometryContext::ModeTwo);
    assert_eq!(GeometryContext::from_settings(true,255),GeometryContext::Enabled);
}

#[test]
fn selected_profiles_interpolate_signed_corrections_and_limit_expansion() {
    let mut bytes = fixture();
    bytes[0xbd] = 2;
    assert_eq!(target(&bytes,52,GeometryContext::Default,0),Ok(FocusDelta(320)));
    assert_eq!(target(&bytes,0,GeometryContext::Default,0),Ok(FocusDelta(180)));
    assert_eq!(target(&bytes,25,GeometryContext::Default,0),Ok(FocusDelta(250)));
    bytes[0xbd] = 3;
    assert_eq!(target(&bytes,52,GeometryContext::Default,0),Ok(FocusDelta(313)));
    bytes[0xbd] = 4;
    assert_eq!(target(&bytes,25,GeometryContext::Default,0),Ok(FocusDelta(313)));
    bytes[0xbd] = 0;
    assert_eq!(target(&bytes,25,GeometryContext::Default,0),Ok(FocusDelta(89)));
}

#[test]
fn unavailable_extended_channel_uses_primary_anchors() {
    let mut bytes = fixture();
    bytes[0x3e1213] = 0;
    bytes[0xbd] = 0x11;
    assert_eq!(target(&bytes,-1,GeometryContext::Default,0),Ok(FocusDelta(100)));
    bytes[0xbd] = 0x13;
    assert_eq!(target(&bytes,-1,GeometryContext::Default,0),Ok(FocusDelta(300)));
}

#[test]
fn delta_subtraction_keeps_signed_halfword_wrapping() {
    let mut bytes = fixture();
    bytes[0xbd] = 1;
    assert_eq!(target(&bytes,-1,GeometryContext::Default,i16::MIN),Ok(FocusDelta(-32668)));
}

#[test]
fn missing_enabled_calibration_is_reported_as_configuration_error() {
    let mut bytes = [0; 0xbe];
    bytes[0xa8] = 0x20;
    assert!(matches!(target(&bytes,-1,GeometryContext::Default,0),Err(ConfigurationError::OutOfBounds { .. })));
}

#[test]
fn iris_encoding_uses_canonical_index_collapse_for_both_banks_and_invalid_indices() {
    let bytes = fixture();
    let mut calibration = calibration(&bytes);
    let mut geometry = PositionGeometry::new(&mut calibration);
    for (index, encoded) in [(0,0),(1,64),(2,0),(30,0),(31,0),(32,64),(33,0),(u32::MAX,0)] {
        assert_eq!(geometry.iris_position(CurveIndex(index)),Ok(encoded));
    }
}

#[test]
fn selection_uses_low_signed_byte_clamping_and_rejects_the_top_two_bits() {
    let mut bytes = fixture();
    bytes[0xbd] = 2;
    bytes[0x3e2cb5 + 30] = 240;
    bytes[0x3e2cd4 + 30] = 240;
    for (selection, expected) in [(0x134,320),(55,320),(56,320),(127,320),
        (128,180),(251,180),(0x4000_0034,250),(i32::MIN,250)] {
        assert_eq!(target(&bytes,selection,GeometryContext::Default,0),Ok(FocusDelta(expected)));
    }
}

#[test]
fn midpoint_above_the_far_anchor_uses_extended_segment_and_its_availability() {
    let mut bytes = fixture();
    bytes[0xbd] = 2;
    half(&mut bytes,0x3e07f7,550);
    assert_eq!(target(&bytes,52,GeometryContext::Default,0),Ok(FocusDelta(770)));
    bytes[0x3e0313] = 0;
    assert_eq!(target(&bytes,52,GeometryContext::Default,0),Ok(FocusDelta(800)));
}

#[test]
fn signed_margin_values_are_not_reinterpreted_as_unsigned_lengths() {
    let mut bytes = fixture();
    half(&mut bytes,0x3e0b40,-11);
    half(&mut bytes,0x3e0c09,-13);
    bytes[0xbd] = 0;
    assert_eq!(target(&bytes,-1,GeometryContext::Default,0),Ok(FocusDelta(111)));
    bytes[0xbd] = 4;
    assert_eq!(target(&bytes,-1,GeometryContext::Default,0),Ok(FocusDelta(287)));
}

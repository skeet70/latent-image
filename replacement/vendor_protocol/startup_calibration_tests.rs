extern crate std;
use std::{vec, vec::Vec};
use super::{StartupCalibration, StartupCalibrationError, StartupCalibrationMode, SAMPLE_CACHE_BYTES, EXTREMA_CACHE_BYTES};
use crate::backend::{ConfigImages, ConfigReadMode, ConfigSpan};
use crate::metadata_calibration::MetadataCalibration;
use xm1_calibration_core::configuration::{ConfigAddress, Configuration, ConfigurationRegion};

fn half(bytes: &mut [u8], key: usize, value: u16) { bytes[key..key + 2].copy_from_slice(&value.to_le_bytes()); }
fn calibration(bytes: &[u8]) -> MetadataCalibration<'_> {
    let region = ConfigurationRegion::new(ConfigAddress(0), bytes).unwrap();
    MetadataCalibration::new(Configuration::new(region, ConfigAddress(0)))
}
fn configuration() -> Vec<u8> {
    let mut bytes = vec![0; 0x420000];
    bytes[0x3e0f80] = 1;
    bytes[0x3e0f82] = 1;
    bytes[0x9524] = 2;
    half(&mut bytes, 0x3e2643, 100);
    for index in 1..20 { half(&mut bytes, 0x3eb100 + (index - 1) * 2, (32768 / index) as u16); }
    bytes
}
fn cache_word(cache: &[u8], index: usize) -> u16 { u16::from_le_bytes([cache[index * 2], cache[index * 2 + 1]]) }

#[test]
fn reverse_sample_queries_keep_their_original_cache_positions_and_unused_channels() {
    let mut bytes = configuration();
    half(&mut bytes, 0x9526, 1);
    half(&mut bytes, 0x9528, 43690);
    let mut state = StartupCalibration::new();
    state.prepare(&mut calibration(&bytes), StartupCalibrationMode::Recompute).unwrap();
    assert_eq!(state.samples()[0], 1);
    assert_eq!(cache_word(state.sample_cache(), 0), 1900);
    assert!((49..=50).contains(&cache_word(state.sample_cache(), 1)));
    assert_eq!(cache_word(state.sample_cache(), 100), 0);
    assert!(state.sample_cache()[202..].iter().all(|byte| *byte == 0xff));
    assert_eq!(state.extrema_cache(), &[0; EXTREMA_CACHE_BYTES]);
}

#[test]
fn recomputed_tables_are_written_to_both_actual_configuration_cache_ranges() {
    let mut bytes = configuration();
    half(&mut bytes, 0x9526, 1);
    let mut state = StartupCalibration::new();
    state.prepare(&mut calibration(&bytes), StartupCalibrationMode::Recompute).unwrap();
    let mut images = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    state.persist(&mut images).unwrap();
    let mut samples = vec![0; SAMPLE_CACHE_BYTES];
    let mut extrema = [0xff; EXTREMA_CACHE_BYTES];
    images.read(ConfigSpan::new(0x40ab00, SAMPLE_CACHE_BYTES as u32).unwrap(), ConfigReadMode::Encoded, &mut samples).unwrap();
    images.read(ConfigSpan::new(0x40fa00, EXTREMA_CACHE_BYTES as u32).unwrap(), ConfigReadMode::Encoded, &mut extrema).unwrap();
    assert_eq!(samples.as_slice(), &state.sample_cache()[..]);
    assert_eq!(&extrema, state.extrema_cache());
    assert_eq!(images.byte(0x40aaff), Some(0));
    assert_eq!(images.byte(0x40f9e8), Some(0));
}

#[test]
fn reload_uses_stored_cache_bytes_and_current_samples_without_reinterpolation() {
    let mut bytes = configuration();
    bytes[0x40ab00..0x40ab00 + SAMPLE_CACHE_BYTES].fill(0x5a);
    bytes[0x40fa00..0x40fa00 + EXTREMA_CACHE_BYTES].fill(0xa5);
    half(&mut bytes, 0x9526 + 200, 0xbeef);
    bytes[0x9524] = 255;
    let mut state = StartupCalibration::new();
    state.prepare(&mut calibration(&bytes), StartupCalibrationMode::Reload).unwrap();
    assert_eq!(state.samples()[100], 0xbeef);
    assert_eq!(state.sample_cache(), &[0x5a; SAMPLE_CACHE_BYTES]);
    assert_eq!(state.extrema_cache(), &[0xa5; EXTREMA_CACHE_BYTES]);
}

#[test]
fn invalid_window_and_descending_knots_do_not_publish_cache_writes() {
    let mut bytes = configuration();
    bytes[0x9524] = 21;
    let mut state = StartupCalibration::new();
    assert!(matches!(state.prepare(&mut calibration(&bytes), StartupCalibrationMode::Recompute), Err(StartupCalibrationError::WindowSize { knots: 21 })));
    bytes[0x9524] = 2;
    half(&mut bytes, 0x3eb100, 65535);
    assert!(matches!(state.prepare(&mut calibration(&bytes), StartupCalibrationMode::Recompute), Err(StartupCalibrationError::InvalidKnotOrder { .. })));
    let mut images = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    state.persist(&mut images).unwrap();
    assert_eq!(images.byte(0x40ab00), Some(0));
    assert_eq!(images.byte(0x40fa00), Some(0));
}

#[test]
fn disabled_calibration_retains_runtime_samples_and_caches() {
    let mut bytes = configuration();
    let mut state = StartupCalibration::new();
    state.prepare(&mut calibration(&bytes), StartupCalibrationMode::Recompute).unwrap();
    let cached = *state.sample_cache();
    bytes[0x3e0009] = 1;
    half(&mut bytes, 0x9526, 400);
    state.prepare(&mut calibration(&bytes), StartupCalibrationMode::Recompute).unwrap();
    assert_eq!(state.samples()[0], 0);
    assert_eq!(state.sample_cache(), &cached);
    let mut images = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    state.persist(&mut images).unwrap();
    assert_eq!(images.byte(0x40ab00), Some(0));
}

#[test]
fn extrema_store_first_minimum_and_the_split_with_largest_remapped_limit() {
    let mut bytes = configuration();
    bytes[0x3e0f82] = 2;
    bytes[0x3e270c] = 10;
    for knot in 0..7 { bytes[0x3ec4af + knot * 9 + 1] = 1; }
    for knot in 0..6 { half(&mut bytes, 0x3ec000 + knot * 2, (knot as u16 + 1) * 10); }
    half(&mut bytes, 0x3e0572, 50);
    let mut state = StartupCalibration::new();
    state.prepare(&mut calibration(&bytes), StartupCalibrationMode::Recompute).unwrap();
    assert_eq!(&state.extrema_cache()[..6], &[0, 1, 0, 1, 0, 1]);
    assert!(state.extrema_cache()[6..].iter().all(|byte| *byte == 0));
}

#[test]
fn sorted_keys_deduplicate_both_curve_banks_and_include_the_unmapped_default() {
    let mut bytes = configuration();
    bytes[0x3e0f82] = 3;
    bytes[0x3e22c8] = 0;
    bytes[0x3e232d] = 33;
    bytes[0x3f4600..0x3f4603].copy_from_slice(&[0, 1, 0xff]);
    half(&mut bytes, 0x3e2530, 300);
    half(&mut bytes, 0x3e2532, 20);
    let mut state = StartupCalibration::new();
    state.rebuild_keys(&mut calibration(&bytes)).unwrap();
    assert_eq!(state.calibration_keys(), &[20, 100, 300]);
}

#[test]
fn sorted_key_capacity_stops_admitting_values_after_128_entries() {
    let mut bytes = configuration();
    bytes[0x3e0f80] = 5;
    bytes[0x3e0f82] = 31;
    for channel in 0..5 {
        bytes[0x3e22c8 + channel] = 0;
        bytes[0x3e232d + channel] = 30;
        for index in 0..31 { bytes[0x3f4600 + channel * 31 + index] = (channel * 31 + index) as u8; }
    }
    for index in 0..155 { half(&mut bytes, 0x3e2530 + index * 2, (500 - index) as u16); }
    let mut state = StartupCalibration::new();
    state.rebuild_keys(&mut calibration(&bytes)).unwrap();
    assert_eq!(state.calibration_keys().len(), 128);
    assert_eq!(state.calibration_keys()[0], 373);
    assert_eq!(state.calibration_keys()[127], 500);
}

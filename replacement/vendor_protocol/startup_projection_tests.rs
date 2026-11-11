extern crate std;
use std::{vec, vec::Vec};
use super::{StartupProjection, StartupProjectionError};
use crate::backend::{ConfigImages, ConfigSpan};
use crate::startup_calibration::{StartupCalibrationError, SAMPLE_CACHE_BYTES, EXTREMA_CACHE_BYTES};
use crate::startup_output_cache::ChannelLookup;

fn half(bytes: &mut [u8], key: usize, value: u16) { bytes[key..key + 2].copy_from_slice(&value.to_le_bytes()); }
fn fixture() -> Vec<u8> {
    let mut bytes = vec![0; 0x420000];
    bytes[0x3e0f80] = 1;
    bytes[0x3e0f82] = 1;
    bytes[0x9524] = 2;
    half(&mut bytes, 0x3e2643, 100);
    for index in 1..20 { half(&mut bytes, 0x3eb100 + (index - 1) * 2, (32768 / index) as u16); }
    bytes
}

#[test]
fn reload_updates_output_and_invalidates_lazy_lookup_without_running_unregistered_workers() {
    let mut bytes = fixture();
    bytes[0x40ab00..0x40ab00 + SAMPLE_CACHE_BYTES].fill(0x5a);
    bytes[0x40fa00..0x40fa00 + EXTREMA_CACHE_BYTES].fill(0xa5);
    bytes[0x3e3dd0..0x3e3dd8].fill(7);
    bytes[0x31d80..0x31dc0].fill(0x55);
    bytes[0x404e00] = 0x77;
    bytes[0x3e44ed] = 11;
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let mut state = StartupProjection::new();
    assert_eq!(state.lookup().channel(&configuration, ChannelLookup::from_raw(0)), Ok(11));
    configuration.write(ConfigSpan::new(0x3e44ed, 1).unwrap(), &[22]).unwrap();
    state.prepare_output(&mut configuration, false).unwrap();
    assert_eq!(state.calibration().sample_cache(), &[0x5a; SAMPLE_CACHE_BYTES]);
    assert_eq!(state.calibration().extrema_cache(), &[0xa5; EXTREMA_CACHE_BYTES]);
    for index in 0..28 {
        assert_eq!(configuration.byte(0x408a00 + index * 2), Some(14));
        assert_eq!(configuration.byte(0x408a01 + index * 2), Some(0));
    }
    assert_eq!(state.lookup().channel(&configuration, ChannelLookup::from_raw(0)), Ok(22));
    assert_eq!(configuration.byte(0x31d80), Some(0x55));
    assert_eq!(configuration.byte(0x404e00), Some(0x77));
}

#[test]
fn recompute_persists_current_calibration_and_initializes_lookup_and_reciprocals() {
    let mut bytes = fixture();
    half(&mut bytes, 0x9526, 1);
    bytes[0x40ab00..0x40ab00 + SAMPLE_CACHE_BYTES].fill(0xa5);
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let mut state = StartupProjection::new();
    state.prepare_output(&mut configuration, true).unwrap();
    assert_eq!(state.calibration().samples()[0], 1);
    assert_eq!(configuration.byte(0x40ab00), Some(0x6c));
    assert_eq!(configuration.byte(0x40ab01), Some(7));
    assert_eq!(configuration.byte(0x40ab00 + 202), Some(0xff));
    assert_eq!(configuration.byte(0x40fa00), Some(0));
    assert_eq!(configuration.byte(0x31d80), Some(8));
    assert_eq!(configuration.byte(0x404e00), Some(0));
    assert_eq!(configuration.byte(0x404e01), Some(8));
    assert_eq!(configuration.byte(0x409cbe), Some(0));
    assert_eq!(configuration.byte(0x409cbf), Some(8));
}

#[test]
fn invalid_calibration_does_not_publish_later_output_caches() {
    let mut bytes = fixture();
    bytes[0x9524] = 21;
    bytes[0x408a00] = 0x5a;
    bytes[0x31d80] = 0xa5;
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let mut state = StartupProjection::new();
    assert_eq!(state.prepare_output(&mut configuration, true), Err(StartupProjectionError::Calibration(StartupCalibrationError::WindowSize { knots: 21 })));
    assert_eq!(configuration.byte(0x408a00), Some(0x5a));
    assert_eq!(configuration.byte(0x31d80), Some(0xa5));
}

#[test]
fn publication_rebuilds_sorted_unique_keys_from_the_current_loaded_configuration() {
    let mut bytes = fixture();
    bytes[0x3e0f82] = 3;
    bytes[0x3e232d] = 2;
    bytes[0x3f4600..0x3f4603].copy_from_slice(&[0, 1, 0xff]);
    half(&mut bytes, 0x3e2530, 300);
    half(&mut bytes, 0x3e2532, 20);
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let mut state = StartupProjection::new();
    state.publish_configuration(&configuration).unwrap();
    assert_eq!(state.calibration().calibration_keys(), &[20, 100, 300]);
    configuration.write(ConfigSpan::new(0x3e2530, 2).unwrap(), &20u16.to_le_bytes()).unwrap();
    state.publish_configuration(&configuration).unwrap();
    assert_eq!(state.calibration().calibration_keys(), &[20, 100]);
}

#[test]
fn registered_reload_publishes_all_six_radial_records_in_descending_source_order() {
    let mut bytes = fixture();
    bytes[0x3e0009] = 1;
    bytes[0x18f47] = 1;
    bytes[0x404aff] = 0xa5;
    bytes[0x404d46] = 0x5a;
    for (record, source) in [0x3420, 0x3410, 0x3400, 0x33f0, 0x33e0, 0x33d0].into_iter().enumerate() {
        for sample in 0..8 { half(&mut bytes, source + sample * 2, record as u16 * 16); }
    }
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let mut state = StartupProjection::new();
    state.prepare_output(&mut configuration, false).unwrap();
    for record in 0..6 {
        let key = 0x404b00 + record * 0x61;
        assert_eq!(configuration.byte(key), Some(0));
        assert_eq!(configuration.byte(key + 1), Some(0));
        assert_eq!(configuration.byte(key + 2), Some(1));
        assert_eq!(configuration.byte(key + 9), Some(record as u8));
        assert_eq!(configuration.byte(key + 10), Some(0));
        assert_eq!(configuration.byte(key + 11), Some(1));
    }
    assert_eq!(configuration.byte(0x404aff), Some(0xa5));
    assert_eq!(configuration.byte(0x404d46), Some(0x5a));
}

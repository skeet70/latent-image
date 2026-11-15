extern crate std;
use std::vec;
use super::{Calibration, configuration::{CfgKey, ConfigAddress, Configuration, ConfigurationError, ConfigurationRegion}, profile::*};

struct Data;
impl Tables for Data {
    fn split_keys(&self, _: SplitTableIndex) -> [CfgKey; 2] { [CfgKey(0x100), CfgKey(0x102)] }
    fn history_fraction(&self, _: HistoryIndex) -> u8 { 100 }
}
fn half(bytes: &mut [u8], key: usize, value: u16) { bytes[key..key + 2].copy_from_slice(&value.to_le_bytes()); }
fn reader(bytes: &[u8]) -> Calibration<'_> { Calibration::new(Configuration::new(ConfigurationRegion::new(ConfigAddress(0), bytes).unwrap(), ConfigAddress(0))) }

#[test]
fn enabled_factors_and_disabled_full_word_modes_produce_literal_triplets() {
    let mut bytes = vec![0; 0x3e0f83];
    bytes[0x3e0f82] = 2;
    for (key, value) in [(0x100,512),(0x102,2048),(0x7ed7,2048),(0x7ed9,512),(0x80d1,2048),(0x80d3,512)] { half(&mut bytes, key, value); }
    let mut calibration = reader(&bytes);
    let mut input = FactorRequest { mode: FactorMode::from_native(2), split: SplitIndex(0x101), selector: ProfileSelector(1), deltas: [DeltaIndex(0), DeltaIndex(10)] };
    let mut values = Triplet([1000,2000,3000]);
    calibration.calibrate_profile(&input, &Data, &mut values).unwrap();
    assert_eq!(values, Triplet([1000,4000,1500]));
    input.mode = FactorMode::from_native(0x100);
    input.selector = ProfileSelector(i32::MIN);
    values = Triplet([65535,32768,1]);
    calibration.calibrate_profile(&input, &Data, &mut values).unwrap();
    assert_eq!(values, Triplet([65535,32768,1]));
}

#[test]
fn clamping_keeps_wrapped_signed_products_and_full_class_flag_shifts() {
    let mut bytes = vec![0; 0x8000];
    for (key, value) in [(0x7930,65535),(0x7934,65535),(0x792a,65535),(0x7938,65535)] { half(&mut bytes, key, value); }
    let mut calibration = reader(&bytes);
    let mut values = Triplet([1,65535,65535]);
    let flags = calibration.clamp_profile(ClampClass(0), &mut values, ProfileRate(10)).unwrap();
    assert_eq!(values, Triplet([32767,1,1]));
    assert_eq!(flags, ClampFlags { upper:1, lower:1 });
    values = Triplet([0,40000,0]);
    let flags = calibration.clamp_profile(ClampClass(86), &mut values, ProfileRate(u32::MAX)).unwrap();
    assert_eq!(values, Triplet([256,32767,256]));
    assert_eq!(flags, ClampFlags { upper:8, lower:20 });
}

#[test]
fn positive_exposure_keeps_full_word_shift_boundaries_and_nonpositive_values() {
    for (exposure, expected) in [(-65536,[1000,2000,3000]),(0,[1000,2000,3000]),(100,[2000,4000,6000]),(3200,[0,0,0])] {
        let mut values = Triplet([1000,2000,3000]);
        apply_positive(ExposureHundredths(exposure), &mut values, &Data);
        assert_eq!(values, Triplet(expected));
    }
}

#[test]
fn missing_geometry_configuration_is_an_error_without_mutating_values() {
    let mut calibration = reader(&[]);
    let mut values = Triplet([1,2,3]);
    assert_eq!(calibration.clamp_geometry(&mut values, ProfileRate(0), ExposureHundredths(-32768)), Err(ConfigurationError::OutOfBounds { address:ConfigAddress(0x792e), length:1 }));
    assert_eq!(values, Triplet([1,2,3]));
}

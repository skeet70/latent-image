use super::commands::{ByteCount, DebugAddress, ProviderError};
use super::metadata::{DEVICE_INFO_LENGTH, DeviceInfoSnapshot, LensPosition, MetadataError, ProfileRecord, PublicationMode, prepare_device_info, produce_calibration};
use super::metadata_calibration::{LimitBank, LimitKind, LimitSide, MetadataCalibration, ModeClass};
use super::metadata_selection::{CalibrationChannel, LensSelection, adjust_selection};
use xm1_calibration_core::{BiasBank, ChannelIndex, CurveIndex};
use xm1_calibration_core::configuration::{ConfigAddress, Configuration, ConfigurationRegion};
use std::vec;

fn configuration(bytes: &[u8]) -> Configuration<'_> {
    let region = ConfigurationRegion::new(ConfigAddress(0x100000), bytes).unwrap();
    Configuration::new(region, ConfigAddress(0x100000))
}
fn set_half(bytes: &mut [u8], key: usize, value: u16) { bytes[key..key + 2].copy_from_slice(&value.to_le_bytes()); }
fn set_word(bytes: &mut [u8], key: usize, value: u32) { bytes[key..key + 4].copy_from_slice(&value.to_le_bytes()); }
fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) }

#[test]
fn mapped_values_collapse_the_second_bank_and_default_only_on_ff() {
    let mut bytes = vec![0; 0x3f4800];
    bytes[0x3e0f82] = 2;
    bytes[0x3f4600] = 3;
    bytes[0x3f4601] = 0xff;
    set_half(&mut bytes, 0x3e2536, 400);
    let mut calibration = MetadataCalibration::new(configuration(&bytes));
    assert_eq!(calibration.mapped_value(CurveIndex(0), ChannelIndex(0)).unwrap().0, 400);
    assert_eq!(calibration.mapped_value(CurveIndex(31), ChannelIndex(0)).unwrap().0, 400);
    assert_eq!(calibration.mapped_value(CurveIndex(1), ChannelIndex(0)).unwrap().0, 100);
    assert_eq!(calibration.mapped_value(CurveIndex(30), ChannelIndex(0)).unwrap().0, 400);
    assert_eq!(calibration.mapped_value(CurveIndex(0), ChannelIndex(-1)).unwrap().0, 0);
}

#[test]
fn mode_limits_preserve_full_words_shift_width_and_zero_sentinels() {
    let mut bytes = vec![0; 0x3f4800];
    set_word(&mut bytes, 0x3e166c, 0x76543210);
    set_word(&mut bytes, 0x3e145a, 0xfedcba98);
    set_half(&mut bytes, 0x3e1676, 0x8001);
    bytes[0x3e162f] = 1;
    let mut calibration = MetadataCalibration::new(configuration(&bytes));
    assert_eq!(calibration.mode_limit(LimitKind::OptionalPacked, LimitBank::First, LimitSide::Other, ModeClass::Zero), Ok(0x76543210));
    assert_eq!(calibration.mode_limit(LimitKind::Packed, LimitBank::First, LimitSide::One, ModeClass::Three), Ok(0xfedcba98));
    assert_eq!(calibration.mode_limit(LimitKind::OptionalScaled, LimitBank::First, LimitSide::Other, ModeClass::Zero), Ok(0x10002));
    assert_eq!(calibration.mode_limit(LimitKind::OptionalWord, LimitBank::Second, LimitSide::Other, ModeClass::Two), Ok(0xffff));
    bytes[0x3e162f] = 32;
    let mut calibration = MetadataCalibration::new(configuration(&bytes));
    assert_eq!(calibration.mode_limit(LimitKind::OptionalScaled, LimitBank::First, LimitSide::Other, ModeClass::Zero), Ok(0xffff));
    assert_eq!(calibration.mode_limit(LimitKind::Scaled, LimitBank::First, LimitSide::Other, ModeClass::Zero), Ok(0));
}

#[test]
fn selection_interpolates_all_three_segments_and_preserves_invalid_or_centered_values() {
    let mut bytes = vec![0; 0x3f4800];
    bytes[0x3e1213] = 1;
    bytes[0x3e1214] = 1;
    bytes[0x3e0313] = 1;
    set_half(&mut bytes, 0x3e0572, 100);
    set_half(&mut bytes, 0x3e063b, 200);
    set_half(&mut bytes, 0x3e07cd, 300);
    for (key, value) in [(0x3e2800, 24), (0x3e2992, 48), (0x3e2b24, 72)] { set_half(&mut bytes, key, value); }
    bytes[0x3e2cb6] = 120;
    let mut calibration = MetadataCalibration::new(configuration(&bytes));
    let channel = CalibrationChannel::new(0).unwrap();
    for (input, expected) in [(50, 68), (150, 180), (250, 286)] {
        assert_eq!(adjust_selection(calibration.profile(), channel, BiasBank::Primary, LensSelection(26), input), Ok(expected));
    }
    for selection in [25, 0x40000000, 0x80000000] {
        assert_eq!(adjust_selection(calibration.profile(), channel, BiasBank::Primary, LensSelection(selection), -123), Ok(-123));
    }
}

#[test]
fn device_info_publishes_real_calibration_bytes_without_zeroing_other_fields() {
    let mut bytes = vec![0; 0x3f4800];
    bytes[0x3e0f82] = 2;
    bytes[0x3e0f80] = 1;
    bytes[0x3f4600] = 1;
    bytes[0x3f4601] = 0xff;
    set_half(&mut bytes, 0x3e2532, 200);
    set_half(&mut bytes, 0x3f2400, 5);
    set_half(&mut bytes, 0x3f2402, (-15i16) as u16);
    set_half(&mut bytes, 0x9285, 50);
    set_half(&mut bytes, 0x3e0317, (-10i16) as u16);
    set_half(&mut bytes, 0x3e04a9, 4);
    set_half(&mut bytes, 0x3e0572, 20);
    set_half(&mut bytes, 0x3f3d14, (-123i16) as u16);
    let mut calibration = MetadataCalibration::new(configuration(&bytes));
    let mut output = vec![0xa5; DEVICE_INFO_LENGTH.0 as usize + 8];
    let mut record = [0; 0x2158];
    record[0x46] = 7;
    let snapshot = DeviceInfoSnapshot { profile: ProfileRecord::new(&record), channel: CalibrationChannel::new(0).unwrap(), lens_selection: LensSelection(25), lens_position: LensPosition(-42), mode: PublicationMode::Ordinary };
    let prepared = prepare_device_info(&mut calibration, &mut output, DebugAddress(0x10000000), &snapshot).unwrap();
    assert_eq!(prepared.length(), ByteCount(0xe297c));
    assert_eq!(prepared.address(), 0x10000000);
    let published = prepared.bytes();
    assert_eq!(&published[..4], b"MCTS");
    assert_eq!(&published[0xb174c..0xb1750], b"ELTS");
    assert_eq!(&published[0xb19f0..0xb19f4], b"ELAL");
    assert_eq!(&published[0xb1750..0xb1756], &[1, 0, 0xfe, 0xff, 0xfd, 0xfd]);
    assert_eq!(&published[0xb1790..0xb1796], &[200, 0, 100, 0, 0xfd, 0xfd]);
    assert_eq!(&published[0xb17d8..0xb17de], &[200, 0, 0, 0, 0xfd, 0xfd]);
    assert_eq!(&published[0xb1818..0xb181e], &[100, 0, 99, 0, 0xfd, 0xfd]);
    assert_eq!(&published[0xb18bc..0xb18c2], &[100, 0, 99, 0, 0xfd, 0xfd]);
    assert_eq!(word(published, 0xb17d0), (-123i32) as u32);
    assert_eq!(word(published, 0xb19cc), (-6i32) as u32);
    assert_eq!(word(published, 0xb19d0), 10);
    assert_eq!(word(published, 0x60), 7);
    assert_eq!(word(published, 0x7c), (-42i32) as u32);
    assert_eq!(word(published, 0x116cc), 0xa5a5a5a5);
    assert_eq!(word(published, 0xb18fc), 0xffff);
    assert_eq!(word(published, 0xb1974), 0);
    for offset in [4, 0x114, 0xb1756, 0xb17ce, 0xb18f8, 0xe2978] {
        assert_eq!(published[offset], 0xa5);
    }
    assert_eq!(&output[DEVICE_INFO_LENGTH.0 as usize..], &[0xa5; 8]);
}

#[test]
fn absent_bypassed_and_short_publications_leave_destination_unchanged() {
    let bytes = [0u8; 1];
    let mut calibration = MetadataCalibration::new(configuration(&bytes));
    let record = [0; 0x2158];
    let mut snapshot = DeviceInfoSnapshot { profile: ProfileRecord::new(&record), channel: CalibrationChannel::new(0).unwrap(), lens_selection: LensSelection(25), lens_position: LensPosition(0), mode: PublicationMode::Bypass };
    let mut output = [0xa5; 16];
    assert!(matches!(prepare_device_info(&mut calibration, &mut output, DebugAddress(1), &snapshot), Err(MetadataError::Bypassed)));
    snapshot.mode = PublicationMode::Ordinary;
    assert!(matches!(prepare_device_info(&mut calibration, &mut output, DebugAddress(1), &snapshot), Err(MetadataError::DestinationTooSmall { .. })));
    assert_eq!(output, [0xa5; 16]);
    assert!(matches!(prepare_device_info(&mut calibration, &mut output, DebugAddress(0), &snapshot), Err(MetadataError::Publication(ProviderError::Unavailable))));
    assert!(matches!(produce_calibration(&mut calibration, &mut output, snapshot.channel, snapshot.lens_selection), Err(MetadataError::DestinationTooSmall { .. })));
}

#[test]
fn profile_selection_resolves_live_generation_category_and_record_fallback() {
    use super::metadata::{ProfileHandle, ProfileRecordIndex, ProfileSelectionState};
    let mut records = [[0; 0x2158]; 2];
    records[0][0x47] = 3;
    records[1][0x47] = 9;
    let mut state = [0; 0x390];
    set_word(&mut state, 8, 1);
    set_word(&mut state, 56 + 12, 7);
    set_word(&mut state, 56 + 20, 0);
    state[56 + 24] = 1;
    let selection = ProfileSelectionState::new(&state);
    let handle = ProfileHandle((7 << 4) | 1);
    assert_eq!(selection.record_index(handle), ProfileRecordIndex(0));
    assert_eq!(ProfileRecord::resolve(&records, &selection, handle).unwrap().first_kind(), 3);
    for handle in [ProfileHandle(u32::MAX), ProfileHandle((6 << 4) | 1), ProfileHandle((7 << 4) | 1 | (1 << 18))] {
        assert_eq!(ProfileRecord::resolve(&records, &selection, handle).unwrap().first_kind(), 9);
    }
    assert_eq!(selection.record_index(ProfileHandle(handle.0 | (63 << 18))), ProfileRecordIndex(0));
    set_word(&mut state, 56 + 20, 2);
    assert_eq!(ProfileSelectionState::new(&state).record_index(handle), ProfileRecordIndex(1));
    set_word(&mut state, 8, 2);
    assert!(matches!(ProfileRecord::resolve(&records, &ProfileSelectionState::new(&state), handle), Err(MetadataError::ProfileRecordUnavailable { index: ProfileRecordIndex(2) })));
}

#[test]
fn channels_above_nine_clear_only_optional_triples_and_nonordinary_header_clear() {
    let bytes = vec![0; 0x3f4800];
    let mut calibration = MetadataCalibration::new(configuration(&bytes));
    let record = [0; 0x2158];
    let snapshot = DeviceInfoSnapshot { profile: ProfileRecord::new(&record), channel: CalibrationChannel::new(10).unwrap(), lens_selection: LensSelection(25), lens_position: LensPosition(0), mode: PublicationMode::Other };
    let mut output = vec![0xa5; DEVICE_INFO_LENGTH.0 as usize];
    let prepared = prepare_device_info(&mut calibration, &mut output, DebugAddress(1), &snapshot).unwrap();
    for offset in [0xb187c, 0xb1880, 0xb188c, 0xb19dc, 0xb19e0, 0xb19ec, 0x116cc] {
        assert_eq!(word(prepared.bytes(), offset), 0);
    }
    assert_eq!(word(prepared.bytes(), 0xb18b8), 0xffff);
    assert_eq!(&prepared.bytes()[0xb18bc..0xb18c0], &[0xff, 0xff, 0xfd, 0xfd]);
    assert_eq!(word(prepared.bytes(), 0xb18f8), 0xa5a5a5a5);
}

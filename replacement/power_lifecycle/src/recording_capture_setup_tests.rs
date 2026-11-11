use crate::recording_capture_setup::*;

struct Parameters { custom: bool }
impl CaptureOutputSource for Parameters {
    fn configuration_byte(&mut self, key: ConfigurationByteKey) -> u8 {
        match key.0 { 0xa902 => if self.custom { 2 } else { 0 }, 0xa903 => 0x34, 0xa904 => 0x12, 0xa905 => 0x78, 0xa906 => 0x56, 0xa907 => 0xbc, 0xa908 => 0x9a, _ => panic!("unexpected configuration key") }
    }
    fn parameter_bank(&mut self, bank: CaptureParameterBank) -> CaptureOutputParameters {
        match bank {
            CaptureParameterBank::StillPrimary => CaptureOutputParameters { first: 100, second: 200, third: 300 },
            CaptureParameterBank::StillSecondary => CaptureOutputParameters { first: 400, second: 500, third: 600 },
            CaptureParameterBank::MoviePrimary => CaptureOutputParameters { first: 700, second: 800, third: 900 },
            CaptureParameterBank::MovieSecondary => CaptureOutputParameters { first: 1000, second: 1100, third: 1200 },
        }
    }
}

#[test]
fn custom_still_dimensions_apply_to_both_outputs_but_not_movie() {
    let mut source = Parameters { custom: true };
    let expected = CaptureOutputParameters { first: 0x1234, second: 0x5678, third: 0x9abc };
    assert_eq!(output_parameters(&mut source, CaptureOutputMode::Still, CaptureOutputKind::Primary), expected);
    assert_eq!(output_parameters(&mut source, CaptureOutputMode::Still, CaptureOutputKind::Secondary), expected);
    assert_eq!(output_parameters(&mut source, CaptureOutputMode::Movie, CaptureOutputKind::Primary), CaptureOutputParameters { first: 700, second: 800, third: 900 });
    assert_eq!(output_parameters(&mut source, CaptureOutputMode::Movie, CaptureOutputKind::Secondary), CaptureOutputParameters { first: 1000, second: 1100, third: 1200 });
}

#[test]
fn still_outputs_use_distinct_parameter_banks_without_override() {
    let mut source = Parameters { custom: false };
    assert_eq!(output_parameters(&mut source, CaptureOutputMode::Still, CaptureOutputKind::Primary), CaptureOutputParameters { first: 100, second: 200, third: 300 });
    assert_eq!(output_parameters(&mut source, CaptureOutputMode::Still, CaptureOutputKind::Secondary), CaptureOutputParameters { first: 400, second: 500, third: 600 });
}

#[test]
fn profile_carries_explicit_opaque_word_without_shifting_defined_fields() {
    let mut profile = [0xff; 40];
    initialize_capture_profile(&mut profile, 0x1234_5678);
    assert_eq!(&profile[..16], &[0, 0, 0xe4, 0x42, 0, 0xee, 0x98, 0, 0x78, 0x56, 0x34, 0x12, 0xc0, 0x0d, 0xfb, 0x5f]);
    assert_eq!(&profile[36..40], &[0, 0xf4, 0xfa, 1]);
}

#[test]
fn request_copies_dirty_masks_before_clearing_original_and_preserves_tail() {
    let mut primary = [0x5a; 0x90];
    primary[0x30..0x32].copy_from_slice(&0x1800u16.to_le_bytes());
    let mut packet = [0xa5; 0xa8];
    prepare_capture_packet(&mut primary, &mut packet, 0x1122_3344, 7, 8, 9, 10, -123);
    assert_eq!(&packet[..8], &[0x44, 0x33, 0x22, 0x11, 7, 0, 0, 0]);
    assert_eq!(&packet[0x2a..0x2c], &(-123i16).to_le_bytes());
    assert_eq!(&packet[0x88..0x8c], &[0x5a, 0x5a, 0x5a, 0x5a]);
    assert_eq!(&primary[0x80..], &[0; 16]);
    assert_eq!(&packet[0x98..], &[8, 0, 0, 0, 9, 0, 0, 0, 10, 0, 0, 0, 0xa5, 0xa5, 0xa5, 0xa5]);
}

#[test]
fn ordinary_request_uses_original_category_not_special_configuration() {
    let mut primary = [0; 0x90];
    primary[0x22..0x24].copy_from_slice(&(-19i16).to_le_bytes());
    let mut packet = [0; 0xa8];
    prepare_capture_packet(&mut primary, &mut packet, 0, 0, 0, 0, 0, 555);
    assert_eq!(&packet[0x2a..0x2c], &(-19i16).to_le_bytes());
    assert_eq!(&packet[0x88..0x8c], &[0, 0, 0x40, 0]);
}

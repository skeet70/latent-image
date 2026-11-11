use super::{project_runtime_configuration, RuntimeConfiguration, CONFIGURATION_BYTES, RUNTIME_CONFIGURATION_BYTES};

#[test]
fn projects_byte_fields_scaling_and_little_endian_pairs() {
    let mut configuration = [0u8; CONFIGURATION_BYTES];
    configuration[0x204] = 9;
    configuration[0x205] = 7;
    configuration[0x21a] = 255;
    configuration[0x21b] = 3;
    configuration[0x102] = 0x34;
    configuration[0x103] = 0x12;
    configuration[0x3b1] = 19;
    configuration[0x3b0] = 255;
    configuration[0x255] = 31;
    configuration[0x1640e] = 0x78;
    configuration[0x1640f] = 0x56;
    configuration[0x212] = 0xbc;
    configuration[0x213] = 0x9a;
    let mut output = RuntimeConfiguration([0xa5; RUNTIME_CONFIGURATION_BYTES]);
    project_runtime_configuration(&configuration, &mut output);
    assert_eq!(&output.0[..2], &[9, 7]);
    assert_eq!(&output.0[20..24], &[0x9c, 0x63, 0x2c, 0x01]);
    assert_eq!(&output.0[24..28], &[0x34, 0x12, 0, 0]);
    assert_eq!(&output.0[0xf2..0xf6], &[106, 0, 0x94, 5]);
    assert_eq!(&output.0[0xfc..0xfe], &[155, 0]);
    assert_eq!(&output.0[0x102..0x106], &[0x78, 0x56, 0x78, 0x56]);
    assert_eq!(&output.0[0x168..0x16a], &[0xbc, 0x9a]);
}

#[test]
fn enforces_the_source_upper_bound_without_clamping_valid_values() {
    let mut configuration = [0u8; CONFIGURATION_BYTES];
    let mut output = RuntimeConfiguration([0; RUNTIME_CONFIGURATION_BYTES]);
    for (input, expected) in [(0, 0), (50, 50), (51, 10), (255, 10)] {
        configuration[0x214] = input;
        project_runtime_configuration(&configuration, &mut output);
        assert_eq!(output.0[0x33], expected);
    }
}

#[test]
fn preserves_unwritten_output_and_retains_the_last_repeated_halfword() {
    let mut configuration = [0u8; CONFIGURATION_BYTES];
    configuration[0x997b] = 0x21;
    configuration[0x997c] = 0x43;
    configuration[0x997d] = 0x65;
    configuration[0x997e] = 0x87;
    let mut output = RuntimeConfiguration([0xa5; RUNTIME_CONFIGURATION_BYTES]);
    project_runtime_configuration(&configuration, &mut output);
    assert_eq!(&output.0[0x9c..0x9e], &[0x21, 0x43]);
    assert_eq!(&output.0[0xa6..0xa8], &[0x65, 0x87]);
    for range in [0x9e..0xa6, 0xa8..0xb0, 0xf1..0xf2, 0xfb..0xfc, 0x167..0x168] {
        assert!(output.0[range].iter().all(|value| *value == 0xa5));
    }
    assert!(output.0[0xbe..0xe0].iter().all(|value| *value == 0));
}

#[test]
fn copies_both_complete_configuration_strings() {
    let mut configuration = [0u8; CONFIGURATION_BYTES];
    configuration[0x9742..0x9762].copy_from_slice(b"0123456789abcdefghijklmnopqrstuv");
    configuration[0x9773..0x9793].copy_from_slice(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ012345");
    let mut output = RuntimeConfiguration([0; RUNTIME_CONFIGURATION_BYTES]);
    project_runtime_configuration(&configuration, &mut output);
    assert_eq!(&output.0[0x117..0x137], b"0123456789abcdefghijklmnopqrstuv");
    assert_eq!(&output.0[0x13c..0x15c], b"ABCDEFGHIJKLMNOPQRSTUVWXYZ012345");
}

#[test]
fn serial_conversion_preserves_packed_digit_validation_and_padding() {
    let mut configuration = [0u8; CONFIGURATION_BYTES];
    configuration[0x11d..0x127].copy_from_slice(b"3131313131");
    let mut output = RuntimeConfiguration([0; RUNTIME_CONFIGURATION_BYTES]);
    project_runtime_configuration(&configuration, &mut output);
    assert_eq!(&output.0[2..18], b"3030303131313131");
    configuration[0x11d..0x11f].copy_from_slice(b"12");
    project_runtime_configuration(&configuration, &mut output);
    assert_eq!(&output.0[2..18], b"3030303030303012");
    configuration[0x11d] = b'x';
    project_runtime_configuration(&configuration, &mut output);
    assert_eq!(&output.0[2..18], b"3030303030303000");
    configuration[0x11d..0x11f].copy_from_slice(b"3x");
    project_runtime_configuration(&configuration, &mut output);
    assert_eq!(&output.0[2..18], b"3030303030303000");
    configuration[0x11d..0x121].copy_from_slice(b"31x1");
    project_runtime_configuration(&configuration, &mut output);
    assert_eq!(&output.0[2..18], b"3030303030303100");
}

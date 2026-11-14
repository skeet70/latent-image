use xm1_saclac_profile_geometry::stock::areas::{area_list, convert_into, TransitionAreas, AREA_LIST_BYTES};

#[test]
fn converted_areas_preserve_initialized_destination_bytes_and_clear_absent_records() {
    let mut answer = TransitionAreas::EMPTY;
    answer.count = 1;
    answer.areas[0][6..8].copy_from_slice(&17u16.to_le_bytes());
    answer.areas[0][0x12] = 0x31;
    answer.areas[0][0x13] = 0x32;
    answer.areas[0][0x54] = 0x33;
    answer.areas[0][0x1c..0x1f].copy_from_slice(&[0x41, 0x42, 0x43]);
    let mut output = [0xa5; AREA_LIST_BYTES];
    output[4 + 0x33] = 0x5a;
    convert_into(&answer, &mut output);
    assert_eq!(&output[..8], &[1, 0, 0, 0, 17, 0, 0, 0]);
    assert_eq!(&output[4 + 0x24..4 + 0x28], &[0x31, 0x32, 0x33, 0xa5]);
    assert_eq!(&output[4 + 0x30..4 + 0x34], &[0x41, 0x42, 0x43, 0x5a]);
    assert!(output[108..].iter().all(|&byte| byte == 0));
    convert_into(&TransitionAreas::EMPTY, &mut output);
    assert!(output.iter().all(|&byte| byte == 0));
}

#[test]
fn owned_results_publish_changed_geometry_without_replacing_retained_destination_bytes() {
    let mut answer = TransitionAreas::EMPTY;
    answer.count = 2;
    answer.areas[0][4..6].copy_from_slice(&100u16.to_le_bytes());
    answer.areas[1][4..6].copy_from_slice(&200u16.to_le_bytes());
    let mut list = area_list(&answer);
    list.present()[0].set_origins([[11, 12], [13, 14]]);
    list.present()[1].set_origins([[21, 22], [23, 24]]);
    let mut output = [0xa5; AREA_LIST_BYTES];
    output[108 + 0x27] = 0x5a;
    output[108 + 0x33] = 0x3c;
    list.write_into(&mut output);
    assert_eq!(&output[..8], &[2, 0, 0, 0, 11, 0, 0, 0]);
    assert_eq!(&output[20..28], &[13, 0, 0, 0, 14, 0, 0, 0]);
    assert_eq!(&output[108..116], &[21, 0, 0, 0, 22, 0, 0, 0]);
    assert_eq!(&output[124..132], &[23, 0, 0, 0, 24, 0, 0, 0]);
    assert_eq!(&output[4 + 0x22..4 + 0x24], &[100, 0]);
    assert_eq!(&output[108 + 0x22..108 + 0x24], &[200, 0]);
    assert_eq!([output[4 + 0x27], output[4 + 0x33], output[108 + 0x27], output[108 + 0x33]], [0xa5, 0xa5, 0x5a, 0x3c]);
    assert!(output[212..].iter().all(|&byte| byte == 0));
}

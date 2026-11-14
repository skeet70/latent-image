mod decode_test_crc;

#[test]
fn published_check_value_for_the_standard_vector() {
    assert_eq!(decode_test_crc::crc32(b"123456789"), 0xcbf4_3926);
}

#[test]
fn published_check_value_for_the_sentence_vector() {
    assert_eq!(
        decode_test_crc::crc32(b"The quick brown fox jumps over the lazy dog"),
        0x414f_a339
    );
}

#[test]
fn empty_input_is_zero() {
    assert_eq!(decode_test_crc::crc32(b""), 0);
}

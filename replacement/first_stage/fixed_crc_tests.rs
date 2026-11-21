mod fixed_crc;

fn bytewise(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0xffff_ffff, |crc, byte| fixed_crc::update(crc, *byte))
}

fn ranged(bytes: &[u8]) -> u32 {
    let mut words = vec![0u32; bytes.len().div_ceil(4)];
    for (index, byte) in bytes.iter().enumerate() {
        words[index / 4] |= u32::from(*byte) << (8 * (index % 4));
    }
    unsafe { fixed_crc::update_range(0xffff_ffff, words.as_ptr() as usize, bytes.len()) }
}

#[test]
fn standard_vector() {
    assert_eq!(!bytewise(b"123456789"), 0xcbf4_3926);
    assert_eq!(!ranged(b"123456789"), 0xcbf4_3926);
}

#[test]
fn empty_input() {
    assert_eq!(!bytewise(b""), 0);
    assert_eq!(!ranged(b""), 0);
}

#[test]
fn range_matches_bytewise_on_the_linked_fixed_body() {
    let body = std::fs::read(std::env::var("XM1_FIXED_BODY").expect("XM1_FIXED_BODY names the fixed body")).unwrap();
    assert!(body.len() > 1_000_000);
    for tail in 0..4 {
        let bytes = &body[..body.len() - tail];
        assert_eq!(ranged(bytes), bytewise(bytes), "body less {tail} bytes");
    }
}

use super::mode_events::decimal_pair;

#[test]
fn diagnostic_decimal_width_is_two_zero_padded_digits() {
    assert_eq!(decimal_pair(0), Some(*b"00"));
    assert_eq!(decimal_pair(5), Some(*b"05"));
    assert_eq!(decimal_pair(19), Some(*b"19"));
    assert_eq!(decimal_pair(99), Some(*b"99"));
}

#[test]
fn diagnostic_decimal_width_rejects_values_that_do_not_fit() {
    assert_eq!(decimal_pair(100), None);
    assert_eq!(decimal_pair(u32::MAX), None);
}

use super::requests::request_enabled;

#[test]
fn an_ordinary_request_is_enabled_outside_the_disabled_mode() {
    assert!(request_enabled(0x10, 0, 0, 0));
    assert!(!request_enabled(0x10, 0, 3, 0));
}

#[test]
fn self_enabled_ids_follow_their_own_enable_bit() {
    for id in [0x31, 0x32] {
        assert!(!request_enabled(id, 0b01, 0, 0));
        assert!(request_enabled(id, 0b10, 0, 0));
    }
}

#[test]
fn a_fallback_enable_bit_makes_every_request_follow_its_own() {
    assert!(!request_enabled(0x10, 0, 0, 0b10));
    assert!(request_enabled(0x10, 0b10, 1, 0b10));
    assert!(!request_enabled(0x10, 0b10, 3, 0b10));
}

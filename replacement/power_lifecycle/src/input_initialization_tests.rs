use super::*;

#[test]
fn bitmap_crosses_at_sixty_three_without_aliasing() {
    let mut words = [0; 2];
    assert_eq!(bitmap(&mut words, KeyBit(62), BitmapOperation::Set), 0);
    assert_eq!(bitmap(&mut words, KeyBit(63), BitmapOperation::Set), 0);
    assert_eq!(bitmap(&mut words, KeyBit(62), BitmapOperation::Query), 1 << 62);
    assert_eq!(bitmap(&mut words, KeyBit(63), BitmapOperation::Query), 1);
    bitmap(&mut words, KeyBit(62), BitmapOperation::Reset);
    assert_eq!(bitmap(&mut words, KeyBit(62), BitmapOperation::Query), 0);
    assert_eq!(bitmap(&mut words, KeyBit(63), BitmapOperation::Query), 1);
}

#[test]
fn out_of_range_operations_leave_existing_keys_intact() {
    let mut words = [7];
    for operation in [BitmapOperation::Query, BitmapOperation::Set, BitmapOperation::Reset, BitmapOperation::Clear, BitmapOperation::Any] {
        assert_eq!(bitmap(&mut words, KeyBit(63), operation), 0);
        assert_eq!(bitmap(&mut words, KeyBit(0), BitmapOperation::Any), 7);
    }
    bitmap(&mut words, KeyBit(0), BitmapOperation::Clear);
    assert_eq!(bitmap(&mut words, KeyBit(0), BitmapOperation::Any), 0);
}

#[test]
fn policy_parser_keeps_category_codes_and_closed_modifier_meanings() {
    let enabled = PolicyRecord::decode(0x51, 0x12345678);
    assert_eq!(enabled.id, FlagId::new(17));
    assert_eq!(enabled.mutation, PolicyMutation::Enable);
    assert_eq!(enabled.argument, 0x12345678);
    assert_eq!(enabled.encoded, 0x51);
    assert_eq!(PolicyRecord::decode(0x91, 0).mutation, PolicyMutation::Disable);
    assert_eq!(PolicyRecord::decode(0xd1, 0).mutation, PolicyMutation::Preserve);
    for id in 0x600..=0x603 { assert_eq!(PolicyRecord::decode(id, 0).id, FlagId::new(id)); }
    assert_eq!(PolicyRecord::decode(0x640, 0).id, FlagId::new(0x640));
}

#[test]
fn category_and_key_channel_bounds_are_closed() {
    for (id, expected) in [(0x304, 1), (0x305, 2), (0x317, 3), (0x318, 4), (0x319, 5), (0x31a, 6), (0x311, 7), (0x31b, 8), (0x31c, 9), (0x302, 0), (0x31d, 0), (u32::MAX, 0)] {
        assert_eq!(category_number(FlagId::new(id)), expected);
    }
    assert_eq!(KeyChannel::parse(0), Some(KeyChannel::Channel0));
    assert_eq!(KeyChannel::parse(3), Some(KeyChannel::Channel3));
    assert_eq!(KeyChannel::parse(4), None);
    assert_eq!(KeyChannel::parse(u32::MAX), None);
}

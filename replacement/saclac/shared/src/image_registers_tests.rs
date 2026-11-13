use crate::image_registers::*;

#[test]
fn window_limits_fill_three_fifteen_bit_fields() {
    let words = pack_window_limits(WindowLimits([0x1234, 0x0567, 0x7fff]), [0, 0]);
    assert_eq!(words, [0x0567_1234, 0x7fff]);
}

#[test]
fn a_negative_window_limit_packs_as_the_largest_field() {
    let words = pack_window_limits(WindowLimits([-1, i16::MIN, -2]), [0, 0]);
    assert_eq!(words, [0x7fff_7fff, 0x7fff]);
}

#[test]
fn window_limits_keep_the_bits_outside_their_fields() {
    let words = pack_window_limits(WindowLimits([0, 0, 0]), [u32::MAX, u32::MAX]);
    assert_eq!(words, [0x8000_8000, 0xffff_8000]);
    let words = pack_window_limits(WindowLimits([1, 2, 3]), [0x8000_8000, 0xabcd_0000]);
    assert_eq!(words, [0x8002_8001, 0xabcd_0003]);
}

#[test]
fn groups_are_numbered_in_pipeline_order() {
    let indices: Vec<_> = RegisterGroup::ALL.into_iter().map(RegisterGroup::index).collect();
    assert_eq!(indices, (0..15).collect::<Vec<_>>());
}

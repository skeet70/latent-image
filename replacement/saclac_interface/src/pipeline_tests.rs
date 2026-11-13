use crate::pipeline::FormatPayload;
use crate::stock::pipeline::{format_block, FORMAT_BLOCK_BYTES};

#[test]
fn the_format_block_keeps_the_stock_length_and_zeroes_what_stock_leaves_unset() {
    let payload = FormatPayload { first_scale: 0x0102, second_scale: 8, alignment: [1, 0], remainder_geometry: true };
    let bytes = format_block(&payload);
    assert_eq!(bytes.len(), FORMAT_BLOCK_BYTES);
    assert_eq!(&bytes[..0x2a], &[0; 0x2a]);
    assert_eq!(&bytes[0x2a..], &[0x02, 0x01, 8, 0, 1, 0, 1, 0, 0, 0]);
}

#[test]
fn a_profile_class_alignment_clears_the_remainder_word() {
    let payload = FormatPayload { first_scale: 1, second_scale: 1, alignment: [0, 1], remainder_geometry: false };
    assert_eq!(&format_block(&payload)[0x2e..], &[0, 1, 0, 0, 0, 0]);
}

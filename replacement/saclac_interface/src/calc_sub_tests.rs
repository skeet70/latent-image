use crate::calc::SharedAddress;
use crate::calc_sub::{AspectRatio, ClassificationRequest, Frame, Orientation};
use crate::stock::calc_sub::{self, BLOCK_BYTES, CLASSIFICATION_BYTES};
use crate::stock::StockCallbackWord;
use crate::{lens_effect, CalcSubOperation, LensEffect, PeerOperation};

const FRAME: Frame = Frame {
    samples: SharedAddress(0x0170_0000),
    plane: SharedAddress(0x0171_0000),
    levels: SharedAddress(0x4172_0000),
    columns: SharedAddress(0x0173_0000),
    rows: SharedAddress(0x0174_0000),
    average_levels: 1,
    motion_regions: 7,
    region_list: SharedAddress(0x4175_0000),
};

fn request(aspect: AspectRatio, orientation: Orientation, restart: bool) -> ClassificationRequest {
    ClassificationRequest { frame: FRAME, aspect, orientation, restart, completion: StockCallbackWord(0x0102_df28) }
}

fn words(bytes: &[u8; CLASSIFICATION_BYTES]) -> [u32; 12] {
    core::array::from_fn(|index| u32::from_le_bytes(bytes[4 * index..4 * index + 4].try_into().unwrap()))
}

#[test]
fn a_classification_fills_the_whole_block_word_by_word() {
    let bytes = calc_sub::classification(&request(AspectRatio::SixteenNine, Orientation::Reading3, true));
    assert_eq!(CLASSIFICATION_BYTES, usize::from(BLOCK_BYTES) - 8);
    assert_eq!(
        words(&bytes),
        [0x0170_0000, 0x0171_0000, 2, 2, 0x4172_0000, 0x0173_0000, 0x0174_0000, 1, 7, 0x4175_0000, 1, 0x0102_df28]
    );
}

#[test]
fn aspect_ratios_and_orientations_post_their_codes() {
    let aspects = [AspectRatio::Other, AspectRatio::ThreeTwo, AspectRatio::SixteenNine, AspectRatio::Square];
    let orientations = [Orientation::Reading1, Orientation::Reading2, Orientation::Reading3, Orientation::Reading4];
    for (code, (aspect, orientation)) in aspects.into_iter().zip(orientations).enumerate() {
        let posted = words(&calc_sub::classification(&request(aspect, orientation, false)));
        assert_eq!((posted[2], posted[3]), (code as u32, code as u32));
    }
}

#[test]
fn cleared_flags_post_zero() {
    let mut frame_without_averaging = request(AspectRatio::Other, Orientation::Reading1, false);
    frame_without_averaging.frame.average_levels = 0;
    let posted = words(&calc_sub::classification(&frame_without_averaging));
    assert_eq!((posted[7], posted[10]), (0, 0));
}

#[test]
fn a_classification_is_operation_3e00_without_a_lens_effect() {
    assert_eq!(calc_sub::calc_sub_operation(CalcSubOperation::Request00), 0x3e00);
    assert_eq!(lens_effect(PeerOperation::CalcSub(CalcSubOperation::Request00)), LensEffect::NoLensDrive);
}

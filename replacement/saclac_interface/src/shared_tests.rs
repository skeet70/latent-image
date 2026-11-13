use crate::shared::*;
use crate::stock::{self, StockCallbackWord};
use crate::{lens_effect, LensComOperation, PeerOperation};

const CALLBACK: StockCallbackWord = StockCallbackWord(0x00f2_8e94);

fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) }

#[test]
fn a_relative_move_carries_the_signed_step_and_the_mode_index() {
    let request = RelativeMove { callback: CALLBACK, step: -3, mode: MoveMode::Third, words: [1, 0, 5, 4] };
    let bytes = stock::shared::relative_move(&request);
    assert_eq!(word(&bytes, 0), CALLBACK.0);
    assert_eq!(&bytes[4..8], &[0xfd, 0xff, 0, 0]);
    assert_eq!(word(&bytes, 8), 2);
    assert_eq!([word(&bytes, 12), word(&bytes, 16), word(&bytes, 20), word(&bytes, 24)], [1, 0, 5, 4]);

    let first = stock::shared::relative_move(&RelativeMove { mode: MoveMode::First, ..request });
    assert_eq!(word(&first, 8), 0);
}

#[test]
fn a_lens_synchronization_names_only_its_callback() {
    let bytes = stock::shared::lens_synchronize(CALLBACK);
    assert_eq!(word(&bytes, 0), CALLBACK.0);
    assert_eq!(word(&bytes, 4), 0);
}

#[test]
fn a_lenscom_request_puts_its_code_under_the_fixed_selector() {
    let command = LensComCommand { code: LensComCode::new(3).unwrap(), completion: StockCallbackWord(0x006b_6f9c) };
    let bytes = stock::shared::lens_com(&command);
    assert_eq!(word(&bytes, 0), 0x006b_6f9c);
    assert_eq!(&bytes[4..8], &[0x40, 0x2a, 3, 0]);
    assert_eq!(bytes[44], 1);
    assert!(bytes[8..44].iter().chain(&bytes[45..]).all(|byte| *byte == 0));
    assert_eq!(stock::shared::lens_com_operation(LensComOperation::Request2d), 0x722d);
}

#[test]
fn a_lenscom_code_fits_one_nibble() {
    assert_eq!(LensComCode::new(0xf).map(LensComCode::get), Some(0xf));
    assert_eq!(LensComCode::new(0x10), None);
}

#[test]
fn lenscom_requests_need_the_lens_safety_contract() {
    assert!(lens_effect(PeerOperation::LensCom(LensComOperation::Request2d)).requires_lens_safety_contract());
}

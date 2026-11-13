use crate::imaging::*;
use crate::stock::{imaging::imaging_payload, StockCallbackWord as Cb};
use crate::ImgDcOperation as Op;

fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) }
fn half(bytes: &[u8], offset: usize) -> u16 { u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap()) }

fn job() -> S1Job {
    S1Job {
        levels: [-2, 0x102, 0x103],
        lens_index: 0x11,
        lens_word: 0x12,
        selection: 0x13,
        step: 0x14,
        step_limit: 0x15,
        step_mode: 0x16,
        curves: [0x21, 0x22],
        output: 0x23,
        view_mode: 0x24,
        descriptors: [0x31, 0x32, 0x33],
        setup: [0x34, 0x35, 0x36],
        window: [0x41, 0x42],
        exposure: 0x43,
        exposure_offset: -5,
        view_active: 1,
        unselected_view: 0x51,
        view_byte: 0x52,
        classes: [5, 6],
        ratio: 0x0280_01e0,
        span: 0x61,
        class: 0x62,
        unselected: 0x63,
        scene: 0x64,
    }
}

#[test]
fn the_s1_job_block_precedes_its_three_completions() {
    let (operation, message) = imaging_payload(&ImagingPayload::BeginS1 { job: job(), callbacks: [Cb(0x00f2_27c0), Cb(0x00f2_2928), Cb(0x00f2_2794)] });
    let bytes = message.bytes();
    assert_eq!((operation, bytes.len()), (Op::Request42, 136));
    assert_eq!([half(bytes, 0), half(bytes, 2), half(bytes, 4), half(bytes, 6)], [0xfffe, 0x102, 0x103, 0]);
    assert_eq!([word(bytes, 8), word(bytes, 12), word(bytes, 16), word(bytes, 20)], [0x11, 0x12, 0x13, 0x14]);
    assert_eq!(&bytes[24..28], &[0x15, 0, 0x16, 0]);
    assert_eq!((word(bytes, 28), word(bytes, 32), word(bytes, 36), word(bytes, 40)), (0x21, 0x22, 0x23, 0x24));
    assert_eq!([word(bytes, 44), word(bytes, 52), word(bytes, 56), word(bytes, 64)], [0x31, 0x33, 0x34, 0x36]);
    assert_eq!([word(bytes, 68), word(bytes, 72), word(bytes, 76)], [0x41, 0x42, 0x43]);
    assert_eq!((half(bytes, 80), half(bytes, 82)), (0xfffb, 0));
    assert_eq!((word(bytes, 84), word(bytes, 88), &bytes[92..96]), (1, 0x51, &[0x52, 0, 0, 0][..]));
    assert_eq!([word(bytes, 96), word(bytes, 100), word(bytes, 104), word(bytes, 108)], [5, 6, 0x0280_01e0, 0x61]);
    assert_eq!([word(bytes, 112), word(bytes, 116), word(bytes, 120)], [0x62, 0x63, 0x64]);
    assert_eq!([word(bytes, 124), word(bytes, 128), word(bytes, 132)], [0x00f2_27c0, 0x00f2_2928, 0x00f2_2794]);
}

#[test]
fn ending_the_s1_job_sends_its_callback_alone() {
    let (operation, message) = imaging_payload(&ImagingPayload::EndS1 { callback: Cb(0x00f2_26cc) });
    assert_eq!((operation, message.bytes()), (Op::Request43, &[0xcc, 0x26, 0xf2, 0][..]));
}

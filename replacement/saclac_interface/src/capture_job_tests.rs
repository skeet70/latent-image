use crate::capture_job::*;
use crate::stock::capture_job as stock;

fn put(bytes: &mut [u8], offset: usize, value: &[u8]) { bytes[offset..offset + value.len()].copy_from_slice(value); }

fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) }

fn half(bytes: &[u8], offset: usize) -> u16 { u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap()) }

#[test]
fn a_capture_start_skips_the_status() {
    let mut payload = [0u8; 20];
    put(&mut payload, 0, &9u32.to_le_bytes());
    put(&mut payload, 4, &3u32.to_le_bytes());
    put(&mut payload, 8, &0x11u16.to_le_bytes());
    put(&mut payload, 0xa, &0x22u16.to_le_bytes());
    put(&mut payload, 0xc, &5u32.to_le_bytes());
    put(&mut payload, 0x10, &6u32.to_le_bytes());
    let start = stock::capture_start(&payload).unwrap();
    assert_eq!(start, CaptureStart { profile: 3, curve_pair: [0x11, 0x22], curve_words: [5, 6] });
    assert!(start.flagged());
    assert_eq!(stock::capture_start(&payload[..19]), None);
}

#[test]
fn a_capture_end_carries_the_readout_pair_and_a_detail() {
    let mut payload = [0u8; 16];
    for index in 0..4 { put(&mut payload, 4 * index, &(index as u32 + 7).to_le_bytes()); }
    assert_eq!(stock::capture_end(&payload), Some(CaptureEnd { readout: [8, 9], detail: 10 }));
    assert_eq!(stock::capture_end(&payload[..15]), None);
}

#[test]
fn an_output_is_the_88_bytes_after_the_status() {
    let mut payload = [0u8; 92];
    put(&mut payload, 4 + 0x1c, &0x55u32.to_le_bytes());
    payload[4 + 0x3c] = 3;
    payload[4 + 0x3e] = 2;
    put(&mut payload, 4 + 0x44, &0x1234u16.to_le_bytes());
    put(&mut payload, 4 + 0x54, &0x89ab_cdefu32.to_le_bytes());
    let output = stock::capture_output(&payload).unwrap();
    assert_eq!((output.profile(), output.outputs(), output.delivered()), (0x55, 3, 2));
    assert_eq!(output.selection().pair, 0x1234);
    assert_eq!(output.trailing_word(), 0x89ab_cdef);
    assert_eq!(stock::capture_output(&payload[..91]), None);
}

#[test]
fn a_delivery_finds_areas_with_a_nonzero_status() {
    assert_eq!(stock::area_delivery(&[1, 0, 0, 0, 0, 0, 0, 0]), Some(AreaDelivery { found: true }));
    assert_eq!(stock::area_delivery(&[0; 8]), Some(AreaDelivery { found: false }));
    assert_eq!(stock::area_delivery(&[0; 3]), None);
}

#[test]
fn a_capture_request_lays_its_windows_transfer_and_record_bytes() {
    let request = CaptureRequest {
        source: 0xa0,
        windows: [CaptureWindow { left: 1, width: 2, half_height: 3 }, CaptureWindow { left: 4, width: 5, half_height: 6 }],
        transfer: TransferPoint { reference: 0x77, second_limit: -1, trim: 2, offset: 3, residual: 4, step: 5 },
        options: 0x200,
        held: true,
        subject_area: 9,
        ..CaptureRequest::default()
    };
    let bytes = stock::capture_request(&request);
    assert_eq!(word(&bytes, 0), 0xa0);
    assert_eq!([half(&bytes, 4), half(&bytes, 6), half(&bytes, 8), half(&bytes, 0xa)], [1, 2, 3, 3]);
    assert_eq!([half(&bytes, 0x14), half(&bytes, 0x16), half(&bytes, 0x18), half(&bytes, 0x1a)], [4, 5, 6, 6]);
    assert_eq!(word(&bytes, 0x20), 0x77);
    assert_eq!(half(&bytes, 0x24), 0xffff);
    assert_eq!(half(&bytes, 0x2c), 5);
    assert_eq!(word(&bytes, 0x64), 0x200);
    assert_eq!([half(&bytes, 0x68), half(&bytes, 0x6a)], [100, 16]);
    assert_eq!((bytes[0x87], bytes[0x91], bytes[0x97]), (1, 1, 9));
}

#[test]
fn a_display_request_is_five_words() {
    let bytes = stock::display_request(&DisplayRequest { setting_word: 1, setting_pair: [2, 3], extent: [4, 5] });
    assert_eq!(core::array::from_fn::<u32, 5, _>(|index| word(&bytes, 4 * index)), [1, 2, 3, 4, 5]);
}

#[test]
fn capture_statistics_fill_the_four_parts_of_the_record() {
    let statistics = CaptureStatistics {
        frame: StatisticsFrame { record_word: 1, frame_words: [2, 3], layout: [4, 5], flagged: true, at_0198: 6, selector: 7 },
        window: [0, 0, 480, 640],
        lens: [8, 9, 10],
        profile: StatisticsProfile { calibration: [11, 12, 13], option: 14, record_byte: 15, ..StatisticsProfile::default() },
    };
    let setting = stock::statistics_setting(&statistics);
    let bytes = setting.bytes();
    assert_eq!([word(bytes, 0xc), word(bytes, 0x10), word(bytes, 0x18), word(bytes, 0x20)], [240, 640, 1, 7]);
    assert_eq!([word(bytes, 0x40), word(bytes, 0x44), word(bytes, 0x48)], [480, 640, 8]);
    assert_eq!([half(bytes, 0x7c), half(bytes, 0x80), half(bytes, 0x86)], [11, 13, 100]);
    assert_eq!((bytes[0x8c], bytes[0x8e]), (14, 15));
}

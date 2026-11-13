use crate::calc::SharedAddress;
use crate::exposure::{Correction, EvaluationRequest, MeasuredEntry, ReleaseStatus, StatisticsRequest, CORRECTION_HALVES};
use crate::stock::exposure::*;
use crate::stock::StockCallbackWord;

fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) }

#[test]
fn statistics_request_places_each_value_and_the_staged_subject_list() {
    let mut areas = [0; 160];
    areas[0] = 0xa1;
    areas[159] = 0xa2;
    let request = StatisticsRequest {
        lens_reference: 0x1111,
        calculation_mode: 2,
        subject_query: [3, 4],
        area_layout: 0x0003_0002,
        context_mode: 5,
        correction_mode: 6,
        statistics_option: 7,
        single_area: true,
        areas,
        area_count: 9,
        scaled: false,
    };
    let bytes = statistics_request(&request, 0x016b_3e1c);
    let words: [u32; 9] = core::array::from_fn(|index| word(&bytes, 4 * index));
    assert_eq!(words, [0x1111, 2, 3, 4, 0x0003_0002, 5, 6, 7, 1]);
    assert_eq!(&bytes[0x24..0x2c], &[100, 0, 100, 0, 16, 0, 0, 0]);
    assert_eq!((bytes[0x2c], bytes[0xcb], bytes[0xcc]), (0xa1, 0xa2, 9));
    assert_eq!(&bytes[0xcd..0xd0], &[0; 3]);
    assert_eq!((word(&bytes, 0xd0), word(&bytes, 0xd4), word(&bytes, 0xd8)), (0, 1, 0x016b_3e1c));
}

#[test]
fn the_statistics_callback_measures_on_status_three_and_releases_otherwise() {
    assert_eq!(statistics_result(3, 0x105), StatisticsResult::Measured(MeasuredEntry(5)));
    assert_eq!(statistics_result(1, 7), StatisticsResult::Released(ReleaseStatus(1)));
    assert_eq!(measured(&measured_payload(MeasuredEntry(6))), Some(MeasuredEntry(6)));
    assert_eq!(release(&release_payload(ReleaseStatus(0x0102_0304))), Some(ReleaseStatus(0x0102_0304)));
    assert_eq!(measured(&[]), None);
    assert_eq!(release(&[1, 2, 3]), None);
}

#[test]
fn evaluation_reads_the_entry_and_four_levels() {
    let mut payload = [0u8; 16];
    payload[0] = 4;
    payload[8..16].copy_from_slice(&[1, 0, 2, 0, 3, 0, 0x34, 0x12]);
    let evaluation = evaluation(&payload).unwrap();
    assert_eq!((evaluation.entry, evaluation.levels), (4, [1, 2, 3, 0x1234]));
    assert!(super::stock::exposure::evaluation(&payload[..15]).is_none());
}

#[test]
fn correction_round_trips_and_an_empty_payload_carries_none() {
    let mut halves = [0u16; CORRECTION_HALVES];
    halves[0x30] = 0x5678;
    halves[0x31] = 0x1234;
    let correction = Correction(halves);
    assert_eq!(super::stock::exposure::correction(&correction_payload(&correction)), Some(correction));
    assert_eq!(correction.word(0x30), 0x1234_5678);
    assert_eq!(super::stock::exposure::correction(&[]), None);
}

#[test]
fn evaluation_request_places_the_block_after_the_callback_and_the_addresses_after_the_block() {
    let request = EvaluationRequest {
        row: 3,
        measured: [[1, 2], [3, 4], [5, 6]],
        statistics: [7, 8, 9, 10],
        row_buffer: Some(SharedAddress(0x5ffb_6120)),
        rates: [100, 10, 20, 10],
        after_reference: true,
        value: 0x1234,
        lower_value: 0x8000,
        tolerance_above: 20,
        tolerance_below: -20,
        floor: -600,
        direction: 3,
        column_count: 2,
        column_regions: Some(SharedAddress(0x016b_4b00)),
        column_buffer: Some(SharedAddress(0x5ffb_7130)),
        dimensions: [11, 12, 13],
        next_profile: 0x17,
        level_curves: [Some(SharedAddress(0x5ffb_ece0)); 4],
        level_classes: Some(SharedAddress(0x5ffb_f530)),
        frame: None,
        subject_curves: [Some(SharedAddress(0x5ffb_f4e0)); 4],
        selection_flags: Some(SharedAddress(0x5ffc_0130)),
    };
    let bytes = evaluation_request(&request, StockCallbackWord(0x00f1_ee10));
    let half = |offset: usize| u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
    assert_eq!((word(&bytes, 0), word(&bytes, 4), word(&bytes, 0x14)), (0x00f1_ee10, 3, 0x5ffb_6120));
    assert_eq!([half(8), half(10), half(12), half(14), half(0x10), half(0x12), half(0x1c), half(0x1e)], [1, 2, 16, 16, 3, 4, 5, 6]);
    assert_eq!((word(&bytes, 0x20), word(&bytes, 0x2c), word(&bytes, 0x30)), (100, 10, 1));
    assert_eq!((half(0x34), half(0x36), half(0x3c), half(0x3e)), (0x1234, 0x8000, 250, -600i16 as u16));
    assert_eq!((word(&bytes, 0x40), half(0x48), word(&bytes, 0x50), word(&bytes, 0x54)), (3, 7, 2, 0x016b_4b00));
    assert_eq!((half(0x5c), word(&bytes, 0x64)), (11, 0x17));
    assert_eq!((word(&bytes, 0x68), word(&bytes, 0x78), word(&bytes, 0x7c), word(&bytes, 0x90)), (0x5ffb_ece0, 0x5ffb_f530, 0, 0x5ffc_0130));
    assert!(bytes[0x94..].iter().all(|byte| *byte == 0));
}

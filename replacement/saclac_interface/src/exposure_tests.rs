use crate::calc::SharedAddress;
use crate::calc::CalcPayload;
use crate::exposure::{CalculationInput, Correction, CorrectionRequest, EvaluationRequest, MeasuredEntry, ReleaseStatus, StatisticsRequest, CORRECTION_HALVES};
use crate::CalcOperation;
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

#[test]
fn correction_request_places_the_block_after_the_callback_and_the_buffers_after_the_block() {
    let address = |at: u32| Some(SharedAddress(at));
    let request = CorrectionRequest {
        style: 2,
        measurement_selection: 1,
        selection_kind: 0x0b,
        selection_flags: 0x1234,
        record_kind: 6,
        record_44: 7,
        focus_class: 8,
        record_32: 9,
        record_3e: 10,
        variant: 11,
        lens_reference: 0x55,
        selection_auxiliary: 12,
        image_word: 0xabcd,
        rate: 40,
        list_words: [13, 14, 15],
        list_byte: 16,
        list_word: 17,
        level_curves: [address(0x5ffb_ece0), address(0x5ffb_eee0), address(0x5ffb_f0e0), address(0x5ffb_f2e0)],
        start: 520,
        start_above_target: 20,
        reference_offset: -3i16 as u16,
        curve_table: 0x6431,
        rate_setting: 18,
        reference: 790,
        ratio: [3, 4],
        second_step: true,
        treated: true,
        first_treatment: false,
        subjects: address(0x016b_3e1c),
        summary: address(0x016b_4230),
        subject_curves: [address(0x5ffb_f4e0), address(0x5ffb_f4f4), address(0x5ffb_f508), address(0x5ffb_f51c)],
        header_61: 1,
        header_47: 5,
        unmeasured: true,
        record_20: -2,
        buffers: [address(0x5ffe_be8c), address(0x5ffe_bf0c)],
        frame: address(0x4000_0000),
    };
    let (operation, message) = super::stock::calc_payload(&CalcPayload::Correction { request, callback: StockCallbackWord(0x00f1_fc08) });
    assert_eq!((operation, message.bytes().len()), (CalcOperation::Request01, CORRECTION_REQUEST_BYTES));
    let bytes = message.bytes();
    let block = |offset: usize| word(bytes, 4 + offset);
    let half = |offset: usize| u16::from_le_bytes([bytes[4 + offset], bytes[5 + offset]]);
    assert_eq!(word(bytes, 0), 0x00f1_fc08);
    assert_eq!([block(0), block(4), block(8), u32::from(half(0x0c)), block(0x10), block(0x14), block(0x18), block(0x1c)], [2, 1, 0x0b, 0x1234, 6, 0x55, 7, 12]);
    assert_eq!([block(0x20), block(0x24), block(0x28), block(0x2c), block(0x30), block(0x34)], [8, 9, 10, 11, 0xabcd, 40]);
    assert_eq!([block(0x38), block(0x3c), block(0x40), u32::from(bytes[4 + 0x44]), block(0x48), block(0x54)], [13, 14, 15, 16, 0x5ffb_ece0, 0x5ffb_f2e0]);
    assert_eq!([half(0x58), half(0x5a), half(0x5c), half(0x64), half(0x66)], [520, 20, -3i16 as u16, 18, 790]);
    assert_eq!([block(0x60), block(0x68), block(0x6c), block(0x70), block(0x74), block(0x78)], [0x6431, 3, 4, 1, 1, 0]);
    assert_eq!([block(0x7c), block(0x80), block(0x84), block(0x88), block(0x94)], [0x016b_3e1c, 0x016b_4230, 17, 0x5ffb_f4e0, 0x5ffb_f51c]);
    assert_eq!([block(0x98), block(0x9c), block(0xa0), u32::from(half(0xa4))], [1, 5, 1, -2i16 as u16 as u32]);
    assert_eq!([word(bytes, 0xac), word(bytes, 0xb0), word(bytes, 0xb4)], [0x5ffe_be8c, 0x5ffe_bf0c, 0x4000_0000]);
    for unwritten in [0x0e, 0x0f, 0x45, 0x46, 0x47, 0x5e, 0x5f, 0xa6, 0xa7] {
        assert_eq!(bytes[4 + unwritten], 0, "block byte {unwritten:#x}");
    }
    assert!(bytes[0xb8..].iter().all(|byte| *byte == 0));
}

#[test]
fn calculation_input_places_each_value_and_the_constants_calc_reads() {
    let input = CalculationInput {
        selection: true,
        lens_reference: 0x55,
        target_word: 0x80,
        target_half: 0x1234,
        area: [7, 8],
        target_level: -5,
        start_level: 300,
        measured_level: 400,
        level_profile: 0x6431,
        unmeasured: true,
        level_policy: 6,
        refinement_pass: true,
        first_treatment: true,
        metering_level: 250,
        grid: [1, 4],
        grid_steps: [0, 3],
        plan_kind: 2,
        plan_exposure: [11, 12, 13, 14, 15],
        measurement_selection: 1,
        selection_kind: 0x0b,
        selection_flags: 0x0102,
        selection_style: 9,
        selection_auxiliary: 10,
        record_33: 0x33,
        record_26: 0x2626,
        offset: -1,
        lead: -2,
        source: 0x44,
        record_20: -3,
        record_level: 500,
        record_24: -4,
        record_5f: 0x5f,
        header_47: 0x47,
        header_61: 0x61,
        header_63: 0x63,
        capture_offset: 0xe0,
        shared_85c: 0x85c,
        record_lens_reference: 0x66,
        levels: [1, 2, 3, 4, 5, 6, 7, 8, -9],
        option: 1,
        ..CalculationInput::default()
    };
    let bytes = calculation_input(&input);
    let half = |offset: usize| u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
    assert_eq!(bytes.len(), CALCULATION_INPUT_BYTES);
    assert_eq!([word(&bytes, 0), word(&bytes, 4), word(&bytes, 8), word(&bytes, 0x0c), u32::from(half(0x10))], [0, 4, 0x55, 0x80, 0x1234]);
    assert_eq!(word(&calculation_input(&CalculationInput::default()), 4), 3);
    assert_eq!([word(&bytes, 0x14), word(&bytes, 0x18)], [7, 8]);
    assert_eq!([half(0x1c), half(0x1e), half(0x20)], [-5i16 as u16, 300, 400]);
    assert_eq!([word(&bytes, 0x24), word(&bytes, 0x28), word(&bytes, 0x2c), word(&bytes, 0x30), word(&bytes, 0x34)], [0x6431, 1, 6, 1, 1]);
    assert_eq!((half(0x38), &bytes[0x3c..0x40], word(&bytes, 0x40)), (250, &[1, 4, 0, 3][..], 2));
    assert_eq!([half(0x44), half(0x4c)], [11, 15]);
    assert_eq!([word(&bytes, 0x50), word(&bytes, 0x54), u32::from(half(0x58)), word(&bytes, 0x78), word(&bytes, 0x9c)], [1, 0x0b, 0x0102, 9, 10]);
    assert_eq!((bytes[0x60], half(0x62)), (0x33, 0x2626));
    assert_eq!([half(0x80), half(0x82), half(0x88), half(0x90), half(0x92), half(0x94)], [-1i16 as u16, 0, -2i16 as u16, -3i16 as u16, 500, -4i16 as u16]);
    assert_eq!([word(&bytes, 0x8c), word(&bytes, 0xdc), word(&bytes, 0xe0), word(&bytes, 0xf8), word(&bytes, 0xfc)], [0x44, 0x5f, 0xe0, 0x85c, 0]);
    assert_eq!([word(&bytes, 0x100), u32::from(half(0x104)), u32::from(half(0x114))], [0x66, 1, -9i16 as u16 as u32]);
    assert_eq!([word(&bytes, 0x118), word(&bytes, 0x11c), word(&bytes, 0x120), u32::from(half(0x124)), word(&bytes, 0x128)], [0, 1, 4, 0, 1]);
    assert_eq!([word(&bytes, 0xa0), word(&bytes, 0xa4), word(&bytes, 0xac)], [0x47, 0x61, 0x63]);
}

use crate::calc::*;
use crate::stock::StockCallbackWord;
use crate::{stock, Calc1Operation, CalcOperation};

fn counting<const N: usize>(first: u8) -> Record<N> {
    let mut bytes = [0; N];
    for (index, byte) in bytes.iter_mut().enumerate() { *byte = first.wrapping_add(index as u8); }
    Record(bytes)
}

const CALLBACK: StockCallbackWord = StockCallbackWord(0x00f3_c104);

#[test]
fn copied_records_follow_the_callback_for_calc() {
    let record = counting::<120>(1);
    let (operation, message) = stock::calc_payload(&CalcPayload::Search { record, callback: CALLBACK });
    assert_eq!(operation, CalcOperation::Request09);
    assert_eq!(&message.bytes()[..4], &CALLBACK.0.to_le_bytes());
    assert_eq!(&message.bytes()[4..], &record.0);

    let record = counting::<20>(7);
    let (operation, message) = stock::calc_payload(&CalcPayload::Image { record, callback: CALLBACK });
    assert_eq!((operation, message.bytes().len()), (CalcOperation::Request0a, 24));
    assert_eq!(&message.bytes()[4..], &record.0);

    let block = counting::<64>(3);
    let (operation, message) = stock::calc_payload(&CalcPayload::PipelineSecond { block, callback: CALLBACK });
    assert_eq!((operation, message.bytes().len()), (CalcOperation::Request04, 68));
    assert_eq!(&message.bytes()[4..], &block.0);

    let block = counting::<116>(9);
    let (operation, message) = stock::calc_payload(&CalcPayload::PipelineThird { block, callback: CALLBACK });
    assert_eq!((operation, message.bytes().len()), (CalcOperation::Request05, 120));
    assert_eq!(&message.bytes()[..4], &CALLBACK.0.to_le_bytes());
    assert_eq!(&message.bytes()[4..], &block.0);
}

#[test]
fn calc1_records_precede_the_callback() {
    use crate::round_acquisition::{AcquisitionRequest, SelectionRequest};
    use crate::stock::round_acquisition::LIVE_REGION;
    for (payload, operation, length, first) in [
        (Calc1Payload::Acquisition { request: AcquisitionRequest { mode: 7, ..AcquisitionRequest::default() }, callback: CALLBACK }, Calc1Operation::Request00, 172, 7),
        (Calc1Payload::Selection { request: SelectionRequest { first: true, ..SelectionRequest::default() }, region: LIVE_REGION, callback: CALLBACK }, Calc1Operation::Request01, 232, 1),
    ] {
        let (posted, message) = stock::calc1_payload(&payload);
        assert_eq!((posted, message.bytes().len(), message.bytes()[0]), (operation, length, first));
        assert_eq!(&message.bytes()[length - 4..], &CALLBACK.0.to_le_bytes());
    }
}

#[test]
fn auxiliary_request_places_each_value_the_constants_and_the_buffers() {
    let request = AuxiliaryRequest {
        complete: true,
        pass: 2,
        selection: 3,
        profile: [0x0101_0101, 0x0202_0202, 0x0303_0303, 0x0404_0404],
        sample: AuxiliarySample {
            mode: 0x1111_1111,
            gain_step: 0x1212_1212,
            exposure: [0x1313_1313, 0x1414_1414],
            level: 0x1515,
            offset_level: 0x1616,
            adjustment: 0x1717_1717,
            size: [0x1818_1818, 0x1919_1919],
            origin: [0x1a1a_1a1a, 0x1b1b_1b1b],
            extent: [0x1c1c_1c1c, 0x1d1d_1d1d],
        },
        reference: 0x2121,
        rate: 0x2222_2222,
        level_class: 0x2323_2323,
        window_status: 4,
        window_halfword: 0x2424,
        resource_level: 0x2525,
        word: 0x2626_2626,
        halfword: 0x2727,
    };
    let (operation, message) = stock::calc1_payload(&Calc1Payload::Auxiliary { request, callback: CALLBACK });
    assert_eq!(operation, Calc1Operation::Request02);
    let bytes = message.bytes();
    assert_eq!(bytes.len(), 128);
    let word = |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let half = |offset: usize| u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap());
    assert_eq!((word(0), bytes[4], word(8)), (1, 2, 3));
    assert_eq!([word(0x0c), word(0x10), word(0x14), word(0x18)], request.profile);
    assert_eq!((word(0x1c), half(0x22), word(0x24), half(0x28), half(0x2a)), (0x1212_1212, 0x1515, 0x1313_1313, 0x2121, 0x1616));
    assert_eq!((word(0x2c), word(0x30), word(0x34), word(0x38)), (0x1111_1111, 0x1414_1414, 0x1818_1818, 0x1919_1919));
    assert_eq!((word(0x3c), word(0x40), half(0x44), half(0x46), half(0x48)), (0x1c1c_1c1c, 0x1d1d_1d1d, 100, 100, 256));
    assert_eq!((word(0x4c), word(0x50), word(0x54), word(0x58), word(0x5c)), (0x1a1a_1a1a, 0x1b1b_1b1b, 0x2222_2222, 0x5ffe_d304, 0x5ffe_e304));
    assert_eq!((word(0x64), word(0x68), word(0x6c)), (0x2323_2323, 0x1717_1717, 1));
    assert_eq!((half(0x70), half(0x72), word(0x74), half(0x78), word(0x7c)), (0x2525, 0x2424, 0x2626_2626, 0x2727, CALLBACK.0));
    for unwritten in [5..8, 0x20..0x22, 0x4a..0x4c, 0x60..0x64, 0x7a..0x7c] { assert!(bytes[unwritten].iter().all(|&byte| byte == 0)); }

    for (status, flag) in [(2, 0), (3, 1), (5, 0)] {
        let request = AuxiliaryRequest { window_status: status, ..request };
        let (_, message) = stock::calc1_payload(&Calc1Payload::Auxiliary { request, callback: CALLBACK });
        assert_eq!(message.bytes()[0x6c], flag);
    }
}

#[test]
fn pipeline_first_and_calculation_fill_only_their_leading_fields_of_a_full_block() {
    let (operation, message) = stock::calc_payload(&CalcPayload::PipelineFirst {
        first: counting::<60>(1), second: counting::<24>(100), callback: CALLBACK,
    });
    assert_eq!((operation, message.bytes().len()), (CalcOperation::Request03, 232));
    assert_eq!(&message.bytes()[4..64], &counting::<60>(1).0);
    assert_eq!(&message.bytes()[64..88], &counting::<24>(100).0);
    assert!(message.bytes()[88..].iter().all(|byte| *byte == 0));

    let (operation, message) = stock::calc_payload(&CalcPayload::Calculation {
        block: SharedAddress(0x016b_4f10), source: SharedAddress(0x016b_7000), callback: StockCallbackWord(0x00f1_d154),
    });
    assert_eq!((operation, message.bytes().len()), (CalcOperation::Request02, 232));
    assert_eq!(&message.bytes()[..12], &[0x54, 0xd1, 0xf1, 0, 0x10, 0x4f, 0x6b, 1, 0, 0x70, 0x6b, 1]);
    assert!(message.bytes()[12..].iter().all(|byte| *byte == 0));
}

#[test]
fn source_request_places_each_value_around_two_unwritten_halfwords() {
    let request = SourceRequest {
        statistics: SourceStatistics::Primary,
        profile: [0x0102_0304, 0x0506_0708],
        decade: 0x090a,
        profile_halfword: 0x0b0c,
        level: 0x0d0e,
        reference: 0x0f10,
        size: [0x1112_1314, 0x1516_1718],
        exposure: 0x191a_1b1c,
        offset_mode: 0,
        mode: 0x1d1e_1f20,
        selection: 0x21,
        extension: 0x2223_2425,
    };
    let (operation, message) = stock::calc_payload(&CalcPayload::Source { request, callback: CALLBACK });
    assert_eq!(operation, CalcOperation::Request11);
    let mut expected = [0u8; 56];
    for (offset, word) in [(0, CALLBACK.0), (4, 0x5ffe_d304), (8, 0x0102_0304), (20, 0x21), (28, 0x1112_1314), (32, 0x1516_1718), (36, 0x0506_0708), (40, 0x191a_1b1c), (48, 0x1d1e_1f20), (52, 0x2223_2425)] {
        expected[offset..offset + 4].copy_from_slice(&u32::to_le_bytes(word));
    }
    for (offset, half) in [(12, 0x090au16), (14, 0x0b0c), (16, 0x0d0e), (24, 0x0f10)] {
        expected[offset..offset + 2].copy_from_slice(&half.to_le_bytes());
    }
    assert_eq!(message.bytes(), &expected);
}

#[test]
fn window_request_places_each_value_and_the_statistics_buffer() {
    let request = WindowRequest {
        selection: 2,
        mode: 5,
        selected: true,
        sample: WindowSample { mode: 0x0102_0304, flags: 0x15, exposure: 0x0607_0809, level: 0x0a0b, adjustment: 0x1112_1314 },
        level: 0x2122,
        reference: 0x3132,
        window: WindowRecord { first: 0x4142_4344, second: 0x5152_5354, status: 2, halfword: 0x6162 },
        refresh: true,
        adjustment: WindowAdjustment {
            controllers: PlannerWord(0x7172_7374),
            search: PlannerWord(1),
        }.for_profile(WindowAdjustmentProfile::from_byte(6)),
    };
    let (operation, message) = stock::calc_payload(&CalcPayload::Window { request, callback: CALLBACK });
    assert_eq!(operation, CalcOperation::Request0c);
    assert_eq!(message.bytes(), &[
        2, 0, 0, 0, 0x44, 0x43, 0x42, 0x41, 0x22, 0x21, 0, 0, 4, 3, 2, 1,
        0x15, 0, 0, 0, 9, 8, 7, 6, 0x0b, 0x0a, 0x32, 0x31, 0, 0, 0, 0,
        0x62, 0x61, 0, 0, 0xb4, 0xc1, 0xfe, 0x5f, 0, 0x10, 0, 0, 0x14, 0x13, 0x12, 0x11,
        2, 0, 0, 0, 0x54, 0x53, 0x52, 0x51, 0x74, 0x73, 0x72, 0x71, 1, 0, 0, 0,
        1, 0, 0, 0, 5, 0, 0, 0, 1, 0, 0, 0, 0x04, 0xc1, 0xf3, 0,
    ][..]);
    for profile_byte in [0, 5, 7, u8::MAX] {
        let adjustment = request.adjustment.for_profile(WindowAdjustmentProfile::from_byte(profile_byte));
        let (_, message) = stock::calc_payload(&CalcPayload::Window {
            request: WindowRequest { adjustment, ..request },
            callback: CALLBACK,
        });
        assert_eq!(&message.bytes()[56..64], &[0x74, 0x73, 0x72, 0x71, 0, 0, 0, 0]);
    }
}

#[test]
fn release_posts_nothing_under_its_own_code() {
    let (operation, message) = stock::calc_payload(&CalcPayload::Release);
    assert_eq!(operation, CalcOperation::Request12);
    assert!(message.bytes().is_empty());
    assert_eq!(stock::calc_operation(operation), 0x3c12);
}

/// The message bytes with each `(offset, bytes)` placed and every other byte zero.
fn placed<const N: usize>(fields: &[(usize, &[u8])]) -> [u8; N] {
    let mut bytes = [0; N];
    for (offset, value) in fields { bytes[*offset..*offset + value.len()].copy_from_slice(value); }
    bytes
}

fn processing() -> CaptureProcessing {
    CaptureProcessing {
        extent: [0x1010_1011, 0x1010_1012],
        grid: [0x2020_2021, 0x2020_2022],
        geometry: core::array::from_fn(|index| 0x30 + index as u8),
        level: 0x4041,
        reference: 0x4243,
        decade: 0x4445,
        companion: 0x4647,
        setting: 0x5050_5051,
        profile: 0x5050_5052,
        lens: 0x5050_5053,
        lens_reference: 0x5050_5054,
        offset_mode: 2,
    }
}

#[test]
fn capture_source_request_names_the_capture_statistics_and_carries_the_offset_mode() {
    let request = SourceRequest {
        statistics: SourceStatistics::Capture,
        profile: [1, 2],
        decade: 3,
        profile_halfword: 4,
        level: 5,
        reference: 6,
        size: [7, 8],
        exposure: 9,
        offset_mode: 2,
        mode: 10,
        selection: 1,
        extension: 11,
    };
    let (_, message) = stock::calc_payload(&CalcPayload::Source { request, callback: CALLBACK });
    assert_eq!(&message.bytes()[4..8], &0x5ffb_6120u32.to_le_bytes());
    assert_eq!(&message.bytes()[44..48], &2u32.to_le_bytes());
}

#[test]
fn capture_first_request_carries_the_record_value_and_the_processing_extent() {
    let request = CaptureFirstRequest { at_0022: -2, processing: processing() };
    let (operation, message) = stock::calc_payload(&CalcPayload::CaptureFirst { request, callback: CALLBACK });
    assert_eq!(stock::calc_operation(operation), 0x3c0f);
    let expected: [u8; 44] = placed(&[
        (0, &CALLBACK.0.to_le_bytes()),
        (4, &(-2i16).to_le_bytes()),
        (8, &0x5ffb_6120u32.to_le_bytes()),
        (12, &0x4243u16.to_le_bytes()),
        (14, &0x4041u16.to_le_bytes()),
        (16, &0x1010_1011u32.to_le_bytes()),
        (20, &0x1010_1012u32.to_le_bytes()),
        (24, &0x5050_5052u32.to_le_bytes()),
    ]);
    assert_eq!(message.bytes(), &expected[..]);
}

#[test]
fn capture_third_request_names_three_capture_buffers() {
    let request = CaptureThirdRequest { scene: 9, scene_flags: 3, at_0022: 0x0607, processing: processing(), calibration: [-1000, 250], detail: 0xd0d0_d0d1 };
    let (operation, message) = stock::calc_payload(&CalcPayload::CaptureThird { request, callback: CALLBACK });
    assert_eq!(stock::calc_operation(operation), 0x3c0e);
    let expected: [u8; 60] = placed(&[
        (0, &CALLBACK.0.to_le_bytes()),
        (4, &9u32.to_le_bytes()),
        (8, &3u16.to_le_bytes()),
        (10, &0x0607u16.to_le_bytes()),
        (12, &0x5ffb_6120u32.to_le_bytes()),
        (16, &0x5ffb_7120u32.to_le_bytes()),
        (20, &0x5ffb_7130u32.to_le_bytes()),
        (24, &0x4243u16.to_le_bytes()),
        (28, &0x1010_1011u32.to_le_bytes()),
        (32, &0x1010_1012u32.to_le_bytes()),
        (36, &0x2020_2021u32.to_le_bytes()),
        (40, &0x2020_2022u32.to_le_bytes()),
        (44, &0x5050_5054u32.to_le_bytes()),
        (48, &0x5050_5052u32.to_le_bytes()),
        (52, &(-1000i16).to_le_bytes()),
        (54, &250i16.to_le_bytes()),
        (56, &0xd0d0_d0d1u32.to_le_bytes()),
    ]);
    assert_eq!(message.bytes(), &expected[..]);
}

#[test]
fn capture_second_request_places_record_measurement_and_processing_values() {
    let measurement = CaptureMeasurement {
        first: 0x0a01,
        second: 0x0a02,
        base: 0x0a03,
        third: 0x0a04,
        fourth: 0x0a05,
        slot: core::array::from_fn(|index| 0x60 + index as u8),
        fifth: 0x0a06,
        sixth: 0x0a07,
        paired: true,
        count: 3,
        block: core::array::from_fn(|index| 0x80 + index as u8),
        seventh: 0x0a08,
    };
    let request = CaptureSecondRequest {
        scene: 5,
        scene_class: 0,
        header_63: 1,
        scene_flags: 2,
        at_003e: 0x11,
        at_003f: 0x12,
        auxiliary: 0x13,
        at_0048: 0x14,
        at_0045: 0x15,
        profile_kind: 0x16,
        at_0022: 0x0b01,
        at_000e: [0x21, 0x22, 0x23, 0x24, 0x25, 0x26],
        at_002a: 0x0b02,
        at_00a0: 0x0b03,
        at_0092: 0x0b04,
        at_001a: 0x0b05,
        level_total: 0x0b06,
        lens_level: 0x0b07,
        offsets: [0x0c0c_0c01, 0x0c0c_0c02],
        at_01ec: 0x0c0c_0c03,
        at_017a: 0x0d01,
        at_017c: 0x0d02,
        at_017e: 0x0d03,
        at_0180: 0x0d04,
        at_0186: 0x0d05,
        at_0188: 0x0e0e_0e01,
        capture_offset: 0x0e0e_0e02,
        at_0164: 0x0e0e_0e03,
        at_0168: 0x0e0e_0e04,
        at_01f4: 0x0e0e_0e05,
        processing: processing(),
        window_measured: true,
        measurement,
        detail: 0x0f0f_0f01,
    };
    let (operation, message) = stock::calc_payload(&CalcPayload::CaptureSecond { request, callback: CALLBACK });
    assert_eq!(stock::calc_operation(operation), 0x3c10);
    let expected: [u8; 224] = placed(&[
        (0, &CALLBACK.0.to_le_bytes()),
        (8, &5u32.to_le_bytes()),
        (12, &0x11u32.to_le_bytes()),
        (16, &0x12u32.to_le_bytes()),
        (20, &0x13u32.to_le_bytes()),
        (24, &0x0b01u16.to_le_bytes()),
        (28, &0x14u32.to_le_bytes()),
        (32, &[0x21, 0x22, 0x23, 0x24, 0x25, 0x26]),
        (40, &0x15u32.to_le_bytes()),
        (44, &0x0b02u32.to_le_bytes()),
        (48, &2u16.to_le_bytes()),
        (50, &0x0b03u16.to_le_bytes()),
        (52, &0x0b04u16.to_le_bytes()),
        (54, &0x0a01u16.to_le_bytes()),
        (56, &0x0a02u16.to_le_bytes()),
        (58, &0x0b06u16.to_le_bytes()),
        (60, &0x0b07u16.to_le_bytes()),
        (64, &0x0c0c_0c01u32.to_le_bytes()),
        (68, &0x0c0c_0c02u32.to_le_bytes()),
        (72, &2u32.to_le_bytes()),
        (76, &0x0c0c_0c03u32.to_le_bytes()),
        (80, &0x0d01u16.to_le_bytes()),
        (82, &0x0d02u16.to_le_bytes()),
        (84, &0x0d03u16.to_le_bytes()),
        (86, &0x0d04u16.to_le_bytes()),
        (88, &0x0d05u16.to_le_bytes()),
        (90, &0x4647u16.to_le_bytes()),
        (92, &0x0e0e_0e01u32.to_le_bytes()),
        (96, &0x0e0e_0e02u32.to_le_bytes()),
        (100, &0x0e0e_0e03u32.to_le_bytes()),
        (104, &0x0e0e_0e04u32.to_le_bytes()),
        (108, &0x5ffb_6120u32.to_le_bytes()),
        (112, &0x4243u16.to_le_bytes()),
        (114, &0x4041u16.to_le_bytes()),
        (116, &0x1010_1011u32.to_le_bytes()),
        (120, &0x1010_1012u32.to_le_bytes()),
        (124, &0x5050_5054u32.to_le_bytes()),
        (128, &0x5050_5053u32.to_le_bytes()),
        (132, &0x5050_5052u32.to_le_bytes()),
        (136, &0x0e0e_0e05u32.to_le_bytes()),
        (140, &1u32.to_le_bytes()),
        (152, &0x0a03u16.to_le_bytes()),
        (154, &0x0a04u16.to_le_bytes()),
        (156, &0x0a05u16.to_le_bytes()),
        (158, &measurement.slot),
        (170, &0x0a06u16.to_le_bytes()),
        (172, &1u32.to_le_bytes()),
        (176, &0x16u32.to_le_bytes()),
        (180, &0x0a07u16.to_le_bytes()),
        (182, &[3]),
        (184, &measurement.block),
        (208, &0x0b05u16.to_le_bytes()),
        (210, &0x0a08u16.to_le_bytes()),
        (212, &1u32.to_le_bytes()),
        (216, &0u32.to_le_bytes()),
        (220, &0x0f0f_0f01u32.to_le_bytes()),
    ]);
    assert_eq!(message.bytes(), &expected[..]);
}

#[test]
fn area_evaluation_carries_the_pairs_and_shares_of_the_chosen_list_only() {
    let geometry = AreaGeometry {
        active: true,
        list: Some(crate::area_measurement::EvaluatedList::Second),
        settings: [0x71, 0x72, 0x73],
        count: 2,
        shares: [40, 50, 60],
        scale: 0x7475,
        list_value: 0x7676_7677,
        area_count: 1,
        accepted: [true, false, true],
        span: 0x7879,
        shift: -3,
        record_value: 0x7a7a_7a7b,
        within_limit: true,
        slot_rate: 0x7c7c_7c7d,
        ..AreaGeometry::default()
    };
    let request = AreaEvaluationRequest {
        scene: 0x11,
        profile_kind: 0x12,
        header_47: 0x13,
        at_001a: 0x0b01,
        at_0022: 0x0b02,
        at_0045: 0x14,
        at_004b: 0x15,
        at_0048: 0x16,
        at_0041: 0x17,
        at_01b4: 0x0c0c_0c01,
        at_01f2: 0x0b03,
        level_total: 0x0b04,
        at_017c: 0x0b05,
        at_0186: 0x0b06,
        at_0170: 0x0c0c_0c02,
        at_0174: 0x0b07,
        at_0176: 0x0b08,
        at_0188: 0x0c0c_0c03,
        at_01f0: 0x0b09,
        frames: [0x0d0d_0d01, 0x0d0d_0d02],
        lens_level: 0x0b0a,
        level_offset: -7,
        processing: processing(),
        geometry,
        scene_flags: 1,
        lens_temperature: -5,
    };
    let callbacks = [StockCallbackWord(0x00f2_acd0), StockCallbackWord(0x00f2_ac98)];
    let (operation, message) = stock::calc_payload(&CalcPayload::AreaEvaluation { request, callbacks });
    assert_eq!(stock::calc_operation(operation), 0x3c0d);
    let expected: [u8; 220] = placed(&[
        (0, &0x00f2_acd0u32.to_le_bytes()),
        (4, &0x00f2_ac98u32.to_le_bytes()),
        (8, &0x11u32.to_le_bytes()),
        (12, &0x12u32.to_le_bytes()),
        (16, &0x13u32.to_le_bytes()),
        (20, &0x0b01u16.to_le_bytes()),
        (22, &0x0b02u16.to_le_bytes()),
        (24, &0x14u32.to_le_bytes()),
        (28, &0x15u32.to_le_bytes()),
        (32, &0x16u32.to_le_bytes()),
        (36, &0x17u32.to_le_bytes()),
        (40, &0x7a7a_7a7bu32.to_le_bytes()),
        (44, &0x0c0c_0c01u32.to_le_bytes()),
        (48, &0x0b03u16.to_le_bytes()),
        (50, &0x0b04u16.to_le_bytes()),
        (52, &0x0b05u16.to_le_bytes()),
        (54, &0x0b06u16.to_le_bytes()),
        (56, &0x7c7c_7c7du32.to_le_bytes()),
        (60, &0x0c0c_0c02u32.to_le_bytes()),
        (64, &0x0b07u16.to_le_bytes()),
        (66, &0x0b08u16.to_le_bytes()),
        (68, &0x0c0c_0c03u32.to_le_bytes()),
        (72, &0x0b09u16.to_le_bytes()),
        (76, &0x5ffb_6120u32.to_le_bytes()),
        (80, &0x5ffb_71d0u32.to_le_bytes()),
        (84, &0x0d0d_0d02u32.to_le_bytes()),
        (88, &0x0b0au16.to_le_bytes()),
        (90, &0x4243u16.to_le_bytes()),
        (92, &0x4041u16.to_le_bytes()),
        (94, &(-7i16).to_le_bytes()),
        (96, &0x1010_1011u32.to_le_bytes()),
        (100, &0x1010_1012u32.to_le_bytes()),
        (104, &processing().geometry),
        (128, &0x5050_5054u32.to_le_bytes()),
        (132, &2u32.to_le_bytes()),
        (136, &2u32.to_le_bytes()),
        (140, &[0x71, 0x72, 0x73, 2]),
        (144, &[0xff; 8]),
        (168, &40u32.to_le_bytes()),
        (172, &50u32.to_le_bytes()),
        (180, &0x7475u16.to_le_bytes()),
        (184, &0x7676_7677u32.to_le_bytes()),
        (188, &1u32.to_le_bytes()),
        (192, &1u32.to_le_bytes()),
        (200, &1u32.to_le_bytes()),
        (204, &0x7879u16.to_le_bytes()),
        (206, &(-3i16).to_le_bytes()),
        (208, &1u32.to_le_bytes()),
        (212, &1u16.to_le_bytes()),
        (214, &[0xfb]),
        (216, &0x0d0d_0d01u32.to_le_bytes()),
    ]);
    assert_eq!(message.bytes(), &expected[..]);
}

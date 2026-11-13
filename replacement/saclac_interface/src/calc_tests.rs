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
}

#[test]
fn calc1_records_precede_the_callback() {
    for (payload, operation, length, first) in [
        (Calc1Payload::Acquisition { record: counting::<168>(1), callback: CALLBACK }, Calc1Operation::Request00, 172, 1),
        (Calc1Payload::Selection { record: counting::<228>(2), callback: CALLBACK }, Calc1Operation::Request01, 232, 2),
        (Calc1Payload::Parameters { record: counting::<124>(3), callback: CALLBACK }, Calc1Operation::Request02, 128, 3),
    ] {
        let (posted, message) = stock::calc1_payload(&payload);
        assert_eq!((posted, message.bytes().len(), message.bytes()[0]), (operation, length, first));
        assert_eq!(&message.bytes()[length - 4..], &CALLBACK.0.to_le_bytes());
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
        profile: [0x0102_0304, 0x0506_0708],
        decade: 0x090a,
        profile_halfword: 0x0b0c,
        level: 0x0d0e,
        reference: 0x0f10,
        size: [0x1112_1314, 0x1516_1718],
        exposure: 0x191a_1b1c,
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
    };
    let (operation, message) = stock::calc_payload(&CalcPayload::Window { request, callback: CALLBACK });
    assert_eq!(operation, CalcOperation::Request0c);
    assert_eq!(message.bytes(), &[
        2, 0, 0, 0, 0x44, 0x43, 0x42, 0x41, 0x22, 0x21, 0, 0, 4, 3, 2, 1,
        0x15, 0, 0, 0, 9, 8, 7, 6, 0x0b, 0x0a, 0x32, 0x31, 0, 0, 0, 0,
        0x62, 0x61, 0, 0, 0xb4, 0xc1, 0xfe, 0x5f, 0, 0x10, 0, 0, 0x14, 0x13, 0x12, 0x11,
        2, 0, 0, 0, 0x54, 0x53, 0x52, 0x51, 0, 0, 0, 0, 0, 0, 0, 0,
        1, 0, 0, 0, 5, 0, 0, 0, 1, 0, 0, 0, 0x04, 0xc1, 0xf3, 0,
    ][..]);
}

#[test]
fn release_posts_nothing_under_its_own_code() {
    let (operation, message) = stock::calc_payload(&CalcPayload::Release);
    assert_eq!(operation, CalcOperation::Request12);
    assert!(message.bytes().is_empty());
    assert_eq!(stock::calc_operation(operation), 0x3c12);
}

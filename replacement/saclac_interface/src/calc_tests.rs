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
fn source_record_moves_its_late_word_ahead_of_halfword_fourteen() {
    let record = counting::<48>(0);
    let (operation, message) = stock::calc_payload(&CalcPayload::Source { record, callback: CALLBACK });
    assert_eq!(operation, CalcOperation::Request11);
    let mut expected = [0u8; 56];
    expected[..4].copy_from_slice(&CALLBACK.0.to_le_bytes());
    expected[4..18].copy_from_slice(&record.0[..14]);
    expected[20..24].copy_from_slice(&record.0[40..44]);
    expected[24..26].copy_from_slice(&record.0[14..16]);
    expected[28..52].copy_from_slice(&record.0[16..40]);
    expected[52..56].copy_from_slice(&record.0[44..48]);
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
        adjustment: [0x7172_7374, 1],
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
}

#[test]
fn release_posts_nothing_under_its_own_code() {
    let (operation, message) = stock::calc_payload(&CalcPayload::Release);
    assert_eq!(operation, CalcOperation::Request12);
    assert!(message.bytes().is_empty());
    assert_eq!(stock::calc_operation(operation), 0x3c12);
}

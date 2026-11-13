use crate::imgdc::{ImgDcMessage, ImgDcPayload, Record};
use crate::stock::{self, StockCallbackWord};
use crate::ImgDcOperation;

const CALLBACK: StockCallbackWord = StockCallbackWord(0x00f2_f7a0);

fn counting<const N: usize>(first: u8) -> Record<N> {
    let mut bytes = [0; N];
    for (index, byte) in bytes.iter_mut().enumerate() { *byte = first.wrapping_add(index as u8); }
    Record(bytes)
}

fn bytes<'a>(message: &'a ImgDcMessage<'_>) -> &'a [u8] {
    match message {
        ImgDcMessage::Bytes(encoded) => encoded.bytes(),
        ImgDcMessage::Block { .. } => panic!("a block request posts no bytes"),
    }
}

#[test]
fn selection_posts_its_four_arguments_in_order() {
    let payload = ImgDcPayload::Selection {
        callback: CALLBACK,
        first: 0x1122_3344,
        second: 0x5566_7788,
        third: 0x99aa_bbcc,
    };
    let (operation, message) = stock::imgdc_payload(&payload);
    assert_eq!(operation, ImgDcOperation::Request16);
    let mut expected = [0; 16];
    expected[0..4].copy_from_slice(&CALLBACK.0.to_le_bytes());
    expected[4..8].copy_from_slice(&0x1122_3344u32.to_le_bytes());
    expected[8..12].copy_from_slice(&0x5566_7788u32.to_le_bytes());
    expected[12..16].copy_from_slice(&0x99aa_bbccu32.to_le_bytes());
    assert_eq!(bytes(&message), &expected[..]);
}

#[test]
fn scan_record_follows_the_callback() {
    let record = counting::<228>(1);
    let payload = ImgDcPayload::Scan { callback: CALLBACK, record };
    let (operation, message) = stock::imgdc_payload(&payload);
    assert_eq!(operation, ImgDcOperation::Request26);
    let posted = bytes(&message);
    assert_eq!(posted.len(), 232);
    assert_eq!(&posted[..4], &CALLBACK.0.to_le_bytes());
    assert_eq!(&posted[4..], &record.0);
}

#[test]
fn single_word_requests_carry_only_the_callback() {
    for (payload, operation) in [
        (ImgDcPayload::Request28 { callback: CALLBACK }, ImgDcOperation::Request28),
        (ImgDcPayload::Request22 { callback: CALLBACK }, ImgDcOperation::Request22),
        (ImgDcPayload::Request23 { callback: CALLBACK }, ImgDcOperation::Request23),
        (ImgDcPayload::Request43 { callback: CALLBACK }, ImgDcOperation::Request43),
    ] {
        let (posted, message) = stock::imgdc_payload(&payload);
        assert_eq!(posted, operation);
        assert_eq!(bytes(&message), &CALLBACK.0.to_le_bytes()[..]);
    }
}

#[test]
fn two_word_requests_place_their_value_after_the_callback() {
    for (payload, operation, value) in [
        (ImgDcPayload::Request29 { callback: CALLBACK, value: 0x0102_0304 }, ImgDcOperation::Request29, 0x0102_0304u32),
        (ImgDcPayload::Request2a { callback: CALLBACK, value: 0x0506_0708 }, ImgDcOperation::Request2a, 0x0506_0708u32),
    ] {
        let (posted, message) = stock::imgdc_payload(&payload);
        assert_eq!(posted, operation);
        let mut expected = [0; 8];
        expected[0..4].copy_from_slice(&CALLBACK.0.to_le_bytes());
        expected[4..8].copy_from_slice(&value.to_le_bytes());
        assert_eq!(bytes(&message), &expected[..]);
    }
}

#[test]
fn empty_requests_post_no_bytes() {
    for (payload, operation) in [
        (ImgDcPayload::Request2f, ImgDcOperation::Request2f),
        (ImgDcPayload::Request1b, ImgDcOperation::Request1b),
        (ImgDcPayload::Request21, ImgDcOperation::Request21),
        (ImgDcPayload::Request3b, ImgDcOperation::Request3b),
    ] {
        let (posted, message) = stock::imgdc_payload(&payload);
        assert_eq!(posted, operation);
        assert!(bytes(&message).is_empty());
    }
}

#[test]
fn request_2d_posts_its_record_whole() {
    let record = counting::<44>(7);
    let payload = ImgDcPayload::Request2d { record };
    let (operation, message) = stock::imgdc_payload(&payload);
    assert_eq!(operation, ImgDcOperation::Request2d);
    assert_eq!(bytes(&message), &record.0[..]);
}

#[test]
fn child_selection_places_the_record_then_three_callback_words() {
    let record = counting::<124>(9);
    let payload = ImgDcPayload::ChildSelection {
        record,
        callback: CALLBACK,
        first: StockCallbackWord(0x00f2_2928),
        second: StockCallbackWord(0x00f2_2794),
    };
    let (operation, message) = stock::imgdc_payload(&payload);
    assert_eq!(operation, ImgDcOperation::Request42);
    let posted = bytes(&message);
    assert_eq!(posted.len(), 136);
    assert_eq!(&posted[..124], &record.0);
    assert_eq!(&posted[124..128], &CALLBACK.0.to_le_bytes());
    assert_eq!(&posted[128..132], &0x00f2_2928u32.to_le_bytes());
    assert_eq!(&posted[132..136], &0x00f2_2794u32.to_le_bytes());
}

#[test]
fn request_44_leaves_bytes_six_to_eight_zero() {
    let payload = ImgDcPayload::Request44 {
        callback: CALLBACK,
        halfword: 0x1234,
        first: 1,
        second: 2,
        third: 3,
        fourth: 4,
    };
    let (operation, message) = stock::imgdc_payload(&payload);
    assert_eq!(operation, ImgDcOperation::Request44);
    let mut expected = [0xff; 24];
    expected[0..4].copy_from_slice(&CALLBACK.0.to_le_bytes());
    expected[4..6].copy_from_slice(&0x1234u16.to_le_bytes());
    expected[6..8].copy_from_slice(&[0, 0]);
    for (index, value) in [1u32, 2, 3, 4].into_iter().enumerate() {
        expected[8 + 4 * index..12 + 4 * index].copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(bytes(&message), &expected[..]);
}

#[test]
fn request_3a_leaves_bytes_nine_to_twelve_zero() {
    let payload = ImgDcPayload::Request3a {
        callback: CALLBACK,
        first: 0x11,
        second: 0x22,
        third: 0x33,
        fourth: 0x44,
        fifth: 0x55,
        sixth: 0x66,
        seventh: 0x77,
        eighth: 0x88,
        ninth: 0x99,
    };
    let (operation, message) = stock::imgdc_payload(&payload);
    assert_eq!(operation, ImgDcOperation::Request3a);
    let mut expected = [0xff; 40];
    expected[0..4].copy_from_slice(&CALLBACK.0.to_le_bytes());
    expected[4..8].copy_from_slice(&0x11u32.to_le_bytes());
    expected[8] = 0x22;
    expected[9..12].copy_from_slice(&[0, 0, 0]);
    for (index, value) in [0x33u32, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99].into_iter().enumerate() {
        expected[12 + 4 * index..16 + 4 * index].copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(bytes(&message), &expected[..]);
}

#[test]
fn record_requests_name_the_stock_block_and_post_no_bytes() {
    let record = counting::<296>(3);
    let payload = ImgDcPayload::Request2c { record };
    let (operation, message) = stock::imgdc_payload(&payload);
    assert_eq!(operation, ImgDcOperation::Request2c);
    assert!(matches!(
        message,
        ImgDcMessage::Block { address: 0x016f_4488, capacity: 296, record: posted } if posted == &record.0[..]
    ));

    let record = counting::<80>(5);
    let payload = ImgDcPayload::Request45 { record };
    let (operation, message) = stock::imgdc_payload(&payload);
    assert_eq!(operation, ImgDcOperation::Request45);
    assert!(matches!(
        message,
        ImgDcMessage::Block { address: 0x016f_224c, capacity: 80, record: posted } if posted == &record.0[..]
    ));
}

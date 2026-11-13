use crate::statistics::{Record, StatisticsPayload};
use crate::stock::{self, StockCallbackWord};
use crate::StatisticsOperation;

const CALLBACK: StockCallbackWord = StockCallbackWord(0x00f3_a8d4);

fn counting<const N: usize>(first: u8) -> Record<N> {
    let mut bytes = [0; N];
    for (index, byte) in bytes.iter_mut().enumerate() { *byte = first.wrapping_add(index as u8); }
    Record(bytes)
}

#[test]
fn request_0b_posts_one_word() {
    let (operation, message) = stock::statistics::payload(&StatisticsPayload::Request0b { value: 0x1122_3344 });
    assert_eq!(operation, StatisticsOperation::Request0b);
    assert_eq!(message.bytes(), &0x1122_3344u32.to_le_bytes()[..]);
}

#[test]
fn request_0e_places_the_record_before_the_callback() {
    let record = counting::<28>(1);
    let (operation, message) = stock::statistics::payload(&StatisticsPayload::Request0e {
        record,
        callback: StockCallbackWord(0x00f3_a8fc),
    });
    assert_eq!(operation, StatisticsOperation::Request0e);
    let posted = message.bytes();
    assert_eq!(posted.len(), 32);
    assert_eq!(&posted[..28], &record.0);
    assert_eq!(&posted[28..], &0x00f3_a8fcu32.to_le_bytes());
}

#[test]
fn request_0f_posts_only_the_callback() {
    let (operation, message) = stock::statistics::payload(&StatisticsPayload::Request0f { callback: CALLBACK });
    assert_eq!(operation, StatisticsOperation::Request0f);
    assert_eq!(message.bytes(), &CALLBACK.0.to_le_bytes()[..]);
}

#[test]
fn request_43_posts_no_bytes() {
    let (operation, message) = stock::statistics::payload(&StatisticsPayload::Request43);
    assert_eq!(operation, StatisticsOperation::Request43);
    assert!(message.bytes().is_empty());
}

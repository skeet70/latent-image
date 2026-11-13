use crate::capture_results::*;
use crate::stock::capture_results::*;

#[test]
fn the_readout_takes_its_bytes_and_words_from_their_payload_offsets() {
    let mut payload = [0u8; 0x24];
    payload[4] = 7;
    payload[5] = 9;
    payload[0x1c..0x20].copy_from_slice(&3u32.to_le_bytes());
    payload[0x20..0x24].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    assert_eq!(readout(&payload), Some(ReadoutPayload { source: 7, variant: 9, mode: 3, value: 0x1234_5678 }));
    assert_eq!(readout(&payload[..0x23]), None);
}

#[test]
fn job_results_take_their_leading_bytes() {
    let payload: [u8; 48] = core::array::from_fn(|index| index as u8);
    assert_eq!(first_result(&payload).unwrap().last_word(), u32::from_le_bytes([8, 9, 10, 11]));
    assert_eq!(second_result(&payload), Some(SecondResult(payload)));
    assert_eq!(third_result(&payload), Some(ThirdResult(u32::from_le_bytes([0, 1, 2, 3]))));
    assert_eq!(fourth_result(&payload), Some(FourthResult { first: 0, second: u16::from_le_bytes([2, 3]), third: u16::from_le_bytes([4, 5]) }));
    assert_eq!(second_result(&payload[..47]), None);
}

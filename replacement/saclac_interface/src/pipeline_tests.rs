use crate::pipeline::*;
use crate::stock::pipeline::*;

#[test]
fn word_results_round_trip_through_their_payloads() {
    assert_eq!(raw_check(&raw_check_payload(RawCheck(0x0102_0304))), Some(RawCheck(0x0102_0304)));
    assert_eq!(tone_status(&tone_status_payload(ToneStatus(7))), Some(ToneStatus(7)));
    let progress = ToneProgress { status: 0x8000_0001, level: 0xfedc };
    assert_eq!(tone_progress_payload(progress), [1, 0, 0, 0x80, 0xdc, 0xfe]);
    assert_eq!(tone_progress(&tone_progress_payload(progress)), Some(progress));
    assert_eq!(tone_progress(&[0; 5]), None);
    assert_eq!(raw_check(&[0; 3]), None);
}

#[test]
fn calculation_results_take_their_leading_bytes() {
    let payload: [u8; 120] = core::array::from_fn(|index| index as u8);
    assert_eq!(raw_calculation(&payload).unwrap().0, payload[..20]);
    assert_eq!(tone_calculation(&payload).unwrap().0, payload[..108]);
    assert_eq!(gradation_calculation(&payload).unwrap().0, payload[..96]);
    assert_eq!(tone_calculation(&payload[..107]), None);
    assert_eq!(gradation_calculation(&payload[..95]), None);
    assert_eq!(raw_calculation(&payload[..19]), None);
}

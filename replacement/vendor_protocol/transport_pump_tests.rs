use super::*;

const ALL: [UsbEvent; 12] = UsbEvent::ORDER;

fn status(bits: &[u8]) -> UsbStatus {
    UsbStatus::new(bits.iter().fold(0u64, |word, bit| word | (1u64 << bit)), 0)
}

#[test]
fn the_shadow_mask_removes_its_bits_from_the_pending_word() {
    let pending = (1u64 << 33) | (1u64 << 3);
    let status = UsbStatus::new(pending, (1u64 << 3) | (1u64 << 40));
    assert!(!status.contains(UsbEvent::ChannelTwoRequest));
    assert!(status.contains(UsbEvent::ControlComplete));
    assert_eq!(status.bits(), 1u64 << 33);
}

#[test]
fn a_stopped_controller_produces_no_events_and_no_acknowledge() {
    assert_eq!(pump(status(&[46, 13]), PumpState { stopping: true }), Pumped::STOPPED);
    assert!(pump(status(&[46, 13]), PumpState::RUNNING).acknowledge().is_some());
}

#[test]
fn every_set_bit_is_reported_in_the_stock_check_order() {
    let pumped = pump(status(&[18, 2, 3, 38, 4, 1, 0, 13, 33, 31, 44, 46]), PumpState::RUNNING);
    assert_eq!(pumped.events().collect::<Vec<_>>(), ALL);
}

#[test]
fn the_acknowledge_clears_every_reported_bit_and_bit_eighteen() {
    let pumped = pump(status(&[46, 13]), PumpState::RUNNING);
    assert_eq!(pumped.acknowledge(), Some(Acknowledge { low: 0xdfff, middle: 0xfffb, high: 0xbfff }));
    let pumped = pump(UsbStatus::NONE, PumpState::RUNNING);
    assert_eq!(pumped.events().count(), 0);
    assert_eq!(pumped.acknowledge(), Some(Acknowledge { low: 0xffff, middle: 0xfffb, high: 0xffff }));
}

#[test]
fn a_masked_bit_produces_no_event() {
    let pending = UsbStatus::new(1u64 << 44, 1u64 << 44);
    assert!(pump(pending, PumpState::RUNNING).is_empty());
}

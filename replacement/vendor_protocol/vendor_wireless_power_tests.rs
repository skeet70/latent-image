use super::{PowerOutcome, PowerReply, RadioPowerHandshake, RadioTick, RegisterTransfer, SdioReply};

#[test]
fn ready_radio_restores_control_after_initial_delay() {
    let mut radio = RadioPowerHandshake::new();
    let disable = radio.transfer(RadioTick(10)).unwrap();
    assert_eq!(disable.command(), 53);
    assert_eq!(disable.argument(), 0x9000_0804);
    assert_eq!(disable.write_data(), Some([2, 0, 0, 0]));
    assert_eq!(radio.receive(disable.token, SdioReply::Written, RadioTick(10)), PowerReply::Applied);
    assert_eq!(radio.transfer(RadioTick(509)), None);
    let status = radio.transfer(RadioTick(510)).unwrap();
    assert_eq!(status.argument(), 0x1000_0004);
    radio.receive(status.token, SdioReply::Read([0, 0, 32, 0]), RadioTick(510));
    let interrupt = radio.transfer(RadioTick(510)).unwrap();
    assert_eq!(interrupt.argument(), 0x1000_2004);
    radio.receive(interrupt.token, SdioReply::Read([0; 4]), RadioTick(510));
    assert_eq!(radio.outcome(), None);
    let enable = radio.transfer(RadioTick(510)).unwrap();
    assert_eq!(enable.write_data(), Some([1, 0, 0, 0]));
    radio.receive(enable.token, SdioReply::Written, RadioTick(510));
    assert_eq!(radio.outcome(), Some(PowerOutcome::Ready));
    assert_eq!(radio.transfer(RadioTick(999)), None);
}

#[test]
fn nonready_status_is_polled_at_ten_tick_intervals() {
    let mut radio = RadioPowerHandshake::new();
    let transfer = radio.transfer(RadioTick(0)).unwrap();
    radio.receive(transfer.token, SdioReply::Written, RadioTick(0));
    let status = radio.transfer(RadioTick(500)).unwrap();
    radio.receive(status.token, SdioReply::Read([0; 4]), RadioTick(500));
    assert_eq!(radio.transfer(RadioTick(509)), None);
    let next = radio.transfer(RadioTick(510)).unwrap();
    assert_eq!(next.argument(), status.argument());
    assert_ne!(next.token, status.token);
    assert_eq!(radio.receive(status.token, SdioReply::Read([0, 0, 32, 0]), RadioTick(510)), PowerReply::Late);
    assert_eq!(radio.outcome(), None);
    assert_eq!(radio.transfer(RadioTick(510)), Some(next));
}

#[test]
fn cancellation_finishes_only_after_a_nonready_status() {
    let mut radio = RadioPowerHandshake::new();
    radio.cancel();
    let disable = radio.transfer(RadioTick(0)).unwrap();
    radio.receive(disable.token, SdioReply::Written, RadioTick(0));
    assert_eq!(radio.outcome(), None);
    let status = radio.transfer(RadioTick(500)).unwrap();
    radio.receive(status.token, SdioReply::Read([0; 4]), RadioTick(500));
    assert_eq!(radio.outcome(), Some(PowerOutcome::Cancelled));
    assert_eq!(PowerOutcome::Cancelled.driver_status(), 0xc000_00bb);
    assert_eq!(radio.transfer(RadioTick(500)), None);
    assert_eq!(radio.receive(status.token, SdioReply::Read([0, 0, 32, 0]), RadioTick(500)), PowerReply::Late);
}

#[test]
fn ready_status_takes_precedence_over_cancellation() {
    let mut radio = RadioPowerHandshake::new();
    let disable = radio.transfer(RadioTick(0)).unwrap();
    radio.receive(disable.token, SdioReply::Written, RadioTick(0));
    radio.cancel();
    let status = radio.transfer(RadioTick(500)).unwrap();
    radio.receive(status.token, SdioReply::Read([0, 0, 32, 0]), RadioTick(500));
    let interrupt = radio.transfer(RadioTick(500)).unwrap();
    radio.receive(interrupt.token, SdioReply::Read([0; 4]), RadioTick(500));
    let enable = radio.transfer(RadioTick(500)).unwrap();
    radio.receive(enable.token, SdioReply::Written, RadioTick(500));
    assert_eq!(radio.outcome(), Some(PowerOutcome::Ready));
}

#[test]
fn interrupt_words_are_read_when_interrupt_status_bit_zero_is_set() {
    let mut radio = RadioPowerHandshake::new();
    let disable = radio.transfer(RadioTick(0)).unwrap();
    radio.receive(disable.token, SdioReply::Written, RadioTick(0));
    let status = radio.transfer(RadioTick(500)).unwrap();
    radio.receive(status.token, SdioReply::Read([0, 0, 32, 0]), RadioTick(500));
    let interrupt = radio.transfer(RadioTick(500)).unwrap();
    radio.receive(interrupt.token, SdioReply::Read([1, 0, 0, 0]), RadioTick(500));
    let low = radio.transfer(RadioTick(500)).unwrap();
    assert_eq!(low.argument(), 0x1000_4004);
    radio.receive(low.token, SdioReply::Read([1, 2, 3, 4]), RadioTick(500));
    let high = radio.transfer(RadioTick(500)).unwrap();
    assert_eq!(high.argument(), 0x1000_4804);
    radio.receive(high.token, SdioReply::Read([5, 6, 7, 8]), RadioTick(500));
    assert_eq!(radio.interrupt_words(), [0x0403_0201, 0x0807_0605]);
    assert!(matches!(radio.transfer(RadioTick(500)).unwrap().operation, RegisterTransfer::WriteControl(1)));
}

#[test]
fn failed_read_preserves_nonready_status_and_does_not_complete() {
    let mut radio = RadioPowerHandshake::new();
    let disable = radio.transfer(RadioTick(0)).unwrap();
    radio.receive(disable.token, SdioReply::Failed, RadioTick(0));
    let status = radio.transfer(RadioTick(500)).unwrap();
    radio.receive(status.token, SdioReply::Failed, RadioTick(500));
    assert_eq!(radio.outcome(), None);
    assert_eq!(radio.transfer(RadioTick(509)), None);
    assert_eq!(radio.transfer(RadioTick(510)).unwrap().argument(), 0x1000_0004);
}

#[test]
fn delay_comparison_survives_timer_wrap() {
    let mut radio = RadioPowerHandshake::new();
    let disable = radio.transfer(RadioTick(u32::MAX - 100)).unwrap();
    radio.receive(disable.token, SdioReply::Written, RadioTick(u32::MAX - 100));
    assert_eq!(radio.transfer(RadioTick(398)), None);
    assert_eq!(radio.transfer(RadioTick(399)).unwrap().argument(), 0x1000_0004);
}

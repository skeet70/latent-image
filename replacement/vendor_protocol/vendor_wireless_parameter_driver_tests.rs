use super::{RadioParameterDriver, SdioWriteOutcome, TrafficClass, TransferReply, TransmitCredits, TransmitError};
use super::super::vendor_wireless_radio_program::{RadioChannel, RadioMode, RadioParameterProgram, RadioRate, RadioSelection};

fn program(selection: RadioSelection) -> RadioParameterProgram {
    RadioParameterProgram::new(RadioMode::Configured, selection, RadioChannel::from_abi(1).unwrap(), RadioRate(0), false)
}

#[test]
fn parameter_reaches_actual_command_fifo_bytes() {
    let mut driver = RadioParameterDriver::new(7);
    driver.configure(program(RadioSelection::Receive)).unwrap();
    let transfer = driver.transfer().unwrap();
    assert_eq!(transfer.command(), 53);
    assert_eq!(transfer.argument(), 0x9000_5814);
    assert_eq!(transfer.bytes(), &[20, 0, 0, 0x50, 1, 1, 8, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(driver.outcome(), None);
}

#[test]
fn completed_receive_program_exposes_success_without_a_spare_credit() {
    let mut driver = RadioParameterDriver::new(0);
    driver.configure(program(RadioSelection::Receive)).unwrap();
    for expected in [
        [1, 0, 0, 0, 0, 0, 0, 0],
        [18, 0, 0, 0, 0xe0, 0xcd, 0x24, 0],
        [1, 0, 0, 0, 2, 0, 0, 0],
    ] {
        let transfer = driver.transfer().unwrap();
        assert_eq!(&transfer.bytes()[12..], &expected);
        let identity = transfer.identity;
        assert_eq!(driver.receive(identity, SdioWriteOutcome::Written), TransferReply::Applied);
    }
    assert_eq!(driver.outcome(), Some(Ok(())));
    assert!(driver.transfer().is_none());
}

#[test]
fn transmission_waits_for_actual_returned_command_credits() {
    let mut driver = RadioParameterDriver::new(0);
    driver.configure(program(RadioSelection::Transmit)).unwrap();
    for _ in 0..4 {
        let identity = driver.transfer().unwrap().identity;
        driver.receive(identity, SdioWriteOutcome::Written);
    }
    assert!(driver.transfer().is_none());
    assert_eq!(driver.outcome(), None);
    driver.observe_credit_returns([0, 0, 0, 0, 3, 0, 0, 0]).unwrap();
    for _ in 0..3 {
        let identity = driver.transfer().unwrap().identity;
        driver.receive(identity, SdioWriteOutcome::Written);
    }
    assert_eq!(driver.outcome(), Some(Ok(())));
}

#[test]
fn physical_failure_remains_visible_without_changing_the_stock_logical_return() {
    let mut driver = RadioParameterDriver::new(0);
    driver.configure(program(RadioSelection::Receive)).unwrap();
    let identity = driver.transfer().unwrap().identity;
    driver.receive(identity, SdioWriteOutcome::Failed);
    assert_eq!(driver.last_physical_outcome(), Some(SdioWriteOutcome::Failed));
    assert_eq!(driver.outcome(), None);
    assert_eq!(&driver.transfer().unwrap().bytes()[12..], &[18, 0, 0, 0, 0xe0, 0xcd, 0x24, 0]);
    assert_eq!(driver.receive(identity, SdioWriteOutcome::Written), TransferReply::Late);
    assert_eq!(driver.last_physical_outcome(), Some(SdioWriteOutcome::Failed));
}

#[test]
fn cancelled_physical_completion_cannot_finish_a_new_request() {
    let mut driver = RadioParameterDriver::new(255);
    driver.configure(program(RadioSelection::Receive)).unwrap();
    let old = driver.transfer().unwrap().identity;
    driver.cancel();
    assert_eq!(driver.outcome(), Some(Err(TransmitError::Cancelled)));
    driver.configure(program(RadioSelection::Receive)).unwrap();
    let new = driver.transfer().unwrap();
    assert_eq!(new.bytes()[6], 1);
    let new_identity = new.identity;
    assert_ne!(new_identity, old);
    assert_eq!(driver.receive(old, SdioWriteOutcome::Written), TransferReply::Late);
    assert_eq!(driver.outcome(), None);
    assert_eq!(driver.transfer().unwrap().identity, new_identity);
}

#[test]
fn concurrent_configuration_does_not_replace_the_active_request() {
    let mut driver = RadioParameterDriver::new(0);
    driver.configure(program(RadioSelection::Receive)).unwrap();
    let active = driver.transfer().unwrap().identity;
    assert_eq!(driver.configure(program(RadioSelection::Transmit)), Err(TransmitError::Busy));
    assert_eq!(driver.transfer().unwrap().identity, active);
}

#[test]
fn invalid_credit_returns_preserve_the_previous_credit_bank() {
    let mut credits = TransmitCredits::new();
    assert!(credits.consume(TrafficClass::Command));
    assert_eq!(credits.observe_returns([0, 0, 0, 0, 2, 0, 0, 0]), Err(TransmitError::CreditOverflow));
    assert_eq!(credits.available(TrafficClass::Command), 3);
    assert_eq!(credits.observe_returns([0, 0, 0, 0, 1, 0, 0, 0]), Ok(true));
    assert_eq!(credits.available(TrafficClass::Command), 4);
    assert_eq!(credits.observe_returns([0; 8]), Ok(false));
}

#[test]
fn credit_return_uses_byte_wrapping_before_maximum_comparison() {
    let mut credits = TransmitCredits::new();
    assert_eq!(credits.observe_returns([0, 0, 0, 0, 255, 0, 0, 0]), Ok(true));
    assert_eq!(credits.available(TrafficClass::Command), 3);
    credits.reset();
    assert_eq!(credits.available(TrafficClass::Command), 4);
}

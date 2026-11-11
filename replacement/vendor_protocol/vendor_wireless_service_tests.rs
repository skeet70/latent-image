use super::{RadioTestCommand, RadioTestOrdering, ServiceCommandError};
use super::super::vendor_wireless_radio_program::{ParameterSelector, RadioProgramError};

fn parameters(entries: [u8; 4]) -> std::vec::Vec<(ParameterSelector, i32)> {
    RadioTestCommand::from_configuration_entries(entries).unwrap().configuration().program()
        .map(|parameter| (parameter.selector, parameter.value())).collect()
}

#[test]
fn transmitting_extended_rate_preserves_signed_rate_encoding() {
    use ParameterSelector::{ExtendedRate, Filter, Frequency, Operation, Rate, SignedRateMode};
    let command = RadioTestCommand::from_configuration_entries([1, 14, 0x8e, 0]).unwrap();
    assert_eq!(command.ordering(), RadioTestOrdering::ShutdownThenConfigure);
    assert_eq!(parameters([1, 14, 0x8e, 0]), [
        (Operation, 0), (Frequency, 2_484_000), (Filter, 0), (SignedRateMode, 2),
        (ExtendedRate, 1), (Rate, i32::MIN + 2), (Operation, 1),
    ]);
}

#[test]
fn low_rates_do_not_enable_extended_signed_mode() {
    use ParameterSelector::{ExtendedRate, Filter, Frequency, Operation, Rate, SignedRateMode};
    assert_eq!(parameters([1, 1, 0x8b, 0]), [
        (Operation, 0), (Frequency, 2_412_000), (Filter, 0), (SignedRateMode, 0),
        (ExtendedRate, 0), (Rate, 11), (Operation, 1),
    ]);
}

#[test]
fn receiving_program_precedes_lan_shutdown() {
    use ParameterSelector::{Frequency, Operation};
    let command = RadioTestCommand::from_configuration_entries([0, 6, 0xff, 0]).unwrap();
    assert_eq!(command.ordering(), RadioTestOrdering::ConfigureThenStopLan);
    assert_eq!(parameters([0, 6, 0xff, 0]), [(Operation, 0), (Frequency, 2_437_000), (Operation, 2)]);
}

#[test]
fn direct_receive_does_not_validate_unused_channel_or_rate() {
    assert_eq!(parameters([0, 255, 255, 1]), [(ParameterSelector::Operation, 0)]);
}

#[test]
fn direct_transmit_uses_channel_without_rate_parameters() {
    assert_eq!(parameters([1, 14, 255, 1]), [
        (ParameterSelector::Frequency, 2_484_000), (ParameterSelector::Operation, 5),
    ]);
}

#[test]
fn invalid_channel_preserves_reset_prefix_before_terminal_failure() {
    let command = RadioTestCommand::from_configuration_entries([1, 15, 0, 0]).unwrap();
    assert_eq!(command.ordering(), RadioTestOrdering::ShutdownThenConfigure);
    let mut program = command.configuration().program();
    assert_eq!(program.by_ref().map(|parameter| (parameter.selector, parameter.value())).collect::<std::vec::Vec<_>>(),
        [(ParameterSelector::Operation, 0)]);
    assert_eq!(program.terminal_error(), Some(RadioProgramError::InvalidChannel));
}

#[test]
fn unsupported_command_mode_precedes_selection_error() {
    assert_eq!(RadioTestCommand::from_configuration_entries([2, 1, 0, 2]), Err(ServiceCommandError::Mode));
    assert_eq!(RadioTestCommand::from_configuration_entries([2, 1, 0, 0]), Err(ServiceCommandError::Selection));
}

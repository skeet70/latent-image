use super::{CommandSequence, RadioChannel, RadioMode, RadioParameterProgram, RadioProgramError, RadioRate, RadioSelection};

#[test]
fn configured_receive_requests_idle_frequency_then_receive() {
    let program = RadioParameterProgram::new(RadioMode::Configured, RadioSelection::Receive, RadioChannel::from_abi(14).unwrap(), RadioRate(0), false);
    let bytes: std::vec::Vec<_> = program.map(|parameter| parameter.encode()).collect();
    assert_eq!(bytes, [
        [1, 0, 0, 0, 0, 0, 0, 0],
        [18, 0, 0, 0, 0x20, 0xe7, 0x25, 0],
        [1, 0, 0, 0, 2, 0, 0, 0],
    ]);
}

#[test]
fn configured_transmit_preserves_signed_rate_and_extended_flag() {
    let program = RadioParameterProgram::new(RadioMode::Configured, RadioSelection::Transmit, RadioChannel::from_abi(1).unwrap(), RadioRate(i32::MIN + 3), true);
    let bytes: std::vec::Vec<_> = program.map(|parameter| parameter.encode()).collect();
    assert_eq!(bytes, [
        [1, 0, 0, 0, 0, 0, 0, 0],
        [18, 0, 0, 0, 0xe0, 0xcd, 0x24, 0],
        [7, 0, 0, 0, 0, 0, 0, 0],
        [4, 0, 0, 0, 2, 0, 0, 0],
        [15, 0, 0, 0, 1, 0, 0, 0],
        [3, 0, 0, 0, 3, 0, 0, 128],
        [1, 0, 0, 0, 1, 0, 0, 0],
    ]);
}

#[test]
fn unsigned_rate_disables_both_extended_modes() {
    let program = RadioParameterProgram::new(RadioMode::Configured, RadioSelection::Transmit, RadioChannel::from_abi(6).unwrap(), RadioRate(11), true);
    let bytes: std::vec::Vec<_> = program.map(|parameter| parameter.encode()).collect();
    assert_eq!(bytes[3], [4, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(bytes[4], [15, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(bytes[5], [3, 0, 0, 0, 11, 0, 0, 0]);
}

#[test]
fn direct_receive_has_no_frequency_request() {
    let program = RadioParameterProgram::new(RadioMode::Direct, RadioSelection::Receive, RadioChannel::from_abi(13).unwrap(), RadioRate(0), false);
    assert_eq!(program.map(|parameter| parameter.encode()).collect::<std::vec::Vec<_>>(), [[1, 0, 0, 0, 0, 0, 0, 0]]);
}

#[test]
fn direct_transmit_uses_operation_five() {
    let program = RadioParameterProgram::new(RadioMode::Direct, RadioSelection::Transmit, RadioChannel::from_abi(1).unwrap(), RadioRate(0), false);
    assert_eq!(program.map(|parameter| parameter.encode()).collect::<std::vec::Vec<_>>(), [
        [18, 0, 0, 0, 0xe0, 0xcd, 0x24, 0],
        [1, 0, 0, 0, 5, 0, 0, 0],
    ]);
}

#[test]
fn command_packing_preserves_unassigned_driver_header_bytes_and_wraps_sequence() {
    let mut program = RadioParameterProgram::new(RadioMode::Direct, RadioSelection::Receive, RadioChannel::from_abi(1).unwrap(), RadioRate(0), false);
    let parameter = program.next().unwrap();
    let mut sequence = CommandSequence::from_last_issued(255);
    let mut bytes = [0xa5; 20];
    parameter.write_driver_command(sequence.issue_next(), &mut bytes);
    assert_eq!(bytes, [20, 0, 0xa5, 0xa5, 1, 1, 0, 0xa5, 1, 0xa5, 0xa5, 0xa5, 1, 0, 0, 0, 0, 0, 0, 0]);
    parameter.write_driver_command(sequence.issue_next(), &mut bytes);
    assert_eq!(bytes[6], 1);
}

#[test]
fn channel_boundary_preserves_signed_arithmetic() {
    assert_eq!(RadioChannel::from_abi(15), Err(RadioProgramError::InvalidChannel));
    assert_eq!(RadioChannel::from_abi(0).unwrap().frequency_khz(), 2_407_000);
    assert_eq!(RadioChannel::from_abi(13).unwrap().frequency_khz(), 2_472_000);
    assert_eq!(RadioChannel::from_abi(14).unwrap().frequency_khz(), 2_484_000);
}

#[test]
fn rejected_configured_channel_retains_idle_prefix() {
    let mut program = RadioParameterProgram::with_channel_request(RadioMode::Configured,
        RadioSelection::Transmit, super::RadioChannelRequest::from_abi(15), RadioRate(1), false);
    assert_eq!(program.next().unwrap().encode(), [1, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(program.next(), None);
    assert_eq!(program.terminal_error(), Some(RadioProgramError::InvalidChannel));
}

#[test]
fn direct_receive_does_not_validate_unused_channel() {
    let mut program = RadioParameterProgram::with_channel_request(RadioMode::Direct,
        RadioSelection::Receive, super::RadioChannelRequest::from_abi(15), RadioRate(1), false);
    assert_eq!(program.next().unwrap().encode(), [1, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(program.next(), None);
    assert_eq!(program.terminal_error(), None);
}

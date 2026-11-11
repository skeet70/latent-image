use super::*;
use super::super::packet::CommandPacket;

fn state() -> PtpState {
    PtpState {
        session_id: None,
        transaction: TransactionId(0),
        response: ResponseCode(0x2001),
    }
}

fn command(operation: u16, parameters: [u32; 5]) -> CommandPacket {
    CommandPacket {
        operation: OperationCode(operation),
        transaction: TransactionId(42),
        parameters,
        parameter_count: 5,
    }
}

fn execute(
    operations: &mut RemoteSessionOperations,
    state: &mut PtpState,
    output: &mut PtpOutput,
    phase: PtpPhase,
    command: CommandPacket,
) -> OperationReply {
    operations.execute(OperationRequest { phase, command, data: &[] }, state, output).unwrap()
}

fn acknowledge(output: &mut PtpOutput) {
    let size = output.queued_bytes().len();
    output.acknowledge(size).unwrap();
}

#[test]
fn device_information_is_empty_remote_data_without_a_session() {
    let mut operations = RemoteSessionOperations::new();
    let mut state = state();
    let mut output = PtpOutput::new();
    let command = command(0x1001, [0; 5]);
    let result = execute(&mut operations, &mut state, &mut output, PtpPhase::Command, command);
    assert_eq!(result.response.0, 0x2001);
    assert_eq!(output.queued_bytes(), &[12, 0, 0, 0, 2, 0, 1, 16, 42, 0, 0, 0]);
    acknowledge(&mut output);
    execute(&mut operations, &mut state, &mut output, PtpPhase::Data, command);
    assert_eq!(output.queued_bytes(), &[12, 0, 0, 0, 3, 0, 1, 32, 42, 0, 0, 0]);
    assert!(state.session_id.is_none());
}

#[test]
fn open_session_error_precedence_and_close_preservation() {
    let mut operations = RemoteSessionOperations::new();
    let mut state = state();
    let mut output = PtpOutput::new();
    for (parameters, expected) in [([0, 1, 0, 0, 0], 0x2006), ([0; 5], 0x201d), ([77, 0, 0, 0, 0], 0x2001), ([0; 5], 0x201d), ([88, 0, 0, 0, 0], 0x201e)] {
        let result = execute(&mut operations, &mut state, &mut output, PtpPhase::Command, command(0x1002, parameters));
        assert_eq!(result.response.0, expected);
        acknowledge(&mut output);
    }
    assert_eq!(state.session_id.as_ref().map(|id| id.0), Some(77));
    let result = execute(&mut operations, &mut state, &mut output, PtpPhase::Command, command(0x1003, [1, 0, 0, 0, 0]));
    assert_eq!(result.response.0, 0x2006);
    assert_eq!(state.session_id.as_ref().map(|id| id.0), Some(77));
    acknowledge(&mut output);
    let result = execute(&mut operations, &mut state, &mut output, PtpPhase::Command, command(0x1003, [0; 5]));
    assert_eq!(result.response.0, 0x2001);
    assert!(state.session_id.is_none());
    acknowledge(&mut output);
    let result = execute(&mut operations, &mut state, &mut output, PtpPhase::Command, command(0x1003, [1; 5]));
    assert_eq!(result.response.0, 0x2003);
}

#[test]
fn storage_list_exposes_the_receive_catalog_not_physical_media() {
    let mut operations = RemoteSessionOperations::new();
    let mut state = state();
    state.session_id = Some(SessionIdentifier(9));
    let mut output = PtpOutput::new();
    execute(&mut operations, &mut state, &mut output, PtpPhase::Command, command(0x1004, [0; 5]));
    assert_eq!(output.queued_bytes(), &[20, 0, 0, 0, 2, 0, 4, 16, 42, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 16]);
}

#[test]
fn storage_information_error_precedence_survives_data_completion() {
    for (session, parameters, expected) in [
        (false, [7; 5], 0x2003),
        (true, [0, 1, 0, 0, 0], 0x2008),
        (true, [0xfffffffe, 1, 0, 0, 0], 0x2008),
        (true, [0x10000002, 1, 0, 0, 0], 0x2013),
        (true, [0x10000001, 1, 0, 0, 0], 0x2006),
        (true, [0x10000001, 0, 0, 0, 0], 0x2001),
        (true, [0xffffffff, 0, 0, 0, 0], 0x2001),
    ] {
        let mut operations = RemoteSessionOperations::new();
        let mut state = state();
        state.session_id = session.then_some(SessionIdentifier(1));
        let mut output = PtpOutput::new();
        let command = command(0x1005, parameters);
        let first = execute(&mut operations, &mut state, &mut output, PtpPhase::Command, command);
        assert_eq!(first.response.0, expected);
        assert_eq!(output.queued_bytes(), &[12, 0, 0, 0, 2, 0, 5, 16, 42, 0, 0, 0]);
        acknowledge(&mut output);
        let second = execute(&mut operations, &mut state, &mut output, PtpPhase::Data, command);
        assert_eq!(second.response.0, expected);
        assert_eq!(output.queued_bytes()[4..], [3, 0, expected as u8, (expected >> 8) as u8, 42, 0, 0, 0]);
    }
}

#[test]
fn teardown_discards_session_and_pending_transaction() {
    let mut operations = RemoteSessionOperations::new();
    let mut state = state();
    state.session_id = Some(SessionIdentifier(9));
    let mut output = PtpOutput::new();
    let command = command(0x1001, [0; 5]);
    execute(&mut operations, &mut state, &mut output, PtpPhase::Command, command);
    acknowledge(&mut output);
    operations.teardown(&mut state);
    assert!(state.session_id.is_none());
    assert!(matches!(operations.execute(OperationRequest { phase: PtpPhase::Data, command, data: &[] }, &mut state, &mut output), Err(PtpError::InvalidState)));
    assert!(output.is_empty());
}

#[test]
fn noncommand_phase_keeps_the_saved_response_and_transaction() {
    let mut operations = RemoteSessionOperations::new();
    let mut state = state();
    let mut output = PtpOutput::new();
    let original = command(0x1001, [1, 0, 0, 0, 0]);
    execute(&mut operations, &mut state, &mut output, PtpPhase::Command, original);
    acknowledge(&mut output);
    let mut unrelated = original;
    unrelated.transaction = TransactionId(43);
    assert!(matches!(operations.execute(OperationRequest { phase: PtpPhase::Cancel, command: unrelated, data: &[] }, &mut state, &mut output), Err(PtpError::InvalidState)));
    assert!(output.is_empty());
    let result = execute(&mut operations, &mut state, &mut output, PtpPhase::Cancel, original);
    assert_eq!(result.response.0, 0x2006);
    assert_eq!(output.queued_bytes(), &[12, 0, 0, 0, 3, 0, 6, 32, 42, 0, 0, 0]);
}

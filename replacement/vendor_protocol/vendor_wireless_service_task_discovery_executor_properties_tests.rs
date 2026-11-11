use super::*;
use super::super::executor::SessionIdentifier;
use super::super::packet::CommandPacket;
use super::super::super::interface_modes::RemoteSelection;

fn request(operation: u16, phase: PtpPhase, parameters: &[u32]) -> OperationRequest<'static> {
    let mut values = [0; 5];
    values[..parameters.len()].copy_from_slice(parameters);
    OperationRequest { phase, command: CommandPacket { operation: OperationCode(operation), transaction: TransactionId(7), parameters: values, parameter_count: parameters.len() as u8 }, data: &[] }
}
fn session() -> PtpState { let mut state = PtpState::new(); state.session_id = Some(SessionIdentifier(1)); state }

#[test]
fn object_property_support_uses_target_format_list() {
    let mut operations = RemotePropertyOperations::new();
    let mut state = session();
    let mut output = PtpOutput::new();
    let mut control = RemoteControl::new();
    operations.execute(request(0x9801, PtpPhase::Command, &[0x3001]), &mut state, &mut output, &mut control).unwrap();
    assert_eq!(&output.queued_bytes()[12..], &[8,0,0,0,1,220,2,220,3,220,4,220,7,220,11,220,65,220,68,220]);
    let length = output.queued_bytes().len();
    output.acknowledge(length).unwrap();
    operations.execute(request(0x9801, PtpPhase::Data, &[0x3001]), &mut state, &mut output, &mut control).unwrap();
    assert_eq!(&output.queued_bytes()[6..8], &0x2001u16.to_le_bytes());
}

#[test]
fn descriptor_reports_writability_default_group_and_form() {
    let descriptor = schema::ObjectPropertyDescriptor::for_property(schema::ObjectPropertyId::De94);
    let mut bytes = [0; 64];
    let length = descriptor.encode(&mut bytes).unwrap();
    assert_eq!(&bytes[..length], &[0x94,0xde,4,0,1,0,0,2,0,0,0,2,3,0,0,0,1,0,2,0]);
}

#[test]
fn descriptor_rejects_unknown_property_before_unknown_format() {
    let mut operations = RemotePropertyOperations::new();
    let mut state = session();
    let mut output = PtpOutput::new();
    let mut control = RemoteControl::new();
    let reply = operations.execute(request(0x9802, PtpPhase::Command, &[0xdc0a,0xffff]), &mut state, &mut output, &mut control).unwrap();
    assert_eq!(reply.response, ResponseCode(0xa801));
}

#[test]
fn device_descriptor_with_null_provider_does_not_publish_success() {
    let mut operations = RemotePropertyOperations::new();
    let mut state = session();
    let mut output = PtpOutput::new();
    let mut control = RemoteControl::new();
    operations.execute(request(0x1014, PtpPhase::Command, &[0xd406]), &mut state, &mut output, &mut control).unwrap();
    assert!(output.queued_bytes().is_empty());
    assert!(operations.pending());
    operations.teardown();
    assert!(!operations.pending());
}

#[test]
fn property_notifications_have_halfword_count_and_six_byte_entries() {
    let mut operations = RemotePropertyOperations::new();
    let mut state = session();
    let mut output = PtpOutput::new();
    let mut control = RemoteControl::new();
    control.select(RemoteSelection::Configure);
    control.publish_count(0x12345678);
    operations.execute(request(0x1015, PtpPhase::Command, &[0xd212]), &mut state, &mut output, &mut control).unwrap();
    assert_eq!(&output.queued_bytes()[12..], &[2,0,0,0xdf,1,0,0,0,0x22,0xd2,0x78,0x56,0x34,0x12]);
    let length = output.queued_bytes().len();
    output.acknowledge(length).unwrap();
    operations.execute(request(0x1015, PtpPhase::Data, &[0xd212]), &mut state, &mut output, &mut control).unwrap();
    let length = output.queued_bytes().len();
    output.acknowledge(length).unwrap();
    operations.execute(request(0x1015, PtpPhase::Command, &[0xd212]), &mut state, &mut output, &mut control).unwrap();
    assert_eq!(&output.queued_bytes()[12..], &[0,0]);
}

#[test]
fn rejected_minimum_request_remains_observable_but_advertised_minimum_is_constant() {
    let mut values = RemotePropertyValues::new();
    let control = RemoteControl::new();
    assert_eq!(values.write(PropertyIdentifier(0xdf22), &[1,0,0,0], &control), PropertyStatus::InvalidValue);
    assert_eq!(values.minimum_requests()[1], MinimumRequest(1));
    let mut bytes = [0; 4];
    assert_eq!(values.read(PropertyIdentifier(0xdf22), &control, &mut bytes), Ok(4));
    assert_eq!(bytes, [2,0,0,0]);
}

#[test]
fn destination_validation_clears_previous_value_before_failure() {
    let mut values = RemotePropertyValues::new();
    let control = RemoteControl::new();
    assert_eq!(values.write(PropertyIdentifier(0xd228), &[2,b'A',0,0,0], &control), PropertyStatus::Success);
    assert_eq!(&values.destination()[..2], &[b'A',0]);
    assert_eq!(values.write(PropertyIdentifier(0xd228), &[0], &control), PropertyStatus::InvalidValue);
    assert!(values.destination().iter().all(|byte| *byte == 0));
}

#[test]
fn acknowledgement_failure_preserves_received_selection() {
    let mut values = RemotePropertyValues::new();
    let mut control = RemoteControl::new();
    control.select(RemoteSelection::Configure);
    assert_eq!(values.write(PropertyIdentifier(0xdf01), &[3,0], &control), PropertyStatus::InvalidValue);
    let mut bytes = [0; 2];
    assert_eq!(values.read(PropertyIdentifier(0xdf01), &control, &mut bytes), Ok(2));
    assert_eq!(bytes, [3,0]);
}

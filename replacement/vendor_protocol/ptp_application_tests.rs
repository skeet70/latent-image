use super::ptp_application::{close_session, open_session, ApplicationError, ApplicationOperation, DeviceProperties, InitiatorVersion};
use super::ptp_runtime::PtpRuntime;
use super::ptp_types::{BatteryLevel, PtpCommand, PtpOperationCode, PtpPropertyCode, PtpResponseCode, PtpSessionId, PtpTransactionId};

fn command(operation: u16, parameters: [u32; 5]) -> PtpCommand {
    PtpCommand { operation: PtpOperationCode(operation), transaction: PtpTransactionId(17), parameters, parameter_count: 5 }
}

#[test]
fn session_requires_nonzero_identifier_and_preserves_existing_session_on_failure() {
    let mut runtime = PtpRuntime::new();
    assert_eq!(open_session(&mut runtime, &command(0x1002, [0; 5])).code, PtpResponseCode::INVALID_PARAMETER);
    assert_eq!(runtime.session(), None);
    assert_eq!(open_session(&mut runtime, &command(0x1002, [7, 0, 0, 0, 0])).code, PtpResponseCode::OK);
    assert_eq!(open_session(&mut runtime, &command(0x1002, [8, 0, 0, 0, 0])).code, PtpResponseCode::SESSION_ALREADY_OPEN);
    assert_eq!(runtime.session(), Some(PtpSessionId(7)));
    assert_eq!(close_session(&mut runtime, &command(0x1003, [0, 0, 9, 0, 0])).code, PtpResponseCode(0x2006));
    assert_eq!(runtime.session(), Some(PtpSessionId(7)));
    let reply = close_session(&mut runtime, &command(0x1003, [0; 5]));
    assert_eq!(reply.code, PtpResponseCode::OK);
    assert_eq!(reply.transaction, PtpTransactionId(17));
    assert_eq!(close_session(&mut runtime, &command(0x1003, [0; 5])).code, PtpResponseCode::SESSION_NOT_OPEN);
}

#[test]
fn transport_reset_closes_the_session_the_next_open_sees() {
    let mut runtime = PtpRuntime::new();
    open_session(&mut runtime, &command(0x1002, [1, 0, 0, 0, 0]));
    runtime.reset();
    assert_eq!(open_session(&mut runtime, &command(0x1002, [2, 0, 0, 0, 0])).code, PtpResponseCode::OK);
    runtime.notify(0);
    assert!(runtime.take_event().is_some());
}

#[test]
fn session_validation_precedes_already_open_error() {
    let mut runtime = PtpRuntime::new();
    open_session(&mut runtime, &command(0x1002, [1, 0, 0, 0, 0]));
    assert_eq!(open_session(&mut runtime, &command(0x1002, [2, 1, 0, 0, 0])).code, PtpResponseCode(0x2006));
    assert_eq!(open_session(&mut runtime, &command(0x1002, [0; 5])).code, PtpResponseCode::INVALID_PARAMETER);
}

#[test]
fn battery_descriptor_reports_the_transport_value_and_source_range() {
    let properties = DeviceProperties::new();
    let mut runtime = PtpRuntime::new();
    runtime.set_battery(BatteryLevel::from_status_flags(16));
    let mut output = [0xa5; 20];
    let bytes = properties.descriptor(PtpPropertyCode(0x5001), runtime.battery(), &mut output).unwrap();
    assert_eq!(&output[..bytes], &[1, 0x50, 2, 0, 0, 3, 2, 1, 0, 3, 1]);
    assert_eq!(output[bytes], 0xa5);
}

#[test]
fn device_properties_publish_the_source_defaults() {
    let properties = DeviceProperties::new();
    let mut output = [0; 20];
    for (property, expected) in [(0xd303, &[1][..]), (0xd406, &[1, 0, 0][..]), (0xd407, &[1, 0, 0, 0][..])] {
        let bytes = properties.value(PtpPropertyCode(property), BatteryLevel::Full, &mut output).unwrap();
        assert_eq!(&output[..bytes], expected);
    }
    let bytes = properties.descriptor(PtpPropertyCode(0xd406), BatteryLevel::Full, &mut output).unwrap();
    assert_eq!(&output[..bytes], &[6, 0xd4, 0xff, 0xff, 1, 1, 0, 0, 1, 0, 0, 0]);
    assert_eq!(properties.value(PtpPropertyCode(0x5002), BatteryLevel::Full, &mut output), Err(ApplicationError::PropertyNotSupported));
}

#[test]
fn short_output_is_not_partially_published() {
    let properties = DeviceProperties::new();
    let mut output = [0xa5; 10];
    assert_eq!(properties.descriptor(PtpPropertyCode(0x5001), BatteryLevel::Full, &mut output), Err(ApplicationError::ShortOutput { required: 11 }));
    assert_eq!(output, [0xa5; 10]);
}

#[test]
fn operation_ranges_do_not_admit_unbound_commands() {
    assert_eq!(ApplicationOperation::decode(PtpOperationCode(0x1005)), Some(ApplicationOperation::StorageInformation));
    assert_eq!(ApplicationOperation::decode(PtpOperationCode(0x9805)), Some(ApplicationOperation::ObjectPropertyList));
    for code in [0x1000, 0x100e, 0x1010, 0x101c, 0x9001, 0x901e, 0x9804, 0x980c, 0xffff] {
        assert_eq!(ApplicationOperation::decode(PtpOperationCode(code)), None);
    }
}

#[test]
fn current_initiator_snapshot_is_encoded_without_changing_the_default() {
    let text = [3, b'P', 0, b'C', 0, 0, 0];
    let initiator = InitiatorVersion::parse(&text).unwrap();
    let properties = DeviceProperties::snapshot(initiator);
    let mut output = [0; 20];
    let bytes = properties.value(PtpPropertyCode(0xd406), BatteryLevel::Low, &mut output).unwrap();
    assert_eq!(&output[..bytes], &text);
    let bytes = properties.descriptor(PtpPropertyCode(0xd406), BatteryLevel::Low, &mut output).unwrap();
    assert_eq!(&output[..bytes], &[6, 0xd4, 0xff, 0xff, 1, 1, 0, 0, 3, b'P', 0, b'C', 0, 0, 0, 0]);
}

#[test]
fn truncated_initiator_dataset_is_rejected_at_the_snapshot_boundary() {
    assert_eq!(InitiatorVersion::parse(&[]), Err(ApplicationError::InvalidDataset));
    assert_eq!(InitiatorVersion::parse(&[2, b'P', 0]), Err(ApplicationError::InvalidDataset));
    assert!(InitiatorVersion::parse(&[0]).is_ok());
}

use super::ptp_runtime::*;
use super::ptp_types::*;

fn command(transaction: u32) -> [u8; 16] {
    let mut packet = [0; 16];
    encode_header(&mut packet, PtpContainerKind::Command, 0x1002, PtpTransactionId(transaction), 4).unwrap();
    packet[12..].copy_from_slice(&7u32.to_le_bytes());
    packet
}

#[test]
fn command_decoding_zero_fills_unused_parameters() {
    let bytes = command(0x78563412);
    let decoded = PtpContainer::decode(&bytes).unwrap().command().unwrap();
    assert_eq!(decoded.operation, PtpOperationCode(0x1002));
    assert_eq!(decoded.transaction, PtpTransactionId(0x78563412));
    assert_eq!(decoded.parameters, [7, 0, 0, 0, 0]);
    assert_eq!(decoded.parameter_count, 1);
}

#[test]
fn response_serializes_only_advertised_parameters() {
    let response = PtpResponse { code: PtpResponseCode::OK,
        transaction: PtpTransactionId(0x78563412), parameters: [7, 8, 9, 10, 11], parameter_count: 1 };
    let mut bytes = [0xaa; 32];
    assert_eq!(response.encode(&mut bytes), Ok(16));
    assert_eq!(&bytes[..16], &[16,0,0,0,3,0,1,0x20,0x12,0x34,0x56,0x78,7,0,0,0]);
    assert_eq!(&bytes[16..], &[0xaa;16]);
}

#[test]
fn data_transaction_must_match_last_command() {
    let mut runtime = PtpRuntime::new();
    runtime.initialize(64);
    runtime.receive(&command(3)).unwrap();
    let mut bytes = [0;12];
    encode_header(&mut bytes, PtpContainerKind::Data, 0x1002, PtpTransactionId(2), 0).unwrap();
    assert_eq!(runtime.receive(&bytes), Err(PtpPacketError::StaleTransaction));
    encode_header(&mut bytes, PtpContainerKind::Data, 0x1002, PtpTransactionId(3), 0).unwrap();
    assert!(runtime.receive(&bytes).is_ok());
}

#[test]
fn canceled_transfer_rejects_late_completion_and_preserves_session() {
    let mut runtime = PtpRuntime::new();
    runtime.initialize(64);
    runtime.set_session(Some(PtpSessionId(7)));
    runtime.receive(&command(3)).unwrap();
    let mut output = [0;64];
    let transfer = runtime.prepare_result(PtpApplicationResult::Data { operation:PtpOperationCode(0x1009),
        transaction:PtpTransactionId(3), bytes:52 }, &mut output).unwrap().unwrap();
    assert_eq!(transfer.bytes,64);
    assert!(transfer.zero_packet);
    assert_eq!(runtime.finish_cancel(&[1,0,3,0,0,0]), PtpControlResult::Canceled);
    assert!(!runtime.complete_transfer(transfer.id));
    runtime.notify(0);
    assert_eq!(runtime.take_event(), Some([16,0,0,0,4,0,6,0x40,255,255,255,255,0,0,0,0]));
}

#[test]
fn stale_cancel_does_not_discard_active_command() {
    let mut runtime = PtpRuntime::new();
    runtime.receive(&command(3)).unwrap();
    assert_eq!(runtime.finish_cancel(&[1,0,4,0,0,0]),PtpControlResult::Complete);
    assert_eq!(runtime.command().unwrap().transaction,PtpTransactionId(3));
}

#[test]
fn reset_closes_session_and_discards_queued_event() {
    let mut runtime = PtpRuntime::new();
    runtime.set_session(Some(PtpSessionId(1)));
    runtime.notify(0);
    runtime.reset();
    assert_eq!(runtime.take_event(),None);
    runtime.notify(0);
    assert_eq!(runtime.take_event(),None);
    assert_eq!(runtime.completion_status(),PtpResponseCode::OK);
}

#[test]
fn device_status_endpoint_branches_match_wire_lengths() {
    let runtime = PtpRuntime::new();
    let mut output = [0xff;12];
    assert_eq!(runtime.status_packet(1,false,0x82,false,4,&mut output),4);
    assert_eq!(output,[4,0,1,0x20,0,0,0,0,0,0,0,0]);
    assert_eq!(runtime.status_packet(1,true,0x82,false,12,&mut output),8);
    assert_eq!(output,[8,0,1,0x20,1,0,0,0,0,0,0,0]);
    assert_eq!(runtime.status_packet(1,false,0x82,true,12,&mut output),8);
    assert_eq!(output,[8,0,1,0x20,0x82,0,0,0,0,0,0,0]);
    assert_eq!(runtime.status_packet(1,true,0x82,true,7,&mut output),7);
    assert_eq!(output,[12,0,1,0x20,0x82,0,0,0,1,0,0,0]);
}

#[test]
fn reply_completion_is_consumed_once() {
    let mut runtime = PtpRuntime::new();
    runtime.set_reply_phase(PtpTransferPhase::Pending);
    assert_eq!(runtime.poll_reply().status(),0x100);
    runtime.set_reply_phase(PtpTransferPhase::Complete);
    assert_eq!(runtime.poll_reply().status(),0x220);
    assert_eq!(runtime.poll_reply().status(),0);
}

#[test]
fn malformed_lengths_are_refused_without_panicking() {
    for length in 0..12 { assert_eq!(PtpContainer::decode(&[0;12][..length]),Err(PtpPacketError::Truncated)); }
    let mut bytes = command(1);
    bytes[..4].copy_from_slice(&11u32.to_le_bytes());
    assert_eq!(PtpContainer::decode(&bytes),Err(PtpPacketError::InvalidLength));
    bytes[..4].copy_from_slice(&17u32.to_le_bytes());
    assert_eq!(PtpContainer::decode(&bytes),Err(PtpPacketError::Truncated));
}

#[test]
fn receiver_preserves_header_offset_then_reuses_payload_buffer() {
    let mut receiver = PtpReceiver::new(64);
    let mut buffer = [0;96];
    encode_header(&mut buffer,PtpContainerKind::Data,0x1009,PtpTransactionId(1),188).unwrap();
    assert_eq!(receiver.window(64,96),Ok(PtpReceiveWindow {offset:0,bytes:64}));
    assert_eq!(receiver.complete(&buffer[..64]),Ok(false));
    assert_eq!(receiver.window(64,96),Ok(PtpReceiveWindow {offset:64,bytes:32}));
    assert_eq!(receiver.complete(&buffer),Ok(true));
    assert_eq!(receiver.window(64,96),Ok(PtpReceiveWindow {offset:0,bytes:96}));
    assert_eq!(receiver.complete(&buffer),Ok(true));
    assert_eq!(receiver.window(64,96),Ok(PtpReceiveWindow {offset:0,bytes:8}));
    assert_eq!(receiver.complete(&buffer[..8]),Ok(true));
    assert_eq!(receiver.window(32,96),Ok(PtpReceiveWindow {offset:0,bytes:32}));
}

#[test]
fn exact_packet_length_does_not_request_a_header_continuation() {
    let mut receiver = PtpReceiver::new(64);
    let mut buffer = [0;64];
    encode_header(&mut buffer,PtpContainerKind::Data,0x1009,PtpTransactionId(1),52).unwrap();
    receiver.window(64,64).unwrap();
    assert_eq!(receiver.complete(&buffer),Ok(true));
    assert_eq!(receiver.window(12,64),Ok(PtpReceiveWindow {offset:0,bytes:12}));
}

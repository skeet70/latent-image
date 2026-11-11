use super::packet::*;

#[test]
fn command_preserves_operation_transaction_and_parameter_order() {
    let bytes = [20,0,0,0,1,0,0x15,0x10,0x78,0x56,0x34,0x12,9,0,0,0,7,0,0,0];
    assert_eq!(ApplicationPacket::parse(&bytes), Ok(ApplicationPacket::Command(CommandPacket {
        operation: OperationCode(0x1015), transaction: TransactionId(0x12345678),
        parameters: [9,7,0,0,0], parameter_count: 2,
    })));
}

#[test]
fn application_frame_rejects_mismatched_size_and_partial_parameters() {
    assert_eq!(ApplicationPacket::parse(&[12,0,0,0]), Err(PacketError::LengthMismatch));
    assert_eq!(ApplicationPacket::parse(&[13,0,0,0,1,0,1,0x10,0,0,0,0,3]), Err(PacketError::InvalidCommandParameters));
    assert_eq!(ApplicationPacket::parse(&[4,0,0,0]), Err(PacketError::Truncated));
}

#[test]
fn data_payload_is_delivered_without_header() {
    let bytes = [15,0,0,0,2,0,2,0x10,3,0,0,0,8,9,10];
    assert_eq!(ApplicationPacket::parse(&bytes), Ok(ApplicationPacket::Data {
        operation: OperationCode(0x1002), transaction: TransactionId(3), bytes: &[8,9,10],
    }));
}

#[test]
fn handshake_and_success_reply_preserve_little_endian_unicode() {
    let mut bytes = [0;82];
    bytes[..4].copy_from_slice(&82u32.to_le_bytes());
    bytes[4] = 1;
    bytes[8..12].copy_from_slice(&[0xf2,0xe4,0x53,0x8f]);
    bytes[24..28].copy_from_slice(&7u32.to_le_bytes());
    bytes[28..32].copy_from_slice(&[0xa9,3,0x42,0]);
    let peer = PeerHandshake::parse(&bytes).unwrap().unwrap();
    assert_eq!(peer.protocol, 7);
    assert_eq!(&peer.name[..2], &[0x03a9,0x0042]);
    let mut name = [0;20];
    name[0] = 0x03a9;
    let (frame, length) = response_frame(0, CameraIdentity { name });
    assert_eq!(length,68);
    assert_eq!(&frame[..12], &[68,0,0,0,2,0,0,0,0,0,0,0]);
    assert_eq!(&frame[12..28], &[0x08,0x70,0xb0,0x61,0x0a,0x8b,0x45,0x93,0xb2,0xe7,0x93,0x57,0xdd,0x36,0xe0,0x50]);
    assert_eq!(&frame[28..32], &[0xa9,3,0,0]);
}

#[test]
fn rejection_and_finish_have_distinct_wire_frames() {
    let (frame,length) = response_frame(0x2019,CameraIdentity { name:[0;20] });
    assert_eq!(length,12);
    assert_eq!(&frame[..length], &[12,0,0,0,5,0,0,0,0x19,0x20,0,0]);
    assert_eq!(finish_frame(), [8,0,0,0,255,255,255,255]);
}

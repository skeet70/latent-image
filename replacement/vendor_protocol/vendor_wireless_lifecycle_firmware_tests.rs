use super::{FirmwareAction, FirmwareChunk};
use crate::sd_controller::Buffer;
use crate::vendor_wireless_lifecycle::DriverStatus;
use crate::vendor_wireless_power::RadioTick;

#[test]
fn cancelled_ready_poll_still_writes_chunk_before_failure() {
    let mut scratch = [0x5a; 2560];
    let mut chunk = FirmwareChunk::new(&[1, 2, 3], &mut scratch).unwrap();
    assert_eq!(chunk.action(RadioTick(0)), Some(FirmwareAction::ReadReady));
    assert!(chunk.ready_reply(0, true, RadioTick(0)));
    assert_eq!(chunk.outcome(), None);
    let request = chunk.write_request().unwrap();
    assert_eq!(request.argument.wire(), 0x9001_0008);
    let Buffer::Fifo(bytes) = request.buffer else { panic!() };
    assert_eq!(bytes, &[8, 0, 0, 0, 1, 2, 3, 0x5a]);
    chunk.written();
    assert_eq!(chunk.outcome(), Some(DriverStatus::FAILURE));
}

#[test]
fn full_chunk_uses_five_blocks_and_preserves_scratch_padding() {
    let mut scratch = [0x5a; 2560];
    let mut chunk = FirmwareChunk::new(&[3; 2048], &mut scratch).unwrap();
    chunk.ready_reply(0x100, false, RadioTick(0));
    let request = chunk.write_request().unwrap();
    assert_eq!(request.argument.wire(), 0x9801_0005);
    let Buffer::Fifo(bytes) = request.buffer else { panic!() };
    assert_eq!(bytes.len(), 2560);
    assert_eq!(&bytes[..4], &[4, 8, 0, 0]);
    assert_eq!(&bytes[2052..2056], &[0; 4]);
    assert!(bytes[2056..].iter().all(|byte| *byte == 0x5a));
}

#[test]
fn missing_ready_bit_has_512_delays_then_a_final_read() {
    let mut scratch = [0; 2560];
    let mut chunk = FirmwareChunk::new(&[1], &mut scratch).unwrap();
    for poll in 0..512 {
        assert_eq!(chunk.action(RadioTick(poll * 10)), Some(FirmwareAction::ReadReady));
        chunk.ready_reply(0, false, RadioTick(poll * 10));
        assert_eq!(chunk.action(RadioTick(poll * 10 + 9)), None);
    }
    assert_eq!(chunk.action(RadioTick(5120)), Some(FirmwareAction::ReadReady));
    chunk.ready_reply(0, false, RadioTick(5120));
    assert_eq!(chunk.action(RadioTick(5120)), Some(FirmwareAction::WriteChunk));
}

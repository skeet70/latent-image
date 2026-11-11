use crate::settings_shutdown_actor::*;
use crate::shutdown::{KernelStatus, ServiceMessage, ShutdownRtos, TaskId};
use crate::record_store::*;
use std::{boxed::Box, vec, vec::Vec};

struct Queue { messages: Vec<(u16, u8, u8, Vec<u8>)>, failure: Option<KernelStatus> }
impl ShutdownRtos for Queue {
    fn current_task(&mut self) -> TaskId { TaskId(2) }
    fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
        if let Some(error) = self.failure { return Err(error); }
        self.messages.push((message.operation.0, message.sender.0, message.destination.0, message.payload.to_vec())); Ok(())
    }
    fn set_event_flag(&mut self, _: u32, _: u32) -> Result<(), KernelStatus> { unreachable!() }
}
#[test]
fn release_queue_copies_callback_and_preserves_failure() {
    let mut queue = Queue { messages: Vec::new(), failure: None };
    post_release(&mut queue, CallbackToken(0x12345678)).unwrap();
    assert_eq!(queue.messages, vec![(0x6301, 2, 83, vec![0x78, 0x56, 0x34, 0x12])]);
    queue.failure = Some(KernelStatus(-62));
    assert_eq!(post_release(&mut queue, CallbackToken(9)), Err(KernelStatus(-62)));
    assert_eq!(queue.messages.len(), 1);
}
#[test]
fn record_checksum_protects_only_actual_payload_window() {
    let mut record = [0u8; RECORD_BYTES];
    record[4] = 250;
    record[0x5fb] = 9;
    seal(&mut record);
    assert_eq!(record[0x5fc], 3);
    record[0] = 7;
    record[0x5ff] = 8;
    assert!(valid(&record));
    record[4] = 1;
    assert!(!valid(&record));
}
struct Medium { bytes: Box<[[u8; BLOCK_BYTES]; 2]>, fail: bool }
impl RecordMedium for Medium {
    fn read(&mut self, _: RecordKind, bank: Bank, position: BytePosition, output: &mut [u8]) -> Result<(), MediumError> {
        output.copy_from_slice(&self.bytes[bank as usize][position.0 as usize..position.0 as usize + output.len()]); Ok(())
    }
    fn program(&mut self, _: RecordKind, bank: Bank, position: BytePosition, source: &[u8]) -> Result<(), MediumError> {
        if self.fail { return Err(MediumError(-63)); }
        self.bytes[bank as usize][position.0 as usize..position.0 as usize + source.len()].copy_from_slice(source); Ok(())
    }
    fn erase(&mut self, _: RecordKind, bank: Bank) -> Result<(), MediumError> { self.bytes[bank as usize].fill(0xff); Ok(()) }
}
struct Stage([u8; RECORD_BYTES]);
impl Staging for Stage { fn bytes(&mut self, _: RecordKind) -> &mut [u8] { &mut self.0 } }
struct Runtime { acknowledged: Vec<CallbackToken>, lock_error: Option<KernelStatus> }
impl ReleaseRuntime for Runtime {
    fn acquire(&mut self) -> Result<(), KernelStatus> { match self.lock_error { Some(error) => Err(error), None => Ok(()) } }
    fn release(&mut self) -> Result<(), KernelStatus> { Ok(()) }
    fn cancel_timer(&mut self, _: TimerHandle) {}
    fn acknowledge_release(&mut self, callback: CallbackToken) { self.acknowledged.push(callback); }
}
#[test]
fn release_all_states_closes_store_even_when_medium_program_fails() {
    for state in [TransferState::Initial, TransferState::Active, TransferState::Released, TransferState::Fourth] {
        let mut medium = Medium { bytes: vec![[0xff; BLOCK_BYTES]; 2].into_boxed_slice().try_into().ok().unwrap(), fail: true };
        let mut stage = Stage([0xff; RECORD_BYTES]);
        let mut blocks: Box<[[u8; BLOCK_BYTES]; 2]> = vec![[0; BLOCK_BYTES]; 2].into_boxed_slice().try_into().ok().unwrap();
        let mut cache = CacheState::new();
        let mut service = Service::new(); service.open(&mut stage).unwrap();
        let handle = service.register(RecordKind::Kind4, RecordBytes(1536), RecordCount(20), SequenceOffset(0), SettingsCodec).unwrap();
        let mut entry = Entry::new(&mut medium, &mut stage, &mut blocks, &mut cache);
        let snapshot = SettingsRecord([0; RECORD_BYTES]);
        let mut current = SettingsRecord([0; RECORD_BYTES]); current.0[8] = 7;
        let mut actor = SettingsShutdownActor::adopt(state, None, &mut current, &snapshot);
        let mut runtime = Runtime { acknowledged: Vec::new(), lock_error: None };
        let result = actor.release(&17u32.to_le_bytes(), &mut runtime, &mut service, handle, &mut entry).unwrap();
        assert!(matches!(result.append, AppendObservation::Attempted(Ok(Appended { persisted: Persisted::ProgramFailed(MediumError(-63)), .. }))));
        assert_eq!(runtime.acknowledged, vec![CallbackToken(17)]);
        assert_eq!(actor.state(), TransferState::Released);
        assert!(service.get_mut(handle).is_none());
    }
}

#[test]
fn calendar_window_preserves_inclusive_end_and_february_rollover() {
    use crate::settings_shutdown_bounds::*;
    let start = TransferTimestamp::parse([24, 2, 28, 23, 55, 99]).unwrap();
    let at_end = TransferTimestamp::parse([24, 2, 29, 0, 5, 0]).unwrap();
    let after_end = TransferTimestamp::parse([24, 2, 29, 0, 6, 0]).unwrap();
    assert_eq!(classify_window(start, at_end, WindowMinutes(10)), WindowDisposition::Within);
    assert_eq!(classify_window(start, after_end, WindowMinutes(10)), WindowDisposition::Expired);
    let nonleap = TransferTimestamp::parse([25, 2, 28, 23, 55, 99]).unwrap();
    let march = TransferTimestamp::parse([25, 3, 1, 0, 5, 0]).unwrap();
    assert_eq!(classify_window(nonleap, march, WindowMinutes(10)), WindowDisposition::Within);
    assert!(TransferTimestamp::parse([25, 0, 1, 0, 0, 0]).is_none());
    assert!(TransferTimestamp::parse([25, 13, 1, 0, 0, 0]).is_none());
}

#[test]
fn coordinate_failure_retains_longitude_after_publishing_latitude() {
    use crate::settings_shutdown_parser::*;
    let mut pair = CoordinatePair([91, 92]);
    assert!(coordinates(b"1234.567890,S invalid,E", &mut pair).is_err());
    assert_eq!(pair, CoordinatePair([4_527_407u32.wrapping_neg(), 92]));
    coordinates(b"1234.567890,N 12345.678901,W", &mut pair).unwrap();
    assert_eq!(pair, CoordinatePair([4_527_407, 44_554_073u32.wrapping_neg()]));
}
#[test]
fn descriptor_reconstruction_preserves_partial_bytes_and_signed_altitude_copy() {
    use crate::settings_shutdown_parser::*;
    let source = b"1234.567890,S 12345.678901,W -12.3,M 4.5,K 2026:11:10 12:34:56.00";
    let mut text = [0; 72]; text[..source.len()].copy_from_slice(source);
    let mut descriptor = MetadataDescriptor([0xa5; 172]);
    let mut scratch = CoordinatePair([0, 0]);
    reconstruct(&mut descriptor, &text, 1, &mut scratch);
    assert_eq!(&descriptor.0[12..16], &[2, 3, 0, 0]);
    assert_eq!(descriptor.word(20), 12);
    assert_eq!(descriptor.word(36), 34073);
    assert_eq!(descriptor.0[72], 1);
    assert_eq!(descriptor.word(76), 1233);
    assert_eq!(descriptor.word(80), 100);
    assert_eq!(&descriptor.0[156..166], b"2026:11:10");
    assert_eq!(descriptor.0[166], 0xa5);
    assert_eq!(descriptor.word(84), 12);
    assert_eq!(descriptor.word(100), 56);
    let retained = descriptor.0[..12].to_vec();
    reconstruct(&mut descriptor, &text, 2, &mut scratch);
    assert_eq!(&descriptor.0[..12], retained.as_slice());
    assert_eq!(&descriptor.0[12..], &[0; 160]);
}
#[test]
fn decimal_rejects_overflow_without_accepting_nondigits() {
    use crate::settings_shutdown_parser::*;
    assert_eq!(decimal(b"4294967295"), Ok(u32::MAX));
    assert!(decimal(b"4294967296").is_err());
    assert!(decimal(b"12x").is_err());
    assert_eq!(decimal(b"12\0x"), Ok(12));
    assert_eq!(decimal(b""), Ok(0));
}

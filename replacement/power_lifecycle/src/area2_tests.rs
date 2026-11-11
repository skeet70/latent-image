use super::*;
use crate::area2_codec::Area2Codec;
use crate::mirror::*;
use crate::record_store::*;
use std::{boxed::Box, vec, vec::Vec};

struct Medium { banks: Box<[[u8; BLOCK_BYTES]; 2]>, fail: bool, programs: usize, erases: usize }
impl Medium {
    fn new() -> Self { Self { banks: vec![[0xff; BLOCK_BYTES]; 2].into_boxed_slice().try_into().ok().unwrap(), fail: false, programs: 0, erases: 0 } }
}
impl RecordMedium for Medium {
    fn read(&mut self, _: RecordKind, bank: Bank, offset: BytePosition, output: &mut [u8]) -> Result<(), MediumError> {
        output.copy_from_slice(&self.banks[bank as usize][offset.0 as usize..offset.0 as usize + output.len()]); Ok(())
    }
    fn program(&mut self, _: RecordKind, bank: Bank, offset: BytePosition, bytes: &[u8]) -> Result<(), MediumError> {
        self.programs += 1;
        if self.fail { return Err(MediumError(-63)); }
        for (out, byte) in self.banks[bank as usize][offset.0 as usize..].iter_mut().zip(bytes) { *out &= *byte; }
        Ok(())
    }
    fn erase(&mut self, _: RecordKind, bank: Bank) -> Result<(), MediumError> { self.erases += 1; self.banks[bank as usize].fill(0xff); Ok(()) }
}
struct Stage([u8; RECORD_BYTES]);
impl Staging for Stage { fn bytes(&mut self, _: RecordKind) -> &mut [u8] { &mut self.0 } }
struct Driver { writes: Vec<(BackupKey, BackupValue)>, failed: bool }
impl HeaderBackupDriver for Driver {
    fn context(&mut self) -> ExecutionContext { ExecutionContext::Task }
    fn acquire(&mut self, _: BackupSemaphore) {}
    fn release(&mut self, _: BackupSemaphore) {}
    fn diagnostic(&mut self, _: BackupDiagnostic) {}
    fn admit(&mut self, _: RequestDirection) -> Result<(), DeviceStatus> { if self.failed { Err(DeviceStatus(-63)) } else { Ok(()) } }
    fn write_backup(&mut self, key: BackupKey, value: BackupValue, _: BackupWriteMode) -> DeviceStatus { self.writes.push((key, value)); DeviceStatus(if self.failed { -63 } else { 0 }) }
    fn store_request_status(&mut self, _: RequestStatus) {}
    fn report_failure(&mut self, _: RequestStatus) {}
}

fn exercise(selector: SaveSelector, changes: usize, fail: bool, sequence_only: bool) -> (Medium, Area2Record, Area2Record, SaveObservation, bool, bool) {
    let mut medium = Medium::new(); medium.fail = fail;
    let mut stage = Stage([0xff; RECORD_BYTES]);
    let mut blocks: Box<[[u8; BLOCK_BYTES]; 2]> = vec![[0; BLOCK_BYTES]; 2].into_boxed_slice().try_into().ok().unwrap();
    let mut cache = CacheState::new();
    let mut service = Service::new(); service.open(&mut stage).unwrap();
    let a2 = service.register(RecordKind::Kind2, RecordBytes(512), RecordCount(62), SequenceOffset(0), Area2Codec).unwrap();
    let a3 = service.register(RecordKind::Kind3, RecordBytes(512), RecordCount(62), SequenceOffset(0), Area2Codec).unwrap();
    let mut live = [0x35; BACKING_BYTES];
    live[0..4].copy_from_slice(&[17, 0, 101, 0]);
    let mut shadow = [0x35; HEADER_BYTES];
    let keys = core::array::from_fn(|index| BackupKey(index as u16));
    let mut backing = SharedBacking::without_export_residue(&mut live, &mut shadow, &keys, Driver { writes: Vec::new(), failed: false });
    let mut working = Area2Record([0x9a; RECORD_BYTES]);
    let mut snapshot = Area2Record([0x9a; RECORD_BYTES]);
    working.0[4..0x24].fill(0x35);
    working.0[0x24..0x28].copy_from_slice(&[101, 0, 17, 0]);
    working.0[0x28..0x140].fill(0x35);
    working.0[0x1e8..0x1ea].copy_from_slice(&[1, 80]);
    snapshot.0.copy_from_slice(&working.0);
    if sequence_only { working.0[0..2].copy_from_slice(&[12, 0]); }
    let mut state = Area2Lifecycle::adopt(&mut working, &mut snapshot, a2, a3);
    let mut observation = SaveObservation { append: AppendObservation::Equal, completion: LegacyCompletion::Completed };
    for iteration in 0..=changes {
        if iteration != 0 { backing.live_mut()[0x10] = iteration as u8; }
        observation = state.save(if iteration == changes { selector } else { SaveSelector::Snapshot }, &mut backing, FirmwareVersion { major: 1, minor: 80 }, &mut service, &mut Entry::new(&mut medium, &mut stage, &mut blocks, &mut cache));
    }
    (medium, working, snapshot, observation, service.get_mut(a2).is_some(), service.get_mut(a3).is_some())
}

#[test]
fn equal_and_sequence_only_do_not_program() {
    for sequence_only in [false, true] {
        let (medium, working, snapshot, outcome, a2, a3) = exercise(SaveSelector::Snapshot, 0, false, sequence_only);
        assert_eq!(medium.programs, 0);
        assert_eq!(outcome.append, AppendObservation::Equal);
        assert_eq!(working, snapshot);
        assert!(a2 && a3);
    }
}
#[test]
fn changed_record_reaches_medium_and_selector_closes_both_handles() {
    let (medium, working, _, outcome, a2, a3) = exercise(SaveSelector::Close, 1, false, false);
    assert_eq!(medium.programs, 1);
    assert_eq!(&medium.banks[0][0x28..0x140], &working.0[0x28..0x140]);
    assert!(matches!(outcome.append, AppendObservation::Appended(Appended { persisted: Persisted::Confirmed, .. })));
    assert!(!a2 && !a3);
    assert_eq!(outcome.completion.abi(), 0);
}
#[test]
fn failed_program_keeps_legacy_completion_and_snapshot_contract() {
    let (medium, working, snapshot, outcome, _, _) = exercise(SaveSelector::Snapshot, 1, true, false);
    assert_eq!(medium.programs, 1);
    assert!(medium.banks[0].iter().all(|byte| *byte == 0xff));
    assert!(matches!(outcome.append, AppendObservation::Appended(Appended { persisted: Persisted::ProgramFailed(MediumError(-63)), .. })));
    assert_eq!(outcome.completion.abi(), 0);
    assert_eq!(working, snapshot);
}
#[test]
fn full_bank_rotates_and_full_pair_erases_through_owned_store() {
    let (medium, _, _, outcome, _, _) = exercise(SaveSelector::Snapshot, 63, false, false);
    assert_eq!(medium.programs, 63);
    assert!(medium.erases > 0);
    assert!(matches!(outcome.append, AppendObservation::Appended(Appended { persisted: Persisted::Confirmed, .. })));
}
#[test]
fn parity_accepts_corrects_single_bit_and_refuses_two_columns() {
    let mut record = Area2Record([0x35; RECORD_BYTES]);
    crate::area2_codec::seal(&mut record);
    let valid = record;
    record.0[18] ^= 8;
    assert!(matches!(crate::area2_codec::validate(&mut record), crate::area2_codec::Validation::Corrected(_)));
    assert_eq!(record, valid);
    record.0[18] ^= 3;
    assert_eq!(crate::area2_codec::validate(&mut record), crate::area2_codec::Validation::Rejected);
}

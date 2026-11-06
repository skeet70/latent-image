use super::*;
use std::{boxed::Box, vec, vec::Vec};

struct Medium { banks: [Vec<u8>; 2], fail_program: bool, fail_erase: bool, programs: Vec<(Bank, u32, Vec<u8>)>, erases: Vec<Bank> }
impl Medium {
    fn new() -> Self { Self { banks: [vec![0xff; BLOCK_BYTES], vec![0xff; BLOCK_BYTES]], fail_program: false, fail_erase: false, programs: Vec::new(), erases: Vec::new() } }
    fn put(&mut self, bank: Bank, p: usize, seq: u16, marker: u8) { let start = p * 0x60; self.banks[bank as usize][start..start + 2].copy_from_slice(&seq.to_le_bytes()); self.banks[bank as usize][start + 2] = marker; }
}
impl RecordMedium for Medium {
    fn read(&mut self, _: RecordKind, bank: Bank, offset: BytePosition, output: &mut [u8]) -> Result<(), MediumError> { output.copy_from_slice(&self.banks[bank as usize][offset.0 as usize..offset.0 as usize + output.len()]); Ok(()) }
    fn program(&mut self, _: RecordKind, bank: Bank, offset: BytePosition, bytes: &[u8]) -> Result<(), MediumError> {
        self.programs.push((bank, offset.0, bytes.to_vec()));
        if self.fail_program { return Err(MediumError(-7)); }
        for (dest, src) in self.banks[bank as usize][offset.0 as usize..].iter_mut().zip(bytes) { *dest &= src; } Ok(())
    }
    fn erase(&mut self, _: RecordKind, bank: Bank) -> Result<(), MediumError> { self.erases.push(bank); if self.fail_erase { Err(MediumError(-8)) } else { self.banks[bank as usize].fill(0xff); Ok(()) } }
}
struct Buffers([Vec<u8>; 5]);
impl Buffers { fn new() -> Self { Self([vec![0; 0x60], vec![0; 0x1000], vec![0; 0x200], vec![0; 0x600], vec![0; 0x600]]) } }
impl Staging for Buffers { fn bytes(&mut self, kind: RecordKind) -> &mut [u8] { &mut self.0[kind as usize] } }
#[derive(Default)]
struct Checks { seen: Vec<u16>, reject: bool, correct: bool }
impl Callbacks for Checks {
    fn check(&mut self, bytes: &mut [u8]) -> CheckVerdict { self.seen.push(u16::from_le_bytes([bytes[0], bytes[1]])); if self.reject || bytes[2] == 0 { CheckVerdict::Reject } else { if self.correct { bytes[3] = 0x42; } CheckVerdict::Accept } }
    fn seal(&mut self, bytes: &mut [u8]) { bytes[4] = 0x55; }
}
struct Fixture { medium: Medium, staging: Buffers, block: Box<[[u8; BLOCK_BYTES]; 2]>, cache: CacheState, area: Area, checks: Checks }
impl Fixture {
    fn new() -> Self { let mut staging = Buffers::new(); open(&mut staging); Self { medium: Medium::new(), staging, block: vec![[0; BLOCK_BYTES]; 2].into_boxed_slice().try_into().ok().unwrap(), cache: CacheState::new(), area: Area::register(RecordKind::Kind0, Geometry::new(RecordBytes(0x60), RecordCount(3), SequenceOffset(0)).unwrap()), checks: Checks::default() } }
    fn append(&mut self, seed: u16) -> Result<Appended, NotAppended> { Entry::new(&mut self.medium, &mut self.staging, &mut self.block, &mut self.cache).append(&mut self.area, &mut self.checks, Seed(seed), &[0xa5; 0x60]) }
    fn latest(&mut self, recovery: Option<&mut u16>) -> Option<(Candidate, Vec<u8>)> { match Entry::new(&mut self.medium, &mut self.staging, &mut self.block, &mut self.cache).read_latest(&mut self.area, &mut self.checks, recovery) { Latest::Found { candidate, record } => Some((candidate, record.to_vec())), _ => None } }
}
#[test]
fn rsc_6ad434_successor_masks_7fff_and_seals_without_writing_source() { let mut f = Fixture::new(); f.medium.put(Bank::Zero, 0, 0x7fff, 1); let appended = f.append(0).unwrap(); assert_eq!(appended.sequence, Sequence(0)); assert_eq!(f.medium.banks[0][0x60 + 4], 0x55); }
#[test]
fn rsc_6ac880_full_bank_rotates_without_erase_when_other_bank_has_room() { let mut f = Fixture::new(); for p in 0..3 { f.medium.put(Bank::Zero, p, p as u16, 1); } let result = f.append(0).unwrap(); assert_eq!((result.bank, result.position, result.sequence), (Bank::One, Position(0), Sequence(3))); assert!(f.medium.erases.is_empty()); }
#[test]
fn rsc_6ac880_rotation_uses_bank_queried_before_latest() { let mut f = Fixture::new(); f.medium.put(Bank::Zero, 0, 0, 1); for p in 0..3 { f.medium.put(Bank::One, p, p as u16 + 1, 1); } let result = f.append(0).unwrap(); assert_eq!(result.bank, Bank::One); assert_eq!(result.placement, Placement::ErasedThenWritten); assert_eq!(f.medium.erases, vec![Bank::One]); }
#[test]
fn rsc_6ad2f8_rejected_newest_steps_back_and_final_search_corrects_record() { let mut f = Fixture::new(); f.medium.put(Bank::Zero, 0, 4, 1); f.medium.put(Bank::Zero, 1, 5, 0); f.checks.correct = true; let mut skipped = 0; let (candidate, bytes) = f.latest(Some(&mut skipped)).unwrap(); assert_eq!(candidate.sequence, Sequence(4)); assert_eq!(skipped, 1); assert_eq!(bytes[3], 0x42); assert_eq!(f.checks.seen, vec![5, 4, 5, 4, 5, 4]); }
#[test]
fn rsc_6ad3c4_all_rejected_returns_no_record_and_advances_current() { let mut f = Fixture::new(); f.medium.put(Bank::Zero, 0, 2, 1); f.checks.reject = true; assert!(f.latest(None).is_none()); assert_eq!(f.area.current, Bank::One); }
#[test]
fn rsc_6ac694_full_best_probes_following_bank_only_with_recovery_output() { let mut f = Fixture::new(); for p in 0..3 { f.medium.put(Bank::Zero, p, 10 + p as u16, 1); } f.medium.put(Bank::One, 0, 9, 1); let mut skipped = 99; assert_eq!(f.latest(Some(&mut skipped)).unwrap().0.sequence, Sequence(12)); assert_eq!(f.checks.seen, vec![12, 9, 9, 12]); assert_eq!(skipped, 0); }
#[test]
fn rsc_6ac57c_wrap_comparator_selects_zero_after_7fff() { let mut f = Fixture::new(); f.medium.put(Bank::Zero, 0, 0x7fff, 1); f.medium.put(Bank::One, 0, 0, 1); assert_eq!(f.latest(None).unwrap().0.bank, Bank::One); }
#[test]
fn rsc_6ac984_erase_all_erases_both_in_current_order() { let mut f = Fixture::new(); f.area.current = Bank::One; f.medium.put(Bank::Zero, 0, 1, 1); f.medium.put(Bank::One, 0, 2, 1); Entry::new(&mut f.medium, &mut f.staging, &mut f.block, &mut f.cache).erase_all(&mut f.area); assert!(f.medium.banks.iter().all(|b| b.iter().all(|v| *v == 0xff))); assert_eq!(f.medium.erases, vec![Bank::One, Bank::Zero]); }
#[test]
fn rsc_6aded4_kind3_programs_600_with_ff_tail() { let mut f = Fixture::new(); f.area = Area::register(RecordKind::Kind3, Geometry::new(RecordBytes(0x34), RecordCount(127), SequenceOffset(0)).unwrap()); let result = Entry::new(&mut f.medium, &mut f.staging, &mut f.block, &mut f.cache).append(&mut f.area, &mut f.checks, Seed(0x9234), &[0xa5; 0x34]).unwrap(); assert_eq!(result.sequence, Sequence(0x9234)); assert_eq!(f.medium.programs[0].2.len(), 0x600); assert!(f.medium.programs[0].2[0x34..].iter().all(|b| *b == 0xff)); }
#[test]
fn rsc_6ad460_sequence_uses_rejected_physical_predecessor() { let mut f = Fixture::new(); f.medium.put(Bank::Zero, 0, 10, 1); f.medium.put(Bank::Zero, 1, 11, 0); assert_eq!(f.append(99).unwrap().sequence, Sequence(12)); }
#[test]
fn rsc_6ac978_failed_program_is_typed_but_append_succeeds() { let mut f = Fixture::new(); f.medium.fail_program = true; assert_eq!(f.append(0).unwrap().persisted, Persisted::ProgramFailed(MediumError(-7))); assert!(f.medium.banks[0].iter().all(|b| *b == 0xff)); }
#[test]
fn rsc_6ac978_failed_erase_leaves_destination_full() { let mut f = Fixture::new(); for bank in [Bank::Zero, Bank::One] { for p in 0..3 { f.medium.put(bank, p, p as u16 + if bank == Bank::One {3} else {0}, 1); } } f.medium.fail_erase = true; assert_eq!(f.append(0), Err(NotAppended::DestinationFull { erase: Some(MediumError(-8)) })); }
#[test]
fn rsc_6acc38_collect_sorts_unsigned_and_preserves_unused_payload() { let mut f = Fixture::new(); f.medium.put(Bank::Zero, 0, 0x7fff, 1); f.medium.put(Bank::One, 0, 0, 1); let mut output = [0x71; 0x60 * 6]; let count = Entry::new(&mut f.medium, &mut f.staging, &mut f.block, &mut f.cache).collect(&mut f.area, &mut output); assert_eq!(count, 0xc0); assert_eq!(&output[..2], &[0, 0]); assert_eq!(&output[0x60..0x62], &[0xff, 0x7f]); assert_eq!(output[0xc0 + 2], 0x71); }
#[test]
fn rsc_6acda0_read_at_bounds_zero_only_first_byte_outside() { let mut f = Fixture::new(); let mut output = [0x71; 4]; let mut entry = Entry::new(&mut f.medium, &mut f.staging, &mut f.block, &mut f.cache); assert_eq!(entry.read_at(RecordKind::Kind0, BytePosition(0x40000), &mut output), Err(ReadError::OutsideBank)); assert_eq!(output, [0, 0x71, 0x71, 0x71]); assert_eq!(entry.read_at(RecordKind::Kind0, BytePosition(0x1ffff), &mut output), Err(ReadError::CrossesBank)); assert_eq!(output, [0; 4]); }
#[test]
fn rsc_6ad0b4_geometry_checks_alignment_and_capacity() { assert!(Geometry::new(RecordBytes(0x60), RecordCount(1365), SequenceOffset(94)).is_ok()); assert!(Geometry::new(RecordBytes(0x60), RecordCount(1366), SequenceOffset(0)).is_err()); assert!(Geometry::new(RecordBytes(0x60), RecordCount(1), SequenceOffset(1)).is_err()); }

#[test]
fn rsc_6ac328_service_open_registration_close_and_slot_reuse() {
    let mut staging = Buffers::new();
    let mut service = Service::<Checks>::new();
    assert_eq!(service.register(RecordKind::Kind0, RecordBytes(96), RecordCount(7), SequenceOffset(0), Checks::default()), Err(RegistrationError::Closed));
    service.open(&mut staging).unwrap();
    assert!(staging.0.iter().all(|buffer| buffer.iter().all(|b| *b == 0xff)));
    assert_eq!(service.open(&mut staging), Err(RegistrationError::Closed));
    let first = service.register(RecordKind::Kind0, RecordBytes(96), RecordCount(7), SequenceOffset(0), Checks::default()).unwrap();
    assert_eq!(service.get_mut(first).unwrap().area.geometry.records_per_bank(), RecordCount(3));
    for _ in 0..4 { service.register(RecordKind::Kind0, RecordBytes(96), RecordCount(6), SequenceOffset(0), Checks::default()).unwrap(); }
    assert_eq!(service.register(RecordKind::Kind0, RecordBytes(96), RecordCount(6), SequenceOffset(0), Checks::default()), Err(RegistrationError::SlotsFull));
    service.close(first);
    assert!(service.get_mut(first).is_none());
    let reused = service.register(RecordKind::Kind0, RecordBytes(96), RecordCount(6), SequenceOffset(0), Checks::default()).unwrap();
    assert_eq!(reused, first);
}

#[test]
fn rsc_6ad498_staging_source_is_the_record_present_after_destination_seek() {
    let mut f = Fixture::new();
    f.medium.put(Bank::Zero, 0, 10, 1);
    let result = Entry::new(&mut f.medium, &mut f.staging, &mut f.block, &mut f.cache).append_source(&mut f.area, &mut f.checks, Seed(0), RecordSource::Staging).unwrap();
    assert_eq!(result.sequence, Sequence(11));
    assert_eq!(&f.medium.banks[0][0x60..0x65], &[11, 0, 0xff, 0xff, 0x55]);
}

#[test]
fn rsc_geometry_table_all_five_kinds_preserve_program_lengths_and_records() {
    for (kind, s, n, length) in [(RecordKind::Kind0, 0x60, 254, 0x60), (RecordKind::Kind1, 0x1000, 16, 0x1000), (RecordKind::Kind2, 0x200, 31, 0x200), (RecordKind::Kind3, 0x34, 127, 0x600), (RecordKind::Kind4, 0x600, 10, 0x600)] {
        let mut f = Fixture::new();
        f.area = Area::register(kind, Geometry::new(RecordBytes(s), RecordCount(n), SequenceOffset(0)).unwrap());
        let source = vec![0xa5; s as usize];
        let result = Entry::new(&mut f.medium, &mut f.staging, &mut f.block, &mut f.cache).append(&mut f.area, &mut f.checks, Seed(0x1234), &source).unwrap();
        assert_eq!(result.sequence, Sequence(0x1234));
        assert_eq!(f.medium.programs[0].2.len(), length);
        let (candidate, record) = f.latest(None).unwrap();
        assert_eq!(candidate.sequence, Sequence(0x1234));
        assert_eq!(&record[..5], &[0x34, 0x12, 0xa5, 0xa5, 0x55]);
        assert_eq!(source, vec![0xa5; s as usize]);
    }
}

#[test]
fn rsc_6ac57c_failed_scan_leaves_recovery_and_pointer_output_unchanged() {
    let mut f = Fixture::new();
    f.medium.put(Bank::Zero, 0, 1, 0);
    let mut recovery = 0x9876;
    let mut entry = Entry::new(&mut f.medium, &mut f.staging, &mut f.block, &mut f.cache);
    let result = entry.read_latest(&mut f.area, &mut f.checks, Some(&mut recovery));
    assert_eq!(result, Latest::NoneValid { output: FailureOutput::Unchanged });
    assert_eq!(recovery, 0x9876);
}

#[test]
fn rsc_6ad698_full_physical_page_collection_reaches_position_n() {
    let mut f = Fixture::new();
    f.area = Area::register(RecordKind::Kind1, Geometry::new(RecordBytes(0x1000), RecordCount(32), SequenceOffset(0)).unwrap());
    for p in 0..32 {
        f.medium.banks[0][p * 0x1000..p * 0x1000 + 2].copy_from_slice(&(p as u16).to_le_bytes());
    }
    let mut output = vec![0x71; 2 * BLOCK_BYTES];
    let count = Entry::new(&mut f.medium, &mut f.staging, &mut f.block, &mut f.cache).collect(&mut f.area, &mut output);
    assert_eq!(count, BLOCK_BYTES);
    assert_eq!(&output[..2], &[0, 0]);
    assert_eq!(&output[31 * 0x1000..31 * 0x1000 + 2], &[31, 0]);
    assert_eq!(output[BLOCK_BYTES + 2], 0x71);
}

#[test]
fn rsc_6ac328_total_one_truncates_to_zero_and_collection_is_empty() {
    let mut staging = Buffers::new();
    let mut service = Service::<Checks>::new();
    service.open(&mut staging).unwrap();
    let handle = service.register(RecordKind::Kind0, RecordBytes(96), RecordCount(1), SequenceOffset(0), Checks::default()).unwrap();
    let registration = service.get_mut(handle).unwrap();
    assert_eq!(registration.area.geometry.records_per_bank(), RecordCount(0));
    let mut medium = Medium::new();
    let mut block: Box<[[u8; BLOCK_BYTES]; 2]> = vec![[0; BLOCK_BYTES]; 2].into_boxed_slice().try_into().ok().unwrap();
    let mut cache = CacheState::new();
    let mut output = [];
    assert_eq!(Entry::new(&mut medium, &mut staging, &mut block, &mut cache).collect(&mut registration.area, &mut output), 0);
}

#[test]
fn rsc_6ad0b4_geometry_capacity_truncates_non_power_of_two_sizes() {
    for (bytes, capacity) in [(3, 43690), (7, 18724), (0x34, 2520), (0x60, 1365), (0x200, 256), (0x600, 85), (0x1000, 32), (65534, 2)] {
        assert!(Geometry::new(RecordBytes(bytes), RecordCount(capacity), SequenceOffset(0)).is_ok());
        assert_eq!(Geometry::new(RecordBytes(bytes), RecordCount(capacity + 1), SequenceOffset(0)), Err(GeometryError::Count));
    }
}

#[test]
fn rsc_6acef8_non_power_of_two_page_capacity_wraps_position_n_to_page_offset_zero() {
    let mut f = Fixture::new();
    f.area = Area::register(RecordKind::Kind0, Geometry::new(RecordBytes(7), RecordCount(18724), SequenceOffset(0)).unwrap());
    for bank in 0..2 {
        for p in 0..18724 {
            let seq = (p as u16).to_le_bytes();
            f.medium.banks[bank][p * 7..p * 7 + 7].copy_from_slice(&[seq[0], seq[1], 0x50 + bank as u8, 0x22, 0x33, 0x44, 0x55]);
        }
    }
    let mut output = vec![0x71; 262136];
    assert_eq!(Entry::new(&mut f.medium, &mut f.staging, &mut f.block, &mut f.cache).collect(&mut f.area, &mut output), 262136);
    assert_eq!(&output[..7], &[0, 0, 0x50, 0x22, 0x33, 0x44, 0x55]);
    assert_eq!(&output[131068..131075], &[0, 0, 0x51, 0x22, 0x33, 0x44, 0x55]);
    assert_eq!(&f.staging.bytes(RecordKind::Kind0)[..7], &[0, 0, 0x51, 0x22, 0x33, 0x44, 0x55]);
}

#[test]
fn rsc_6ac694_append_null_recovery_output_skips_following_bank_probe() {
    let mut f = Fixture::new();
    for p in 0..3 { f.medium.put(Bank::Zero, p, 10 + p as u16, 1); }
    f.medium.put(Bank::One, 0, 9, 1);
    let appended = f.append(0).unwrap();
    assert_eq!((appended.bank, appended.position, appended.sequence), (Bank::One, Position(1), Sequence(10)));
    assert_eq!(f.checks.seen, vec![12, 9, 12]);
}

struct CacheMedium {
    banks: [[Vec<u8>; 2]; 5],
    fail_read: bool,
    fail_program: bool,
    fail_erase: bool,
}
impl CacheMedium {
    fn new() -> Self {
        Self { banks: core::array::from_fn(|kind| core::array::from_fn(|bank| vec![0x10 * (kind as u8 + 1) + bank as u8; BLOCK_BYTES])), fail_read: false, fail_program: false, fail_erase: false }
    }
}
impl RecordMedium for CacheMedium {
    fn read(&mut self, kind: RecordKind, bank: Bank, offset: BytePosition, output: &mut [u8]) -> Result<(), MediumError> {
        if self.fail_read { return Err(MediumError(-7)); }
        output.copy_from_slice(&self.banks[kind as usize][bank as usize][offset.0 as usize..offset.0 as usize + output.len()]);
        Ok(())
    }
    fn program(&mut self, kind: RecordKind, bank: Bank, offset: BytePosition, bytes: &[u8]) -> Result<(), MediumError> {
        if self.fail_program { return Err(MediumError(-8)); }
        for (dest, source) in self.banks[kind as usize][bank as usize][offset.0 as usize..].iter_mut().zip(bytes) { *dest &= source; }
        Ok(())
    }
    fn erase(&mut self, kind: RecordKind, bank: Bank) -> Result<(), MediumError> {
        if self.fail_erase { return Err(MediumError(-9)); }
        self.banks[kind as usize][bank as usize].fill(0xff);
        Ok(())
    }
}
struct CacheFixture {
    medium: CacheMedium,
    staging: Buffers,
    blocks: Box<[[u8; BLOCK_BYTES]; 2]>,
    cache: CacheState,
}
impl CacheFixture {
    fn new() -> Self {
        Self { medium: CacheMedium::new(), staging: Buffers::new(), blocks: vec![[0; BLOCK_BYTES]; 2].into_boxed_slice().try_into().ok().unwrap(), cache: CacheState::new() }
    }
    fn entry(&mut self) -> Entry<'_, CacheMedium, Buffers> { Entry::new(&mut self.medium, &mut self.staging, &mut self.blocks, &mut self.cache) }
    fn read(&mut self, kind: RecordKind, bank: Bank) -> u8 {
        let mut output = [0];
        assert_eq!(self.entry().read_at(kind, BytePosition(bank as u32 * BLOCK_BYTES as u32), &mut output), Ok(()));
        output[0]
    }
}

#[test]
fn persistent_cache_shares_two_slots_across_kinds_and_refreshes_hit_age() {
    let mut f = CacheFixture::new();
    assert_eq!(f.read(RecordKind::Kind0, Bank::Zero), 0x10);
    assert_eq!(f.read(RecordKind::Kind1, Bank::Zero), 0x20);
    f.medium.banks[0][0].fill(0xa0);
    f.medium.banks[1][0].fill(0xb0);
    assert_eq!(f.read(RecordKind::Kind0, Bank::Zero), 0x10);
    assert_eq!(f.read(RecordKind::Kind2, Bank::Zero), 0x30);
    assert_eq!(f.read(RecordKind::Kind0, Bank::Zero), 0x10);
    assert_eq!(f.read(RecordKind::Kind1, Bank::Zero), 0xb0);
    assert_eq!(f.read(RecordKind::Kind2, Bank::Zero), 0x30);
}

#[test]
fn failed_cache_fill_keeps_evicted_bytes_and_remains_a_hit_after_backend_recovers() {
    let mut f = CacheFixture::new();
    assert_eq!(f.read(RecordKind::Kind0, Bank::Zero), 0x10);
    assert_eq!(f.read(RecordKind::Kind1, Bank::Zero), 0x20);
    f.medium.fail_read = true;
    assert_eq!(f.read(RecordKind::Kind2, Bank::Zero), 0x10);
    f.medium.fail_read = false;
    assert_eq!(f.read(RecordKind::Kind2, Bank::Zero), 0x10);
    assert_eq!(f.read(RecordKind::Kind3, Bank::Zero), 0x40);
    assert_eq!(f.read(RecordKind::Kind4, Bank::Zero), 0x50);
    assert_eq!(f.read(RecordKind::Kind2, Bank::Zero), 0x30);
}

#[test]
fn program_and_erase_invalidate_both_kinds_even_when_backend_fails() {
    for fail in [false, true] {
        let mut f = CacheFixture::new();
        f.medium.banks[0][0].fill(0xff);
        f.medium.banks[0][1].fill(0xff);
        f.medium.fail_program = fail;
        f.medium.fail_erase = fail;
        assert_eq!(f.read(RecordKind::Kind1, Bank::Zero), 0x20);
        let mut area = Area::register(RecordKind::Kind0, Geometry::new(RecordBytes(0x60), RecordCount(3), SequenceOffset(0)).unwrap());
        let result = f.entry().append(&mut area, &mut Checks::default(), Seed(7), &[0xa5; 0x60]).unwrap();
        assert_eq!(result.persisted, if fail { Persisted::ProgramFailed(MediumError(-8)) } else { Persisted::Confirmed });
        f.medium.banks[0][0][0] = 0xc0;
        f.medium.banks[1][0][0] = 0xc1;
        assert_eq!(f.read(RecordKind::Kind0, Bank::Zero), 0xc0);
        assert_eq!(f.read(RecordKind::Kind1, Bank::Zero), 0xc1);
        f.entry().erase_all(&mut area);
        f.medium.banks[0][0][0] = 0xd0;
        f.medium.banks[1][0][0] = 0xd1;
        assert_eq!(f.read(RecordKind::Kind0, Bank::Zero), 0xd0);
        assert_eq!(f.read(RecordKind::Kind1, Bank::Zero), 0xd1);
    }
}

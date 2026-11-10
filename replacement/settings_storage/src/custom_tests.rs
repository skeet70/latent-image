use super::{Area2Startup, CustomBootOutcome, CustomCodec, CustomCommitOutcome, CustomDefaultsConfiguration, CustomMetadata, CustomPayload, CustomRecordBackend, CustomReset, CustomSettingsLifecycle, CustomSettingsRecord, HeaderStartup, RecordStoreBackend, ReloadOutcome, StartupPrerequisites};
use record_store::{self as store, Callbacks};
use std::{boxed::Box, vec, vec::Vec};

struct Medium { banks: [Vec<u8>; 2], fail_program: bool, fail_erase: bool, writes: Vec<Vec<u8>> }
impl Medium {
    fn erased() -> Self { Self { banks: [vec![0xff; store::BLOCK_BYTES], vec![0xff; store::BLOCK_BYTES]], fail_program: false, fail_erase: false, writes: Vec::new() } }
}
impl store::RecordMedium for Medium {
    fn read(&mut self, kind: store::RecordKind, bank: store::Bank, offset: store::BytePosition, output: &mut [u8]) -> Result<(), store::MediumError> {
        assert_eq!(kind, store::RecordKind::Kind3);
        let start = offset.0 as usize;
        output.copy_from_slice(&self.banks[bank as usize][start..start + output.len()]);
        Ok(())
    }
    fn program(&mut self, kind: store::RecordKind, bank: store::Bank, offset: store::BytePosition, bytes: &[u8]) -> Result<(), store::MediumError> {
        assert_eq!(kind, store::RecordKind::Kind3);
        assert_eq!(bytes.len(), 0x600);
        self.writes.push(bytes.to_vec());
        if self.fail_program { return Err(store::MediumError(-7)); }
        let start = offset.0 as usize;
        self.banks[bank as usize][start..start + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }
    fn erase(&mut self, _: store::RecordKind, bank: store::Bank) -> Result<(), store::MediumError> {
        if self.fail_erase { return Err(store::MediumError(-8)); }
        self.banks[bank as usize].fill(0xff);
        Ok(())
    }
}
struct Staging([u8; 0x600]);
impl store::Staging for Staging { fn bytes(&mut self, kind: store::RecordKind) -> &mut [u8] { assert_eq!(kind, store::RecordKind::Kind3); &mut self.0 } }
fn record(sequence: u16, metadata: [u8; 2], payload: [u8; 40]) -> CustomSettingsRecord {
    let mut bytes = [0; 52];
    bytes[..2].copy_from_slice(&sequence.to_le_bytes());
    bytes[2..4].copy_from_slice(&metadata);
    bytes[4..44].copy_from_slice(&payload);
    CustomCodec.seal(&mut bytes);
    CustomSettingsRecord::from_working_wire(bytes)
}
fn prerequisites(reset: CustomReset) -> StartupPrerequisites { StartupPrerequisites { area2: Area2Startup::Accepted, header: HeaderStartup::AcceptedBackup, custom_reset: reset } }
fn blocks() -> Box<[[u8; store::BLOCK_BYTES]; 2]> { vec![[0; store::BLOCK_BYTES]; 2].into_boxed_slice().try_into().ok().unwrap() }

#[test]
fn comparison_includes_every_metadata_and_payload_byte_but_not_sequence_or_parity() {
    let baseline = record(17, [9, 8], [4; 40]);
    for offset in 0..52 {
        let mut wire = *baseline.wire();
        wire[offset] ^= 1;
        assert_eq!(CustomSettingsRecord::from_working_wire(wire).changed_from(&baseline), (2..44).contains(&offset), "offset {offset}");
    }
}

#[test]
fn codec_corrects_single_payload_bit_and_rejects_two_bit_error() {
    let baseline = record(0x7ffe, [0xa9, 0x52], [0x3c; 40]);
    let mut wire = *baseline.wire();
    wire[27] ^= 8;
    let (corrected, outcome) = CustomSettingsRecord::validate(wire).unwrap();
    assert_eq!(corrected, baseline);
    assert!(matches!(outcome, area3_validator::Outcome::Corrected(_)));
    wire[28] ^= 16;
    assert!(CustomSettingsRecord::validate(wire).is_err());
}

#[test]
fn boot_copies_only_corrected_payload_and_preserves_prerequisite_observation() {
    let expected = record(9, [3, 4], [0x72; 40]);
    let mut medium = Medium::erased();
    medium.banks[0][..52].copy_from_slice(expected.wire());
    medium.banks[0][14] ^= 2;
    let mut staging = Staging([0x69; 0x600]);
    let mut scratch = blocks();
    let mut cache = store::CacheState::new();
    let mut backend = RecordStoreBackend::new(&mut medium, &mut staging, &mut scratch, &mut cache);
    let mut previous = record(0, [0, 0], [0; 40]);
    let mut working = previous;
    let mut live = CustomPayload([0; 40]);
    let mut state = CustomSettingsLifecycle::adopt(&mut previous, &mut working, &mut live);
    let mut startup = StartupPrerequisites { area2: Area2Startup::Defaults, header: HeaderStartup::AcceptedAlternate, custom_reset: CustomReset::Preserve };
    let outcome = state.boot(&mut startup, &CustomDefaultsConfiguration::from_abi([0; 6]), &mut backend, || panic!("accepted boot resets modes"));
    assert_eq!(state.live(), &expected.payload());
    assert_eq!(state.working(), &expected);
    assert_eq!(outcome.prerequisites, startup);
    assert!(outcome.missing_defaults.is_none());
    assert!(outcome.reset_defaults.is_none());
}

fn boot_missing(rejected: bool) -> (CustomBootOutcome, CustomPayload, usize) {
    let mut medium = Medium::erased();
    if rejected { medium.banks[0][..52].copy_from_slice(record(0, [0, 0], [0; 40]).wire()); medium.banks[0][4] ^= 3; }
    let mut staging = Staging([0xff; 0x600]);
    let mut scratch = blocks();
    let mut cache = store::CacheState::new();
    let mut backend = RecordStoreBackend::new(&mut medium, &mut staging, &mut scratch, &mut cache);
    let mut previous = record(8, [7, 6], [5; 40]);
    let mut working = previous;
    let mut live = CustomPayload([0; 40]);
    let mut state = CustomSettingsLifecycle::adopt(&mut previous, &mut working, &mut live);
    let mut resets = 0;
    let defaults = CustomDefaultsConfiguration::from_abi([0x81, 1, 3, 8, 0x86, 10]);
    let outcome = state.boot(&mut prerequisites(CustomReset::Preserve), &defaults, &mut backend, || resets += 1);
    assert_eq!(state.live(), &defaults.payload());
    assert_eq!(resets, 1);
    (outcome, *state.live(), medium.writes.len())
}

#[test]
fn empty_and_none_valid_are_distinct_and_both_load_defaults() {
    let (empty, empty_payload, writes) = boot_missing(false);
    let (invalid, invalid_payload, invalid_writes) = boot_missing(true);
    assert_eq!(empty.initial, ReloadOutcome::Empty);
    assert!(matches!(invalid.initial, ReloadOutcome::NoneValid { .. }));
    assert_eq!(empty_payload, invalid_payload);
    assert_eq!((writes, invalid_writes), (1, 1));
}

#[test]
fn configuration_defaults_preserve_other_bits_and_cover_stock_branch_variants() {
    let baseline = *crate::SlotDefaultsWindow::slot(crate::ProfileSelection::Record0).bytes();
    for (input, expected) in [(0,7), (3,7), (4,8), (5,11), (6,14), (7,17), (8,20), (9,23), (10,25), (11,8), (0x80,1), (0x85,1), (0x86,2), (0x87,3), (0x88,4), (0xff,1)] {
        let payload = CustomDefaultsConfiguration::from_abi([0x82, 1, 3, input, input, input]).payload();
        assert_eq!(payload.0[25] & 31, expected);
        assert_eq!(payload.0[24] & 31, expected);
        assert_eq!(payload.0[26], (baseline[26] & 0x80) | 0x40 | expected);
        assert_eq!(payload.0[13], baseline[13] & 31);
        assert_eq!(payload.0[30], (baseline[30] & 0x9f) | 0x60);
    }
}

#[test]
fn reset_flag_refreshes_defaults_and_clears_without_missing_record_mode_reset() {
    let mut medium = Medium::erased();
    medium.banks[0][..52].copy_from_slice(record(0, [1, 2], [0; 40]).wire());
    let mut staging = Staging([0xff; 0x600]);
    let mut scratch = blocks();
    let mut cache = store::CacheState::new();
    let mut backend = RecordStoreBackend::new(&mut medium, &mut staging, &mut scratch, &mut cache);
    let mut previous = record(4, [1, 2], [0; 40]);
    let mut working = previous;
    let mut live = CustomPayload([0; 40]);
    let mut state = CustomSettingsLifecycle::adopt(&mut previous, &mut working, &mut live);
    let mut startup = prerequisites(CustomReset::Reset);
    let defaults = CustomDefaultsConfiguration::from_abi([0; 6]);
    let outcome = state.boot(&mut startup, &defaults, &mut backend, || panic!("reset flag resets modes"));
    assert_eq!(startup.custom_reset, CustomReset::Preserve);
    assert!(outcome.reset_defaults.is_some());
    assert_eq!(state.live(), &defaults.payload());
}

#[test]
fn append_seals_assigned_sequence_preserves_medium_tail_and_rereads_after_program_failure() {
    let original = record(41, [1, 2], [3; 40]);
    let mut medium = Medium::erased();
    medium.banks[0][..52].copy_from_slice(original.wire());
    medium.fail_program = true;
    let mut staging = Staging([0x69; 0x600]);
    let mut scratch = blocks();
    let mut cache = store::CacheState::new();
    let mut previous = original;
    let mut working = original;
    let mut live = CustomPayload([4; 40]);
    let mut state = CustomSettingsLifecycle::adopt(&mut previous, &mut working, &mut live);
    {
        let mut backend = RecordStoreBackend::new(&mut medium, &mut staging, &mut scratch, &mut cache);
        let outcome = state.commit(&mut backend);
        assert_eq!(outcome.retained_abi_status(), 0);
        assert!(matches!(outcome, CustomCommitOutcome::Appended { append: Ok(store::Appended { sequence: store::Sequence(42), persisted: store::Persisted::ProgramFailed(store::MediumError(-7)), .. }), latest: ReloadOutcome::Latest { record, .. } } if record == original));
        assert_eq!(state.working().sequence(), store::Sequence(41));
        assert_eq!(state.live(), &CustomPayload([4; 40]));
    }
    let programmed: [u8; 52] = medium.writes[0][..52].try_into().unwrap();
    assert!(matches!(CustomSettingsRecord::validate(programmed), Ok((_, area3_validator::Outcome::Accepted))));
    assert!(medium.writes[0][52..].iter().all(|byte| *byte == 0x69));
    medium.fail_program = false;
    let mut backend = RecordStoreBackend::new(&mut medium, &mut staging, &mut scratch, &mut cache);
    assert!(matches!(state.commit(&mut backend), CustomCommitOutcome::Appended { append: Ok(store::Appended { persisted: store::Persisted::Confirmed, .. }), latest: ReloadOutcome::Latest { .. } }));
    assert_eq!(state.working().sequence(), store::Sequence(42));
    assert_eq!(state.commit(&mut backend), CustomCommitOutcome::Unchanged);
}

struct Full;
impl CustomRecordBackend for Full {
    fn read_latest(&mut self) -> ReloadOutcome { ReloadOutcome::NoneValid { output: store::FailureOutput::Unchanged } }
    fn append(&mut self, _: &CustomSettingsRecord) -> Result<store::Appended, store::NotAppended> { Err(store::NotAppended::DestinationFull { erase: Some(store::MediumError(-8)) }) }
}
#[test]
fn append_and_reread_failures_remain_typed_and_keep_previous_sequence() {
    let mut previous = record(37, [1, 2], [3; 40]);
    let mut working = record(99, [1, 2], [4; 40]);
    let mut live = CustomPayload([4; 40]);
    let mut state = CustomSettingsLifecycle::adopt(&mut previous, &mut working, &mut live);
    assert!(matches!(state.commit(&mut Full), CustomCommitOutcome::Appended { append: Err(store::NotAppended::DestinationFull { erase: Some(store::MediumError(-8)) }), latest: ReloadOutcome::NoneValid { .. } }));
    assert_eq!(state.working().sequence(), store::Sequence(37));
}

#[test]
fn full_banks_expose_erase_failure_then_real_erase_and_reload() {
    let mut medium = Medium::erased();
    for bank in 0..2 {
        for position in 0..127 {
            let wire = record((bank * 127 + position) as u16, [1, 2], [3; 40]);
            medium.banks[bank][position * 52..(position + 1) * 52].copy_from_slice(wire.wire());
        }
    }
    medium.fail_erase = true;
    let mut staging = Staging([0x69; 0x600]);
    let mut scratch = blocks();
    let mut cache = store::CacheState::new();
    let mut previous = record(253, [1, 2], [3; 40]);
    let mut working = previous;
    let mut live = CustomPayload([4; 40]);
    let mut state = CustomSettingsLifecycle::adopt(&mut previous, &mut working, &mut live);
    {
        let mut backend = RecordStoreBackend::new(&mut medium, &mut staging, &mut scratch, &mut cache);
        let outcome = state.commit(&mut backend);
        assert!(matches!(outcome, CustomCommitOutcome::Appended { append: Err(store::NotAppended::DestinationFull { erase: Some(store::MediumError(-8)) }), latest: ReloadOutcome::Latest { .. } }));
        assert_eq!(state.working().sequence(), store::Sequence(253));
    }
    assert!(medium.writes.is_empty());
    medium.fail_erase = false;
    let mut backend = RecordStoreBackend::new(&mut medium, &mut staging, &mut scratch, &mut cache);
    assert!(matches!(state.commit(&mut backend), CustomCommitOutcome::Appended { append: Ok(store::Appended { placement: store::Placement::ErasedThenWritten, sequence: store::Sequence(254), .. }), latest: ReloadOutcome::Latest { .. } }));
    assert_eq!(state.working().sequence(), store::Sequence(254));
}

#[test]
fn full_first_bank_rotates_to_empty_second_bank() {
    let mut medium = Medium::erased();
    for position in 0..127 {
        medium.banks[0][position * 52..(position + 1) * 52].copy_from_slice(record(position as u16, [1, 2], [3; 40]).wire());
    }
    let mut staging = Staging([0x69; 0x600]);
    let mut scratch = blocks();
    let mut cache = store::CacheState::new();
    let mut backend = RecordStoreBackend::new(&mut medium, &mut staging, &mut scratch, &mut cache);
    let mut previous = record(126, [1, 2], [3; 40]);
    let mut working = previous;
    let mut live = CustomPayload([4; 40]);
    let mut state = CustomSettingsLifecycle::adopt(&mut previous, &mut working, &mut live);
    assert!(matches!(state.commit(&mut backend), CustomCommitOutcome::Appended { append: Ok(store::Appended { bank: store::Bank::One, placement: store::Placement::Rotated, sequence: store::Sequence(127), .. }), latest: ReloadOutcome::Latest { .. } }));
    assert_eq!(state.working().sequence(), store::Sequence(127));
}

#[test]
fn failed_cold_start_keeps_stock_erased_readback_and_reports_program_error() {
    let mut medium = Medium::erased();
    medium.fail_program = true;
    let mut staging = Staging([0x69; 0x600]);
    let mut scratch = blocks();
    let mut cache = store::CacheState::new();
    let mut backend = RecordStoreBackend::new(&mut medium, &mut staging, &mut scratch, &mut cache);
    let mut previous = record(7, [1, 2], [3; 40]);
    let mut working = previous;
    let mut live = CustomPayload([4; 40]);
    let mut state = CustomSettingsLifecycle::adopt(&mut previous, &mut working, &mut live);
    let outcome = state.boot(&mut prerequisites(CustomReset::Preserve), &CustomDefaultsConfiguration::from_abi([0; 6]), &mut backend, || {});
    assert!(matches!(outcome.missing_defaults, Some(CustomCommitOutcome::Appended { append: Ok(store::Appended { persisted: store::Persisted::ProgramFailed(store::MediumError(-7)), .. }), latest: ReloadOutcome::Empty })));
    assert_eq!(state.live(), &CustomPayload([0xff; 40]));
    assert_eq!(state.working().wire(), &[0xff; 52]);
    assert_eq!(medium.writes.len(), 1);
}

#[test]
fn successful_commit_survives_fresh_lifecycle_boot_without_defaults() {
    let mut medium = Medium::erased();
    let mut staging = Staging([0x69; 0x600]);
    let mut scratch = blocks();
    let mut cache = store::CacheState::new();
    {
        let mut backend = RecordStoreBackend::new(&mut medium, &mut staging, &mut scratch, &mut cache);
        let mut previous = record(0xffff, [0xff; 2], [0xff; 40]);
        let mut working = record(0, [1, 2], [3; 40]);
        let mut live = CustomPayload([4; 40]);
        let mut state = CustomSettingsLifecycle::adopt(&mut previous, &mut working, &mut live);
        assert!(matches!(state.commit(&mut backend), CustomCommitOutcome::Appended { latest: ReloadOutcome::Latest { .. }, .. }));
    }
    cache = store::CacheState::new();
    let mut backend = RecordStoreBackend::new(&mut medium, &mut staging, &mut scratch, &mut cache);
    let mut previous = record(99, [9, 9], [9; 40]);
    let mut working = previous;
    let mut live = CustomPayload([0; 40]);
    let mut state = CustomSettingsLifecycle::adopt(&mut previous, &mut working, &mut live);
    let outcome = state.boot_with_defaults(&mut prerequisites(CustomReset::Preserve), || panic!("valid reboot reads defaults"), &mut backend, || panic!("valid reboot resets modes"));
    assert!(outcome.missing_defaults.is_none());
    assert_eq!(state.live(), &CustomPayload([4; 40]));
    assert_eq!(state.working().metadata(), CustomMetadata([1, 2]));
    assert_eq!(state.commit(&mut backend), CustomCommitOutcome::Unchanged);
}

#[test]
fn high_row_parity_bits_are_rejected_by_public_codec_and_record_store() {
    for bit in 11..32 {
        let mut wire = *record(0, [1, 2], [3; 40]).wire();
        wire[44 + bit / 8] ^= 1 << (bit % 8);
        assert!(matches!(CustomSettingsRecord::validate(wire), Err(super::CustomCodecError::RowParityHighBits)));
        let mut medium = Medium::erased();
        medium.banks[0][..52].copy_from_slice(&wire);
        let mut staging = Staging([0xff; 0x600]);
        let mut scratch = blocks();
        let mut cache = store::CacheState::new();
        let mut backend = RecordStoreBackend::new(&mut medium, &mut staging, &mut scratch, &mut cache);
        assert!(matches!(backend.read_latest(), ReloadOutcome::NoneValid { .. }), "bit {bit}");
    }
}

#[test]
fn rejected_newer_record_reports_recovery_count_and_returns_older_payload() {
    for high_row_bit in [false, true] {
        let older = record(7, [1, 2], [3; 40]);
        let mut newer = *record(8, [4, 5], [6; 40]).wire();
        if high_row_bit { newer[45] |= 8; } else { newer[4] ^= 3; }
        let mut medium = Medium::erased();
        medium.banks[0][..52].copy_from_slice(older.wire());
        medium.banks[0][52..104].copy_from_slice(&newer);
        let mut staging = Staging([0xff; 0x600]);
        let mut scratch = blocks();
        let mut cache = store::CacheState::new();
        let mut backend = RecordStoreBackend::new(&mut medium, &mut staging, &mut scratch, &mut cache);
        let mut recovery = 0xaaaa;
        assert!(matches!(backend.read_with_recovery(Some(&mut recovery)), ReloadOutcome::Latest { record, .. } if record == older));
        assert_eq!(recovery, 1);
    }
}

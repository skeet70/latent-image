use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Effect {
    Acquire(BackupSemaphore),
    Release(BackupSemaphore),
    Diagnostic(BackupDiagnostic),
    Write(BackupKey, BackupValue, BackupWriteMode),
    Refresh([u8; CUSTOM_BYTES]),
    Status(RequestStatus),
    Report(RequestStatus),
}

struct Driver {
    context: ExecutionContext,
    refusal: Option<DeviceStatus>,
    write_status: DeviceStatus,
    effects: [Option<Effect>; 128],
    count: usize,
}

impl Driver {
    fn new() -> Self {
        Self { context: ExecutionContext::Task, refusal: None, write_status: DeviceStatus(0), effects: [None; 128], count: 0 }
    }

    fn push(&mut self, effect: Effect) {
        self.effects[self.count] = Some(effect);
        self.count += 1;
    }

    fn effects(&self) -> impl Iterator<Item = Effect> + '_ {
        self.effects[..self.count].iter().flatten().copied()
    }

    fn clear(&mut self) { self.count = 0; }
}

impl HeaderBackupDriver for Driver {
    fn context(&mut self) -> ExecutionContext { self.context }
    fn acquire(&mut self, semaphore: BackupSemaphore) { self.push(Effect::Acquire(semaphore)); }
    fn release(&mut self, semaphore: BackupSemaphore) { self.push(Effect::Release(semaphore)); }
    fn diagnostic(&mut self, diagnostic: BackupDiagnostic) { self.push(Effect::Diagnostic(diagnostic)); }
    fn admit(&mut self, _: RequestDirection) -> Result<(), DeviceStatus> {
        match self.refusal { Some(status) => Err(status), None => Ok(()) }
    }
    fn write_backup(&mut self, key: BackupKey, value: BackupValue, mode: BackupWriteMode) -> DeviceStatus {
        self.push(Effect::Write(key, value, mode));
        self.write_status
    }
    fn store_request_status(&mut self, status: RequestStatus) { self.push(Effect::Status(status)); }
    fn report_failure(&mut self, status: RequestStatus) { self.push(Effect::Report(status)); }
}
impl BackupDriver for Driver {
    fn refresh_custom(&mut self, custom: &[u8; CUSTOM_BYTES]) { self.push(Effect::Refresh(*custom)); }
}

fn with_backing(test: impl FnOnce(&mut SharedBacking<'_, Driver>)) {
    let mut live = [0x11; BACKING_BYTES];
    let mut shadow = [0x22; HEADER_BYTES];
    let keys = core::array::from_fn(|index| BackupKey(0x230 + index as u16 * 3));
    let residue = core::array::from_fn(|index| 0xc0 + index as u8);
    let mut backing = SharedBacking::new(&mut live, &mut shadow, &keys, &residue, Driver::new());
    test(&mut backing);
}

#[test]
fn header_reads_preserve_bytes_from_aligned_and_unaligned_shadow_windows() {
    #[repr(align(2))]
    struct Shadow([u8; HEADER_BYTES + 1]);
    #[repr(align(2))]
    struct Output([u8; HEADER_BYTES]);
    for offset in 0..2 {
        let mut shadow = Shadow(core::array::from_fn(|index| 0x30 + index as u8));
        let expected: [u8; HEADER_BYTES] = core::array::from_fn(|index| 0x30 + (offset + index) as u8);
        let mut live = [0x11; BACKING_BYTES];
        let keys = core::array::from_fn(|index| BackupKey(0x230 + index as u16 * 3));
        let residue = [0; EXPORT_RESIDUE_BYTES];
        let source: &mut [u8; HEADER_BYTES] = (&mut shadow.0[offset..offset + HEADER_BYTES]).try_into().unwrap();
        let mut backing = SharedBacking::new(&mut live, source, &keys, &residue, Driver::new());
        let mut output = Output([0xa5; HEADER_BYTES]);
        assert_eq!(backing.read_header(&mut output.0), RequestOutcome::success());
        assert_eq!(output.0, expected);
    }
}

#[test]
fn exports_preserve_projected_bytes_and_caller_guards_for_odd_offsets_and_lengths() {
    #[repr(align(2))]
    struct Output([u8; 6]);
    for (offset, bytes) in [
        (0, [0x22, 0x22, 0x22]),
        (1, [0x22, 0x22, 0x22]),
        (14, [0x22, 0x22, 0x11]),
        (15, [0x22, 0x11, 0x11]),
        (16, [0x11, 0x11, 0x11]),
        (0x166, [0x11, 0x11, 0xc0]),
        (0x167, [0x11, 0xc0, 0xc1]),
        (0x168, [0xc0, 0xc1, 0xc2]),
    ] {
        for destination in 0..2 {
            for length in 0..=3 {
                with_backing(|backing| {
                    let mut output = Output([0xa5; 6]);
                    let mut expected = [0xa5; 6];
                    expected[destination..destination+length].copy_from_slice(&bytes[..length]);
                    assert_eq!(backing.export(BackingRange::new(offset, length as u32), &mut output.0[destination..destination+length]), BackingStatus::Success);
                    assert_eq!(output.0, expected);
                });
            }
        }
    }
}

#[test]
fn imports_preserve_payload_and_neighbor_bytes_for_odd_offsets_and_lengths() {
    #[repr(align(2))]
    struct Payload([u8; 6]);
    let payload = Payload([0x31, 0x32, 0x33, 0x34, 0x35, 0x36]);
    for source in 0..2 {
        for destination in 0..2 {
            for length in 0..=3 {
                with_backing(|backing| {
                    let mut expected = [0x11; 6];
                    expected[destination..destination+length].copy_from_slice(&payload.0[source..source+length]);
                    let outcome = backing.import(BackingRange::new(16 + destination as u32, length as u32), &payload.0[source..source+length]);
                    assert_eq!(outcome.status, BackingStatus::Success);
                    assert_eq!(&backing.live()[16..22], &expected);
                });
            }
        }
    }
}

#[test]
fn open_refreshes_from_shadow_and_locks_only_the_read() {
    with_backing(|backing| {
        let mut bank_header = [0x33; HEADER_BYTES];
        assert_eq!(backing.read_header(&mut bank_header), RequestOutcome::success());
        assert_eq!(bank_header, [0x22; HEADER_BYTES]);
        assert_eq!(backing.live()[..HEADER_BYTES], [0x11; HEADER_BYTES]);
        assert!(backing.driver().effects().eq([
            Effect::Acquire(BackupSemaphore::Settings),
            Effect::Release(BackupSemaphore::Settings),
            Effect::Status(RequestStatus::Success),
        ]));
    });
}

#[test]
fn close_synchronizes_in_key_order_without_changing_bank_checksum_or_refreshing_custom() {
    with_backing(|backing| {
        let bank_header = [1; HEADER_BYTES];
        let outcome = backing.write_header(&bank_header);
        assert_eq!(outcome.request, RequestOutcome::success());
        assert_eq!(bank_header[15], 1);
        assert_eq!(backing.live()[15], 241);
        assert_eq!(backing.shadow(), &backing.live()[..HEADER_BYTES]);
        let mut effects = backing.driver().effects();
        assert_eq!(effects.next(), Some(Effect::Acquire(BackupSemaphore::Settings)));
        for index in 0..HEADER_BYTES {
            let expected = if index == 15 { 241 } else { 1 };
            let key = BackupKey(0x230 + index as u16 * 3);
            assert_eq!(effects.next(), Some(Effect::Write(key, BackupValue(expected), BackupWriteMode::Immediate)));
            let observation = outcome.synchronized[index].expect("changed header byte");
            assert_eq!(observation.position.index(), index);
            assert_eq!(observation.key, key);
            assert_eq!(observation.value, BackupValue(expected));
        }
        assert_eq!(effects.next(), Some(Effect::Release(BackupSemaphore::Settings)));
        assert_eq!(effects.next(), Some(Effect::Status(RequestStatus::Success)));
        assert_eq!(effects.next(), None);
        drop(effects);
        let mut refreshed = [0; HEADER_BYTES];
        backing.read_header(&mut refreshed);
        assert_eq!(refreshed[15], 241);
    });
}

#[test]
fn each_single_changed_position_writes_only_its_register() {
    for position in 0..HEADER_BYTES {
        let mut source = [0; HEADER_BYTES];
        let mut shadow = [0; HEADER_BYTES];
        if position < 15 {
            source[position] = 1;
            source[15] = 255;
            shadow[15] = 255;
        } else {
            shadow[15] = 1;
        }
        let mut live = [0; BACKING_BYTES];
        let keys = core::array::from_fn(|index| BackupKey(index as u16 + 100));
        let residue = [0; EXPORT_RESIDUE_BYTES];
        let mut backing = SharedBacking::new(&mut live, &mut shadow, &keys, &residue, Driver::new());
        let outcome = backing.write_header(&source);
        assert_eq!(outcome.synchronized.iter().flatten().count(), 1);
        assert_eq!(outcome.synchronized[position].expect("one changed position").value, BackupValue(source[position]));
        assert!(backing.driver().effects().filter(|effect| matches!(effect, Effect::Write(..))).eq([
            Effect::Write(keys[position], BackupValue(source[position]), BackupWriteMode::Immediate),
        ]));
    }
}

#[test]
fn repeated_close_with_identical_bytes_makes_no_device_writes() {
    with_backing(|backing| {
        let header = [7; HEADER_BYTES];
        backing.write_header(&header);
        backing.driver_mut().clear();
        let outcome = backing.write_header(&header);
        assert_eq!(outcome.synchronized, [None; HEADER_BYTES]);
        assert!(!backing.driver().effects().any(|effect| matches!(effect, Effect::Write(..) | Effect::Refresh(_))));
    });
}

#[test]
fn checksum_excludes_old_checksum_and_wraps_at_byte_width() {
    for (first, second, third, expected) in [(0, 0, 0, 0), (255, 0, 0, 1), (255, 1, 0, 0), (255, 255, 1, 1)] {
        with_backing(|backing| {
            let mut header = [0; HEADER_BYTES];
            header[..3].copy_from_slice(&[first, second, third]);
            header[15] = 0xa5;
            backing.write_header(&header);
            assert_eq!(backing.live()[15], expected);
            assert_eq!(backing.shadow()[15], expected);
            assert_eq!(header[15], 0xa5);
        });
    }
}

#[test]
fn driver_failures_remain_observable_but_shadow_advances_without_retry() {
    with_backing(|backing| {
        backing.driver_mut().write_status = DeviceStatus(-9);
        let header = [3; HEADER_BYTES];
        let first = backing.write_header(&header);
        assert_eq!(first.request, RequestOutcome::success());
        assert!(first.synchronized.iter().flatten().all(|observation| observation.status == DeviceStatus(-9)));
        assert_eq!(backing.shadow(), &backing.live()[..HEADER_BYTES]);
        backing.driver_mut().clear();
        let second = backing.write_header(&header);
        assert_eq!(second.synchronized, [None; HEADER_BYTES]);
        assert!(!backing.driver().effects().any(|effect| matches!(effect, Effect::Report(_) | Effect::Write(..))));
    });
}

#[test]
fn rejected_open_and_close_preserve_all_bytes_and_report_mapped_status() {
    with_backing(|backing| {
        backing.driver_mut().refusal = Some(DeviceStatus(-17));
        let mut header = [0x33; HEADER_BYTES];
        let expected = RequestOutcome { underlying: BackingStatus::Refused(DeviceStatus(-17)), mapped: RequestStatus::Failed, reported: true };
        assert_eq!(backing.read_header(&mut header), expected);
        assert_eq!(header, [0x33; HEADER_BYTES]);
        let close = backing.write_header(&header);
        assert_eq!(close.request, expected);
        assert_eq!(close.synchronized, [None; HEADER_BYTES]);
        assert_eq!(backing.live(), &[0x11; BACKING_BYTES]);
        assert_eq!(backing.shadow(), &[0x22; HEADER_BYTES]);
        assert_eq!(backing.driver().effects().filter(|effect| matches!(effect, Effect::Report(RequestStatus::Failed))).count(), 2);
        assert!(!backing.driver().effects().any(|effect| matches!(effect, Effect::Write(..) | Effect::Refresh(_))));
    });
}

#[test]
fn outside_task_diagnoses_each_operation_without_semaphore_calls() {
    with_backing(|backing| {
        backing.driver_mut().context = ExecutionContext::OutsideTask;
        let mut header = [0; HEADER_BYTES];
        backing.read_header(&mut header);
        backing.write_header(&header);
        assert_eq!(backing.driver().effects().filter(|effect| matches!(effect, Effect::Diagnostic(BackupDiagnostic::OutsideTask))).count(), 2);
        assert!(!backing.driver().effects().any(|effect| matches!(effect, Effect::Acquire(_) | Effect::Release(_))));
        assert_eq!(BackupDiagnostic::OutsideTask.legacy_arguments(), (2, 59));
        assert_eq!(BackupSemaphore::Settings.legacy_id(), 37);
        assert_eq!(BackupWriteMode::Immediate.legacy_argument(), 0);
    });
}

#[test]
fn shared_export_assembles_shadow_and_backing_with_explicit_residue() {
    with_backing(|backing| {
        backing.live_mut()[0x10..0x128].fill(0x31);
        backing.live_mut()[0x128..0x140].fill(0x42);
        backing.live_mut()[0x140..0x168].fill(0x53);
        backing.live_mut()[0x168..].fill(0x64);
        let mut exported = [0; BACKING_BYTES];
        assert_eq!(backing.export(BackingRange::new(0, BACKING_BYTES as u32), &mut exported), BackingStatus::Success);
        assert_eq!(exported[..16], [0x22; 16]);
        assert_eq!(exported[0x10..0x128], [0x31; BODY_BYTES]);
        assert_eq!(exported[0x128..0x140], [0x42; 24]);
        assert_eq!(exported[0x140..0x168], [0x53; CUSTOM_BYTES]);
        for (index, byte) in exported[0x168..].iter().enumerate() { assert_eq!(*byte, 0xc0 + index as u8); }
    });
}

#[test]
fn auxiliary_body_and_custom_adapters_share_the_retained_backing() {
    with_backing(|backing| {
        backing.auxiliary_mut()[12..].fill(0x79);
        let auxiliary = [0x68; 12];
        let outcome = backing.import(BackingRange::new(0x128, 12), &auxiliary);
        assert_eq!(outcome.status, BackingStatus::Success);
        assert!(!outcome.custom_refreshed);
        assert_eq!(&backing.auxiliary()[..12], &auxiliary);
        assert_eq!(&backing.auxiliary()[12..], &[0x79; 12]);
        let body = [0x35; BODY_BYTES];
        assert_eq!(backing.write_body(&body).request, RequestOutcome::success());
        assert_eq!(&backing.live()[0x10..0x128], &body);
        let mut loaded_body = [0; BODY_BYTES];
        backing.read_body(&mut loaded_body);
        assert_eq!(loaded_body, body);
        let custom = [0x46; CUSTOM_BYTES];
        let custom_write = backing.import(BackingRange::new(0x140, CUSTOM_BYTES as u32), &custom);
        assert!(custom_write.custom_refreshed);
        let mut loaded_custom = [0; CUSTOM_BYTES];
        backing.read_custom(&mut loaded_custom);
        assert_eq!(loaded_custom, custom);
        assert!(backing.driver().effects().any(|effect| effect == Effect::Refresh(custom)));
    });
}

#[test]
fn admitted_end_boundaries_control_custom_refresh_even_for_empty_writes() {
    for end in [0x140, 0x141, 0x168, 0x19a, 0x19b] {
        with_backing(|backing| {
            let result = backing.import(BackingRange::new(end, 0), &[]);
            assert_eq!(result.status, if end <= 0x19a { BackingStatus::Success } else { BackingStatus::OutOfRange });
            assert_eq!(result.custom_refreshed, end > 0x140 && end <= 0x19a);
            assert_eq!(backing.driver().effects().filter(|effect| matches!(effect, Effect::Refresh(_))).count(), usize::from(end > 0x140 && end <= 0x19a));
            let mut empty = [];
            assert_eq!(backing.export(BackingRange::new(end, 0), &mut empty), result.status);
        });
    }
}

#[test]
fn short_descriptors_truncate_both_words_and_map_range_failure() {
    with_backing(|backing| {
        let mut header = [0; HEADER_BYTES];
        assert_eq!(backing.read_request(RequestDescriptor::from_abi(0x10010, 0x10000), &mut header), RequestOutcome::success());
        assert_eq!(header, [0x22; HEADER_BYTES]);
        let failed = backing.read_request(RequestDescriptor::from_abi(1, 0x19a), &mut header);
        assert_eq!(failed, RequestOutcome { underlying: BackingStatus::OutOfRange, mapped: RequestStatus::Failed, reported: true });
        assert_eq!(failed.underlying.legacy_code(), -30);
        assert_eq!(failed.mapped.legacy_code(), -63);
        assert_eq!(backing.driver().effects().last(), Some(Effect::Report(RequestStatus::Failed)));
        assert_eq!(header, [0x22; HEADER_BYTES]);
    });
}

#[test]
fn range_admission_wraps_but_unavailable_compatibility_addresses_are_explicit() {
    let range = BackingRange::new(u32::MAX, 1);
    assert!(range.admitted());
    assert_eq!(range.end(), 0);
    with_backing(|backing| {
        let mut byte = [0x7e];
        assert_eq!(backing.export(range, &mut byte), BackingStatus::AddressUnavailable);
        assert_eq!(byte, [0x7e]);
        assert_eq!(backing.import(range, &[0x8f]).status, BackingStatus::AddressUnavailable);
    });
}

#[test]
fn invalid_ranges_release_semaphore_before_reporting() {
    with_backing(|backing| {
        let result = backing.write_request(RequestDescriptor::from_abi(1, 0x19a), &[0xff]);
        assert_eq!(result.request.mapped, RequestStatus::Failed);
        assert!(backing.driver().effects().eq([
            Effect::Acquire(BackupSemaphore::Settings),
            Effect::Release(BackupSemaphore::Settings),
            Effect::Status(RequestStatus::Failed),
            Effect::Report(RequestStatus::Failed),
        ]));
        assert_eq!(backing.live(), &[0x11; BACKING_BYTES]);
    });
}

#[test]
fn missing_export_residue_preserves_caller_bytes() {
    let mut live = [0x11; BACKING_BYTES];
    let mut shadow = [0x22; HEADER_BYTES];
    let identifiers = [BackupKey(1); HEADER_BYTES];
    let mut backing = SharedBacking::without_export_residue(&mut live, &mut shadow, &identifiers, Driver::new());
    let mut header = [0; HEADER_BYTES];
    assert_eq!(backing.read_header(&mut header), RequestOutcome::success());
    assert_eq!(header, [0x22; HEADER_BYTES]);
    let mut exported = [0x73; BACKING_BYTES];
    assert_eq!(backing.export(BackingRange::new(0, BACKING_BYTES as u32), &mut exported), BackingStatus::Success);
    assert_eq!(exported[..HEADER_BYTES], [0x22; HEADER_BYTES]);
    assert_eq!(exported[HEADER_BYTES..INITIALIZED_EXPORT_BYTES], [0x11; INITIALIZED_EXPORT_BYTES - HEADER_BYTES]);
    assert_eq!(exported[INITIALIZED_EXPORT_BYTES..], [0x73; EXPORT_RESIDUE_BYTES]);
}

#[test]
fn export_crossing_the_initialized_boundary_keeps_unwritten_residue() {
    let mut live = [0x11; BACKING_BYTES];
    let mut shadow = [0x22; HEADER_BYTES];
    let identifiers = [BackupKey(1); HEADER_BYTES];
    let mut backing = SharedBacking::without_export_residue(&mut live, &mut shadow, &identifiers, Driver::new());
    let mut exported = [0x5a; 0x80];
    assert_eq!(backing.export(BackingRange::new(0x100, 0x80), &mut exported), BackingStatus::Success);
    assert_eq!(exported[..0x68], [0x11; 0x68]);
    assert_eq!(exported[0x68..], [0x5a; 0x18]);
}

#[test]
fn custom_refresh_observes_the_imported_byte_after_header_driver_writes() {
    with_backing(|backing| {
        let outcome = backing.import(BackingRange::new(0x140, 1), &[0x87]);
        assert!(outcome.custom_refreshed);
        let mut expected = [0x11; CUSTOM_BYTES];
        expected[0] = 0x87;
        let mut effects = backing.driver().effects();
        assert_eq!(effects.next(), Some(Effect::Acquire(BackupSemaphore::Settings)));
        assert!(effects.by_ref().take_while(|effect| matches!(effect, Effect::Write(..))).count() > 0);
        assert_eq!(effects.next(), Some(Effect::Release(BackupSemaphore::Settings)));
        assert_eq!(effects.next(), None);
        assert_eq!(backing.driver().effects().filter(|effect| matches!(effect, Effect::Refresh(_))).last(), Some(Effect::Refresh(expected)));
    });
}

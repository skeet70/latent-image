use super::*;
use crate::shutdown::RequestId;
use crate::storage_shutdown_lens_service::LensCallback;
use std::vec::Vec;

struct Storage {
    slots: [LensPendingCommand; 16],
    write: u32,
    read: u32,
    count: u32,
}
impl Storage {
    fn new() -> Self {
        Self {
            slots: [LensPendingCommand::new(RequestId(0), false, 0, LensCallback(0), false); 16],
            write: 0,
            read: 0,
            count: 0,
        }
    }
    fn ring(&mut self) -> LensCommandRing<'_> {
        LensCommandRing::new(&mut self.slots, &mut self.write, &mut self.read, &mut self.count)
    }
}

fn command(value: u32) -> LensPendingCommand {
    LensPendingCommand::new(RequestId(0x7202), false, value, LensCallback(0x30), false)
}

#[test]
fn empty_ring_yields_no_command_and_keeps_its_indices() {
    let mut storage = Storage::new();
    assert_eq!(storage.ring().peek(), None);
    assert_eq!(storage.ring().pop(), None);
    assert_eq!((storage.write, storage.read, storage.count), (0, 0, 0));
}

#[test]
fn commands_leave_in_submission_order() {
    let mut storage = Storage::new();
    for value in [0x10, 0x20, 0x30] {
        assert_eq!(storage.ring().push(command(value)), Ok(()));
    }
    assert_eq!(storage.count, 3);
    for value in [0x10, 0x20, 0x30] {
        assert_eq!(storage.ring().peek().map(|pending| pending.command), Some(value));
        assert_eq!(storage.ring().pop().map(|pending| pending.command), Some(value));
    }
    assert_eq!(storage.ring().pop(), None);
    assert_eq!(storage.count, 0);
}

#[test]
fn sixteen_commands_fill_the_ring_and_the_seventeenth_is_rejected() {
    let mut storage = Storage::new();
    for value in 0..16u32 {
        assert_eq!(storage.ring().push(command(value)), Ok(()));
    }
    assert_eq!((storage.write, storage.read, storage.count), (0, 0, 16));
    assert_eq!(storage.ring().push(command(0x99)), Err(RingFull));
    assert_eq!((storage.write, storage.read, storage.count), (0, 0, 16));
    let values: Vec<u32> = (0..16).filter_map(|_| storage.ring().pop().map(|pending| pending.command)).collect();
    assert_eq!(values, (0..16u32).collect::<Vec<u32>>());
}

#[test]
fn wrap_reuses_a_freed_slot_after_the_write_index_passes_fifteen() {
    let mut storage = Storage::new();
    for value in 0..16u32 {
        assert_eq!(storage.ring().push(command(value)), Ok(()));
    }
    for _ in 0..15 {
        assert!(storage.ring().pop().is_some());
    }
    assert_eq!((storage.write, storage.read, storage.count), (0, 15, 1));
    assert_eq!(storage.ring().push(command(0x100)), Ok(()));
    assert_eq!((storage.write, storage.read, storage.count), (1, 15, 2));
    let values: Vec<u32> = (0..2).filter_map(|_| storage.ring().pop().map(|pending| pending.command)).collect();
    assert_eq!(values, std::vec![15, 0x100]);
}

#[test]
fn clear_discards_pending_commands_and_restarts_both_indices() {
    let mut storage = Storage::new();
    for value in 0..5u32 {
        assert_eq!(storage.ring().push(command(value)), Ok(()));
    }
    storage.ring().clear();
    assert_eq!((storage.write, storage.read, storage.count), (0, 0, 0));
    assert_eq!(storage.ring().peek(), None);
    assert_eq!(storage.ring().push(command(0x77)), Ok(()));
    assert_eq!(storage.ring().pop().map(|pending| pending.command), Some(0x77));
}

#[test]
fn pending_command_carries_operation_last_flag_and_skip_mark() {
    let pending = LensPendingCommand::new(RequestId(0x1234), true, 0xbeef, LensCallback(0x88), true);
    assert_eq!(pending.operation(), RequestId(0x1234));
    assert!(pending.is_last());
    assert!(pending.is_skipped());
    assert_eq!((pending.command, pending.callback), (0xbeef, LensCallback(0x88)));
    let single = LensPendingCommand::new(RequestId(0xffff), false, 0, LensCallback(0), false);
    assert_eq!(single.operation(), RequestId(0xffff));
    assert!(!single.is_last());
    assert!(!single.is_skipped());
}

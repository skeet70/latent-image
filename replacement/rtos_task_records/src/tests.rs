use super::*;
use crate::layout::{Domain, DomainId, GuestAddress, TaskSlot, TaskState};
use core::cell::{Cell, RefCell};
use std::collections::BTreeMap;

#[derive(Clone, Default)]
struct Memory(BTreeMap<u32, u8>);

impl Memory {
    fn word(&self, at: GuestAddress) -> u32 {
        let mut value = 0;
        for index in 0..4 {
            value |= (*self.0.get(&(at.0.wrapping_add(index))).unwrap_or(&0) as u32) << (index * 8);
        }
        value
    }

    fn set_word(&mut self, at: GuestAddress, value: u32) {
        for index in 0..4 {
            self.0.insert(at.0.wrapping_add(index), (value >> (index * 8)) as u8);
        }
    }

    fn byte(&self, at: GuestAddress) -> u8 {
        *self.0.get(&at.0).unwrap_or(&0)
    }

    fn set_byte(&mut self, at: GuestAddress, value: u8) {
        self.0.insert(at.0, value);
    }
}

struct Mock {
    memory: RefCell<Memory>,
    masked: Cell<bool>,
    saved: Cell<u32>,
    barriers: Cell<u32>,
}

impl Mock {
    fn new() -> Self {
        Self {
            memory: RefCell::new(Memory::default()),
            masked: Cell::new(false),
            saved: Cell::new(0),
            barriers: Cell::new(0),
        }
    }

    fn word(&self, at: GuestAddress) -> u32 {
        self.memory.borrow().word(at)
    }

    fn byte(&self, at: GuestAddress) -> u8 {
        self.memory.borrow().byte(at)
    }

    fn set_word(&self, at: GuestAddress, value: u32) {
        self.memory.borrow_mut().set_word(at, value);
    }

    fn set_byte(&self, at: GuestAddress, value: u8) {
        self.memory.borrow_mut().set_byte(at, value);
    }

    fn snapshot(&self) -> Memory {
        self.memory.borrow().clone()
    }
}

impl Machine for Mock {
    fn load_word(&self, at: GuestAddress) -> u32 {
        self.word(at)
    }

    fn store_word(&self, at: GuestAddress, value: u32) {
        self.set_word(at, value);
    }

    fn load_byte(&self, at: GuestAddress) -> u8 {
        self.byte(at)
    }

    fn store_byte(&self, at: GuestAddress, value: u8) {
        self.set_byte(at, value);
    }

    fn mask_interrupts(&self) -> SavedInterrupts {
        assert!(!self.masked.get(), "nested mask");
        self.masked.set(true);
        self.saved.set(0x80);
        SavedInterrupts(0x80)
    }

    fn restore_interrupts(&self, saved: SavedInterrupts) {
        assert!(self.masked.get(), "restore without mask");
        assert_eq!(saved, SavedInterrupts(0x80));
        self.masked.set(false);
    }

    fn dmb(&self) {
        self.barriers.set(self.barriers.get() + 1);
    }
}

const DOMAINS: [Domain; 2] = [Domain::A, Domain::B];

fn fixture(domain: Domain, slot: u32) -> (Mock, TaskSlot) {
    let mock = Mock::new();
    let slot = TaskSlot::new(slot).unwrap();
    let words = domain.words();
    let definition = words.task_definition(slot);
    let task = words.task(slot);
    mock.set_word(task.definition(), definition.0 .0);
    mock.set_word(task.state(), TaskState::DORMANT.bits() as u32);
    mock.set_word(task.priority(), 0xff);
    mock.set_byte(task.flags(), 0xff);
    mock.set_word(task.exception_pattern(), 0xeeee_eeee);
    mock.set_word(task.saved_sp(), 0x1111_1111);
    mock.set_word(task.resume_pc(), 0x2222_2222);
    mock.set_word(task.domain(), 0);
    mock.set_word(task.terminate_pending(), 0xaaaa_aaaa);
    mock.set_word(task.priority_change_pending(), 0xbbbb_bbbb);
    mock.set_word(task.cancel_pending(), 0xcccc_cccc);
    mock.set_word(task.context_discarded(), 0xdddd_dddd);
    mock.set_word(task.pending_priority(), 0x9999_9999);
    mock.set_word(definition.exists(), 0);
    mock.set_word(words.task_creation_id(slot).word(), 0);
    (mock, slot)
}

fn request() -> TaskRequest {
    TaskRequest::new(0x1111_1111, 0x2222_2222, GuestAddress(0x4000_1000), 16, 0x101, GuestAddress(0x0080_0000)).unwrap()
}

fn create_default(mock: &Mock, domain: Domain, slot: TaskSlot) -> Result<TaskSlot, TaskRecordError> {
    create(mock, domain, slot, request())
}

#[test]
fn create_writes_every_stock_record_field_in_both_domains() {
    for domain in DOMAINS {
        let (mock, slot) = fixture(domain, 7);
        let words = domain.words();
        let definition = words.task_definition(slot);
        let task = words.task(slot);
        let id = create_default(&mock, domain, slot).unwrap();
        assert_eq!(id, slot);
        assert_eq!(mock.word(definition.attributes()), 0x1111_1111);
        assert_eq!(mock.word(definition.argument()), 0x2222_2222);
        assert_eq!(mock.word(definition.entry()), 0x4000_1000);
        assert_eq!(mock.word(definition.priority()), 15);
        assert_eq!(mock.word(definition.stack_size()), 0x104);
        assert_eq!(mock.word(definition.stack()), 0x0080_0000);
        assert_eq!(mock.word(definition.exception_attributes()), 0);
        assert_eq!(mock.word(definition.exception_handler()), 0);
        assert_eq!(mock.word(definition.slot()), 7);
        assert_eq!(mock.word(definition.exists()), 1);
        assert_eq!(mock.word(words.task_creation_id(slot).word()), 7);
        assert_eq!(mock.byte(task.state()), TaskState::DORMANT.bits());
        assert_eq!(mock.byte(task.priority()), 15);
        assert_eq!(mock.byte(task.flags()), 0xf8);
        assert_eq!(mock.word(task.exception_pattern()), 0);
        assert_eq!(mock.word(task.domain()), domain.id().word());
        assert_eq!(mock.word(task.terminate_pending()), 0);
        assert_eq!(mock.word(task.priority_change_pending()), 0);
        assert_eq!(mock.word(task.cancel_pending()), 0);
        assert_eq!(mock.word(task.context_discarded()), 0);
        assert_eq!(mock.word(task.saved_sp()), 0x1111_1111);
        assert_eq!(mock.word(task.resume_pc()), 0x2222_2222);
        assert_eq!(mock.word(task.pending_priority()), 0x9999_9999);
        assert!(!mock.masked.get());
        assert_eq!(mock.barriers.get(), 1);
    }
}

#[test]
fn create_refuses_a_duplicate_without_writing() {
    for domain in DOMAINS {
        let (mock, slot) = fixture(domain, 3);
        let words = domain.words();
        mock.set_word(words.task_definition(slot).exists(), 1);
        let before = mock.snapshot();
        assert_eq!(
            create_default(&mock, domain, slot),
            Err(TaskRecordError::AlreadyExists)
        );
        assert_eq!(mock.snapshot().0, before.0);
        assert!(!mock.masked.get());
    }
}

#[test]
fn create_refuses_every_live_state_without_writing() {
    for domain in DOMAINS {
        for state in [
            TaskState::READY,
            TaskState::WAIT,
            TaskState::SUSPENDED,
            TaskState::READY.union(TaskState::WAIT),
            TaskState::WAIT.union(TaskState::OBJECT_QUEUED),
        ] {
            let (mock, slot) = fixture(domain, 5);
            let words = domain.words();
            mock.set_byte(words.task(slot).state(), state.bits());
            let before = mock.snapshot();
            assert_eq!(create_default(&mock, domain, slot), Err(TaskRecordError::TargetLive));
            assert_eq!(mock.snapshot().0, before.0);
        }
    }
}

#[test]
fn create_refuses_an_unbound_task_record_without_writing() {
    for domain in DOMAINS {
        let (mock, slot) = fixture(domain, 9);
        let words = domain.words();
        mock.set_word(words.task(slot).definition(), 0);
        let before = mock.snapshot();
        assert_eq!(create_default(&mock, domain, slot), Err(TaskRecordError::UnboundRecord));
        assert_eq!(mock.snapshot().0, before.0);
    }
}

#[test]
fn create_rejects_out_of_range_priority_and_stack_bytes() {
    assert_eq!(
        TaskRequest::new(0, 0, GuestAddress(0), 0, 0, GuestAddress(0)),
        Err(TaskRecordError::InvalidPriority)
    );
    assert_eq!(
        TaskRequest::new(0, 0, GuestAddress(0), 17, 0, GuestAddress(0)),
        Err(TaskRecordError::InvalidPriority)
    );
    assert_eq!(
        TaskRequest::new(0, 0, GuestAddress(0), 1, 0xffff_fffd, GuestAddress(0)),
        Err(TaskRecordError::InvalidStackSize)
    );
    assert!(TaskRequest::new(0, 0, GuestAddress(0), 1, 0xffff_fffc, GuestAddress(0)).is_ok());
}

#[test]
fn delete_reset_restores_both_stock_defaults() {
    for domain in DOMAINS {
        let (mock, slot) = fixture(domain, 11);
        let words = domain.words();
        let definition = words.task_definition(slot);
        let task = words.task(slot);
        create_default(&mock, domain, slot).unwrap();
        mock.set_word(task.terminate_pending(), 0xaaaa_aaaa);
        mock.set_word(task.priority_change_pending(), 0xbbbb_bbbb);
        mock.set_word(task.cancel_pending(), 0xcccc_cccc);
        mock.set_word(task.context_discarded(), 0xdddd_dddd);
        mock.set_word(task.exception_pattern(), 0xeeee_eeee);
        mock.set_byte(task.flags(), 0xff);
        mock.set_byte(task.state(), TaskState::DORMANT.bits());
        let before = mock.barriers.get();
        delete(&mock, domain, slot, DeleteMode::Reset).unwrap();
        assert_eq!(mock.word(definition.attributes()), 0);
        assert_eq!(mock.word(definition.argument()), 10);
        assert_eq!(mock.word(definition.entry()), 0);
        assert_eq!(mock.word(definition.priority()), 15);
        assert_eq!(mock.word(definition.stack_size()), 4);
        assert_eq!(mock.word(definition.stack()), words.default_stack_word.address().0);
        assert_eq!(mock.word(definition.exception_attributes()), 0);
        assert_eq!(mock.word(definition.exception_handler()), 0);
        assert_eq!(mock.word(definition.slot()), 11);
        assert_eq!(mock.word(definition.exists()), 0);
        assert_eq!(mock.word(words.task_creation_id(slot).word()), 11);
        assert_eq!(mock.byte(task.state()), TaskState::DORMANT.bits());
        assert_eq!(mock.byte(task.priority()), 15);
        assert_eq!(mock.byte(task.flags()), 0xf8);
        assert_eq!(mock.word(task.exception_pattern()), 0);
        assert_eq!(mock.word(task.domain()), domain.id().word());
        assert_eq!(mock.word(task.terminate_pending()), 0);
        assert_eq!(mock.word(task.priority_change_pending()), 0);
        assert_eq!(mock.word(task.cancel_pending()), 0);
        assert_eq!(mock.word(task.context_discarded()), 0);
        assert_eq!(mock.word(task.saved_sp()), 0x1111_1111);
        assert_eq!(mock.word(task.resume_pc()), 0x2222_2222);
        assert_eq!(mock.word(task.pending_priority()), 0x9999_9999);
        assert_eq!(mock.barriers.get(), before + 1);
        assert!(!mock.masked.get());
    }
}

#[test]
fn delete_clear_touches_only_the_existence_word() {
    for domain in DOMAINS {
        let (mock, slot) = fixture(domain, 13);
        let words = domain.words();
        create_default(&mock, domain, slot).unwrap();
        let before = mock.snapshot();
        delete(&mock, domain, slot, DeleteMode::Clear).unwrap();
        let after = mock.snapshot();
        assert_eq!(mock.word(words.task_definition(slot).exists()), 0);
        let mut expected = before;
        expected.set_word(words.task_definition(slot).exists(), 0);
        assert_eq!(after.0, expected.0);
        assert!(!mock.masked.get());
    }
}

#[test]
fn delete_reset_refuses_active_and_blocked_tasks_without_writing() {
    for domain in DOMAINS {
        for state in [
            TaskState::READY,
            TaskState::WAIT,
            TaskState::SUSPENDED,
            TaskState::READY.union(TaskState::WAIT),
            TaskState::WAIT.union(TaskState::OWNER_LOCK_QUEUED),
        ] {
            let (mock, slot) = fixture(domain, 15);
            let words = domain.words();
            create_default(&mock, domain, slot).unwrap();
            mock.set_byte(words.task(slot).state(), state.bits());
            let before = mock.snapshot();
            assert_eq!(
                delete(&mock, domain, slot, DeleteMode::Reset),
                Err(TaskRecordError::TargetLive)
            );
            assert_eq!(mock.snapshot().0, before.0);
        }
    }
}

#[test]
fn delete_clear_refuses_records_that_are_not_created() {
    for domain in DOMAINS {
        let (mock, slot) = fixture(domain, 17);
        let before = mock.snapshot();
        assert_eq!(delete(&mock, domain, slot, DeleteMode::Clear), Err(TaskRecordError::NotCreated));
        assert_eq!(mock.snapshot().0, before.0);
    }
}

#[test]
fn clear_reset_recreate_preserves_saved_task_context() {
    for domain in DOMAINS {
        let (mock, slot) = fixture(domain, 17);
        let words = domain.words();
        let definition = words.task_definition(slot);
        let task = words.task(slot);
        create_default(&mock, domain, slot).unwrap();
        delete(&mock, domain, slot, DeleteMode::Clear).unwrap();
        delete(&mock, domain, slot, DeleteMode::Reset).unwrap();
        assert_eq!(mock.word(definition.exists()), 0);
        assert_eq!(mock.word(definition.attributes()), 0);
        assert_eq!(mock.word(definition.argument()), 16);
        assert_eq!(mock.word(definition.entry()), 0);
        assert_eq!(mock.word(definition.priority()), 15);
        assert_eq!(mock.word(definition.stack_size()), 4);
        assert_eq!(mock.word(definition.stack()), words.default_stack_word.address().0);
        assert_eq!(mock.byte(task.state()), TaskState::DORMANT.bits());
        assert_eq!(mock.word(task.definition()), definition.0 .0);
        assert_eq!(mock.word(task.saved_sp()), 0x1111_1111);
        assert_eq!(mock.word(task.resume_pc()), 0x2222_2222);
        assert_eq!(mock.word(task.pending_priority()), 0x9999_9999);
        assert_eq!(create_default(&mock, domain, slot), Ok(slot));
        assert_eq!(mock.word(definition.exists()), 1);
        assert_eq!(mock.word(words.task_creation_id(slot).word()), slot.get());
        assert_eq!(mock.word(task.saved_sp()), 0x1111_1111);
        assert_eq!(mock.word(task.resume_pc()), 0x2222_2222);
        assert!(!mock.masked.get());
    }
}

#[test]
fn unpublished_reset_refuses_live_targets_without_writing() {
    for domain in DOMAINS {
        let (mock, slot) = fixture(domain, 17);
        mock.set_byte(domain.words().task(slot).state(), TaskState::READY.bits());
        let before = mock.snapshot();
        assert_eq!(delete(&mock, domain, slot, DeleteMode::Reset), Err(TaskRecordError::TargetLive));
        assert_eq!(mock.snapshot().0, before.0);
        assert!(!mock.masked.get());
    }
}

#[test]
fn unpublished_reset_refuses_unbound_targets_without_writing() {
    for domain in DOMAINS {
        let (mock, slot) = fixture(domain, 17);
        mock.set_word(domain.words().task(slot).definition(), 0);
        let before = mock.snapshot();
        assert_eq!(delete(&mock, domain, slot, DeleteMode::Reset), Err(TaskRecordError::UnboundRecord));
        assert_eq!(mock.snapshot().0, before.0);
        assert!(!mock.masked.get());
    }
}

#[test]
fn delete_reset_refuses_an_unbound_task_record_without_writing() {
    for domain in DOMAINS {
        let (mock, slot) = fixture(domain, 19);
        let words = domain.words();
        create_default(&mock, domain, slot).unwrap();
        mock.set_word(words.task(slot).definition(), 0);
        let before = mock.snapshot();
        assert_eq!(
            delete(&mock, domain, slot, DeleteMode::Reset),
            Err(TaskRecordError::UnboundRecord)
        );
        assert_eq!(mock.snapshot().0, before.0);
    }
}

#[test]
fn delete_clear_is_allowed_on_a_live_current_task() {
    let (mock, slot) = fixture(Domain::A, 21);
    let words = Domain::A.words();
    create_default(&mock, Domain::A, slot).unwrap();
    mock.set_byte(words.task(slot).state(), TaskState::READY.bits());
    delete(&mock, Domain::A, slot, DeleteMode::Clear).unwrap();
    assert_eq!(mock.word(words.task_definition(slot).exists()), 0);
}

#[test]
fn reset_then_create_reuses_the_record_without_stale_lifetime_fields() {
    for domain in DOMAINS {
        let (mock, slot) = fixture(domain, 23);
        let words = domain.words();
        let definition = words.task_definition(slot);
        let task = words.task(slot);
        create_default(&mock, domain, slot).unwrap();
        delete(&mock, domain, slot, DeleteMode::Reset).unwrap();
        let replacement = TaskRequest::new(
            0x3333_3333,
            0x4444_4444,
            GuestAddress(0x4000_2000),
            1,
            0x20,
            GuestAddress(0x0081_0000),
        )
        .unwrap();
        assert_eq!(create(&mock, domain, slot, replacement), Ok(slot));
        assert_eq!(mock.word(definition.attributes()), 0x3333_3333);
        assert_eq!(mock.word(definition.argument()), 0x4444_4444);
        assert_eq!(mock.word(definition.entry()), 0x4000_2000);
        assert_eq!(mock.word(definition.priority()), 0);
        assert_eq!(mock.word(definition.stack_size()), 0x20);
        assert_eq!(mock.word(definition.stack()), 0x0081_0000);
        assert_eq!(mock.word(definition.exception_handler()), 0);
        assert_eq!(mock.byte(task.priority()), 0);
        assert_eq!(mock.word(task.exception_pattern()), 0);
        assert_eq!(mock.word(task.terminate_pending()), 0);
        assert_eq!(mock.word(task.context_discarded()), 0);
    }
}

#[test]
fn clear_withdraws_the_definition_but_blocks_recreation_until_the_task_is_dormant() {
    let (mock, slot) = fixture(Domain::A, 25);
    let words = Domain::A.words();
    create_default(&mock, Domain::A, slot).unwrap();
    delete(&mock, Domain::A, slot, DeleteMode::Clear).unwrap();
    mock.set_byte(words.task(slot).state(), TaskState::READY.bits());
    assert_eq!(create_default(&mock, Domain::A, slot), Err(TaskRecordError::TargetLive));
    mock.set_byte(words.task(slot).state(), TaskState::DORMANT.bits());
    assert_eq!(create_default(&mock, Domain::A, slot), Ok(slot));
}

#[test]
fn status_mapping_matches_the_stock_abi_words() {
    assert_eq!(TaskRecordError::AlreadyExists.status(), -41);
    assert_eq!(TaskRecordError::NotCreated.status(), -41);
    assert_eq!(TaskRecordError::TargetLive.status(), -41);
    assert_eq!(TaskRecordError::InvalidSlot.status(), -18);
    assert_eq!(TaskRecordError::InvalidPriority.status(), -18);
    assert_eq!(TaskRecordError::InvalidStackSize.status(), -18);
    assert_eq!(TaskRecordError::UnboundRecord.status(), -18);
}

#[test]
fn domain_ids_are_the_stock_words() {
    assert_eq!(Domain::A.id(), DomainId::A);
    assert_eq!(Domain::B.id(), DomainId::B);
    assert_eq!(Domain::A.id().word(), 1);
    assert_eq!(Domain::B.id().word(), 2);
}

#[test]
fn every_slot_can_be_created_then_exhausted_and_reused_without_changing_context_identity() {
    for domain in DOMAINS {
        let mock = Mock::new();
        let words = domain.words();
        for raw in 1..=120 {
            let slot = TaskSlot::new(raw).unwrap();
            let task = words.task(slot);
            mock.set_word(task.definition(), words.task_definition(slot).0 .0);
            mock.set_word(task.saved_sp(), 0x8000_0000 + raw * 0x100);
            mock.set_word(task.resume_pc(), 0x4000_0000 + raw * 4);
            assert_eq!(create_default(&mock, domain, slot), Ok(slot));
        }
        let full = mock.snapshot();
        for raw in 1..=120 {
            let slot = TaskSlot::new(raw).unwrap();
            assert_eq!(create_default(&mock, domain, slot), Err(TaskRecordError::AlreadyExists));
        }
        assert_eq!(mock.snapshot().0, full.0);
        assert!(TaskSlot::new(0).is_none());
        assert!(TaskSlot::new(121).is_none());
        assert!(TaskSlot::new(u32::MAX).is_none());
        for raw in [1, 120] {
            let slot = TaskSlot::new(raw).unwrap();
            let task = words.task(slot);
            delete(&mock, domain, slot, DeleteMode::Reset).unwrap();
            assert_eq!(create_default(&mock, domain, slot), Ok(slot));
            assert_eq!(mock.word(task.definition()), words.task_definition(slot).0 .0);
            assert_eq!(mock.word(task.saved_sp()), 0x8000_0000 + raw * 0x100);
            assert_eq!(mock.word(task.resume_pc()), 0x4000_0000 + raw * 4);
            assert_eq!(mock.word(words.task_creation_id(slot).word()), raw);
        }
    }
}

#[test]
fn request_stack_rounding_and_priority_boundaries_are_visible_in_both_domains() {
    for domain in DOMAINS {
        for (bytes, rounded) in [(0, 0), (1, 4), (3, 4), (4, 4), (5, 8), (0xffff_fffc, 0xffff_fffc)] {
            for priority in [1, 16] {
                let (mock, slot) = fixture(domain, 120);
                let request = TaskRequest::new(0, 0, GuestAddress(0), priority, bytes, GuestAddress(0)).unwrap();
                assert_eq!(create(&mock, domain, slot, request), Ok(slot));
                let definition = domain.words().task_definition(slot);
                assert_eq!(mock.word(definition.stack_size()), rounded);
                assert_eq!(mock.word(definition.priority()), priority - 1);
            }
        }
    }
}

struct RequestGuard {
    memory: Mock,
    request: GuestAddress,
    readable_words: Vec<u32>,
}

impl Machine for RequestGuard {
    fn load_word(&self, at: GuestAddress) -> u32 {
        if at.0 >= self.request.0 && at.0 < self.request.0 + 24 {
            assert!(self.readable_words.contains(&(at.0 - self.request.0)), "request read outside admitted memory");
        }
        self.memory.load_word(at)
    }

    fn store_word(&self, at: GuestAddress, value: u32) {
        assert!(at.0 < self.request.0 || at.0 >= self.request.0 + 24, "request write");
        self.memory.store_word(at, value);
    }

    fn load_byte(&self, at: GuestAddress) -> u8 {
        self.memory.load_byte(at)
    }

    fn store_byte(&self, at: GuestAddress, value: u8) {
        self.memory.store_byte(at, value);
    }

    fn mask_interrupts(&self) -> SavedInterrupts {
        self.memory.mask_interrupts()
    }

    fn restore_interrupts(&self, saved: SavedInterrupts) {
        self.memory.restore_interrupts(saved);
    }

    fn dmb(&self) {
        self.memory.dmb();
    }
}

#[test]
fn refused_creation_does_not_dereference_unavailable_request_memory() {
    for domain in DOMAINS {
        for error in [TaskRecordError::AlreadyExists, TaskRecordError::TargetLive, TaskRecordError::UnboundRecord] {
            let (memory, slot) = fixture(domain, 1);
            let words = domain.words();
            match error {
                TaskRecordError::AlreadyExists => memory.set_word(words.task_definition(slot).exists(), 1),
                TaskRecordError::TargetLive => memory.set_byte(words.task(slot).state(), TaskState::READY.bits()),
                TaskRecordError::UnboundRecord => memory.set_word(words.task(slot).definition(), 0),
                _ => unreachable!(),
            }
            let before = memory.snapshot();
            let guard = RequestGuard { memory, request: GuestAddress(0x8000_0000), readable_words: vec![] };
            assert_eq!(create_masked_at(&guard, domain, slot, guard.request), Err(error));
            assert_eq!(guard.memory.snapshot().0, before.0);
        }
    }
}

#[test]
fn invalid_request_fields_refuse_before_reading_unavailable_payload_or_writing_records() {
    for domain in DOMAINS {
        for (priority, bytes, readable_words, error) in [
            (0, 0, vec![12], TaskRecordError::InvalidPriority),
            (17, 0, vec![12], TaskRecordError::InvalidPriority),
            (u32::MAX, 0, vec![12], TaskRecordError::InvalidPriority),
            (1, 0xffff_fffd, vec![12, 16], TaskRecordError::InvalidStackSize),
            (16, u32::MAX, vec![12, 16], TaskRecordError::InvalidStackSize),
        ] {
            let (memory, slot) = fixture(domain, 120);
            let request = GuestAddress(0x8000_0000);
            memory.set_word(request.offset(12), priority);
            memory.set_word(request.offset(16), bytes);
            let before = memory.snapshot();
            let guard = RequestGuard { memory, request, readable_words };
            assert_eq!(create_masked_at(&guard, domain, slot, request), Err(error));
            assert_eq!(guard.memory.snapshot().0, before.0);
        }
    }
}

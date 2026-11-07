use crate::layout::{
    DomainWords, FlagConfig, FlagControl, FlagWait, GuestAddress, LockRecord, MailboxControl, MailboxWait,
    Message, Task, TaskState,
};
use crate::sim::Sim;
use crate::*;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

fn entry() -> Entry {
    Entry {
        handler_context: 0,
        irq_masked: false,
        dispatch_enabled: 1,
    }
}

struct Tables {
    flags: u32,
    mailboxes: u32,
    exists: bool,
    attributes: u32,
    priority_limit: u32,
    priority: u32,
}

impl Tables {
    fn ready() -> Self {
        Self {
            flags: 120,
            mailboxes: 1,
            exists: true,
            attributes: 0,
            priority_limit: 32,
            priority: 0,
        }
    }
}

impl Records for Tables {
    fn flags(&self, _: Domain) -> u32 {
        self.flags
    }

    fn mailboxes(&self, _: Domain) -> u32 {
        self.mailboxes
    }
}

impl Facts for Tables {
    fn exists(&self, _: Object) -> bool {
        self.exists
    }

    fn attributes(&self, _: Object) -> u32 {
        self.attributes
    }

    fn priority_limit(&self, _: Object) -> u32 {
        self.priority_limit
    }

    fn message_priority(&self, _: Message) -> u32 {
        self.priority
    }
}

fn req(service: Service) -> Request {
    Request {
        service,
        entry: entry(),
        entry_domain: EntryDomain::new(Domain::A),
        id: EncodedId::of(1, 1),
        params: Params::NONE,
    }
}

fn admit_req(request: &Request) -> Result<Admitted, ObjectStatus> {
    let tables = Tables::ready();
    admit(request, &tables, &tables)
}

fn flag1(domain: Domain) -> FlagId {
    FlagId {
        domain,
        slot: FlagSlot::new(1, 120).unwrap(),
    }
}

fn mailbox1(domain: Domain) -> MailboxId {
    MailboxId {
        domain,
        slot: MailboxSlot::new(1, 1).unwrap(),
    }
}

#[test]
fn class_zero_is_the_entry_domain() {
    assert_eq!(
        resolve_flag(EntryDomain::new(Domain::A), EncodedId::of(0, 1), 120),
        Ok(flag1(Domain::A))
    );
    assert_eq!(
        resolve_flag(EntryDomain::new(Domain::B), EncodedId::of(0, 1), 120),
        Ok(flag1(Domain::B))
    );
    assert_eq!(
        resolve_mailbox(EntryDomain::new(Domain::B), EncodedId::of(0, 1), 1),
        Ok(mailbox1(Domain::B))
    );
}

#[test]
fn a_class_two_id_from_an_a_body_names_b() {
    assert_eq!(
        resolve_flag(EntryDomain::new(Domain::A), EncodedId::of(2, 1), 120),
        Ok(flag1(Domain::B))
    );
    assert_eq!(
        resolve_flag(EntryDomain::new(Domain::B), EncodedId::of(1, 1), 120),
        Ok(flag1(Domain::A))
    );
}

#[test]
fn create_names_the_entry_domain_whatever_the_class() {
    for (entry, expected) in [(Domain::A, Domain::A), (Domain::B, Domain::B)] {
        let mut request = req(Service::FlagCreate);
        request.entry_domain = EntryDomain::new(entry);
        request.id = EncodedId::of(2, 1);
        assert_eq!(
            admit_req(&request).map(|admitted| admitted.object),
            Ok(Object::Flag(flag1(expected))),
            "{entry:?}"
        );
    }
}

#[test]
fn a_foreign_class_is_refused_before_the_index() {
    assert_eq!(
        resolve_flag(EntryDomain::new(Domain::A), EncodedId::of(3, 0), 120),
        Err(ObjectStatus::InvalidClass)
    );
    assert_eq!(
        resolve_flag(EntryDomain::new(Domain::A), EncodedId(0xffff_0001u32 as i32), 120),
        Err(ObjectStatus::InvalidClass)
    );
}

#[test]
fn the_index_is_a_signed_halfword() {
    assert_eq!(EncodedId::of(1, 0xffff).index(), -1);
    assert_eq!(EncodedId::of(1, 0x8000).index(), -32768);
    assert_eq!(EncodedId::of(1, 0x8000).class(), 1);
    assert_eq!(
        resolve_flag(EntryDomain::new(Domain::A), EncodedId::of(1, 0x8000), 120),
        Err(ObjectStatus::InvalidIndex)
    );
    assert_eq!(
        resolve_flag(EntryDomain::new(Domain::A), EncodedId::of(0, 0), 120),
        Err(ObjectStatus::InvalidIndex)
    );
}

#[test]
fn the_index_is_bounded_by_the_live_count() {
    assert_eq!(
        resolve_flag(EntryDomain::new(Domain::A), EncodedId::of(1, 120), 120),
        Ok(FlagId {
            domain: Domain::A,
            slot: FlagSlot::new(120, 120).unwrap()
        })
    );
    assert_eq!(
        resolve_flag(EntryDomain::new(Domain::A), EncodedId::of(1, 121), 120),
        Err(ObjectStatus::InvalidIndex)
    );
    assert_eq!(
        resolve_flag(EntryDomain::new(Domain::A), EncodedId::of(1, 120), 119),
        Err(ObjectStatus::InvalidIndex)
    );
}

#[test]
fn create_and_delete_ignore_the_class_half() {
    assert_eq!(
        own_flag_slot(EncodedId::of(7, 1), 120),
        Ok(FlagSlot::new(1, 120).unwrap())
    );
    assert_eq!(
        own_mailbox_slot(EncodedId::of(0x7000, 1), 1),
        Ok(MailboxSlot::new(1, 1).unwrap())
    );
    assert_eq!(own_flag_slot(EncodedId::of(0, 0), 120), Err(ObjectStatus::InvalidIndex));
    assert_eq!(own_flag_slot(EncodedId::of(0, 121), 120), Err(ObjectStatus::InvalidIndex));
}

#[test]
fn create_has_no_context_check() {
    let mut request = req(Service::FlagCreate);
    request.entry = Entry {
        handler_context: 1,
        irq_masked: true,
        dispatch_enabled: 0,
    };
    assert_eq!(
        admit_req(&request).map(|admitted| admitted.object),
        Ok(Object::Flag(flag1(Domain::A)))
    );
}

#[test]
fn task_forms_refuse_handler_context() {
    let mut request = req(Service::FlagSet);
    request.entry.handler_context = 1;
    assert_eq!(admit_req(&request), Err(ObjectStatus::Context));
}

#[test]
fn the_handler_form_refuses_task_context() {
    assert_eq!(admit_req(&req(Service::FlagSetHandler)), Err(ObjectStatus::Context));
    let mut handler = req(Service::FlagSetHandler);
    handler.entry.handler_context = 1;
    assert!(admit_req(&handler).is_ok());
}

#[test]
fn masked_irq_is_refused() {
    let mut request = req(Service::FlagClear);
    request.entry.irq_masked = true;
    assert_eq!(admit_req(&request), Err(ObjectStatus::Context));
}

#[test]
fn only_the_blocking_forms_read_the_dispatch_word() {
    for service in [
        Service::FlagWait,
        Service::FlagTimedWait,
        Service::MailboxReceive,
        Service::MailboxTimedReceive,
    ] {
        let mut request = req(service);
        request.entry.dispatch_enabled = 0;
        request.params.pattern = Pattern(1);
        assert_eq!(admit_req(&request), Err(ObjectStatus::Context), "{service:?}");
    }
    for service in [
        Service::FlagSet,
        Service::FlagClear,
        Service::FlagPoll,
        Service::MailboxSend,
        Service::MailboxPoll,
    ] {
        let mut request = req(service);
        request.entry.dispatch_enabled = 0;
        request.params.pattern = Pattern(1);
        assert!(admit_req(&request).is_ok(), "{service:?}");
    }
}

fn send_request() -> Request {
    let mut request = req(Service::MailboxSend);
    request.params.message = Some(Message(GuestAddress(0)));
    request
}

#[test]
fn send_checks_existence_before_the_priority() {
    let request = send_request();
    let tables = Tables { exists: false, attributes: 2, ..Tables::ready() };
    assert_eq!(admit(&request, &tables, &tables), Err(ObjectStatus::NotCreated));
}

#[test]
fn send_refuses_a_priority_outside_the_limit() {
    for priority in [0, 33, 0xffff_ffff] {
        let request = send_request();
        let tables = Tables { attributes: 2, priority, ..Tables::ready() };
        assert_eq!(admit(&request, &tables, &tables), Err(ObjectStatus::Param), "{priority}");
    }
    let accepted = send_request();
    let tables = Tables { attributes: 2, priority: 32, ..Tables::ready() };
    assert_eq!(
        admit(&accepted, &tables, &tables).map(|admitted| admitted.priority),
        Ok(Some(MessagePriority::new(32, PriorityLimit(32)).unwrap()))
    );
}

#[test]
fn a_negative_priority_is_refused_whatever_the_limit() {
    assert_eq!(MessagePriority::new(0xffff_ffff, PriorityLimit(0xffff_ffff)), Err(ObjectStatus::Param));
    assert_eq!(MessagePriority::new(0x8000_0000, PriorityLimit(0xffff_ffff)), Err(ObjectStatus::Param));
    assert_eq!(MessagePriority::new(0, PriorityLimit(0xffff_ffff)), Err(ObjectStatus::Param));
    assert_eq!(
        MessagePriority::new(1, PriorityLimit(0xffff_ffff)),
        Err(ObjectStatus::Param),
        "a negative limit admits nothing"
    );
    let request = send_request();
    let tables = Tables { attributes: 2, priority_limit: 0xffff_ffff, priority: 0xffff_ffff, ..Tables::ready() };
    assert_eq!(admit(&request, &tables, &tables), Err(ObjectStatus::Param));
}

#[test]
fn send_ignores_the_priority_without_the_attribute() {
    let request = send_request();
    assert_eq!(admit_req(&request).map(|admitted| admitted.priority), Ok(None));
}

#[test]
fn timed_receive_checks_the_timeout_before_existence() {
    let mut request = req(Service::MailboxTimedReceive);
    request.params.timeout = Some(-2);
    let tables = Tables { exists: false, ..Tables::ready() };
    assert_eq!(admit(&request, &tables, &tables), Err(ObjectStatus::Param));
}

#[test]
fn flag_wait_refuses_a_zero_pattern_before_existence() {
    let request = req(Service::FlagWait);
    let tables = Tables { exists: false, ..Tables::ready() };
    assert_eq!(admit(&request, &tables, &tables), Err(ObjectStatus::Param));
}

#[test]
fn flag_timed_wait_checks_the_mode_before_existence() {
    let mut request = req(Service::FlagTimedWait);
    request.params.pattern = Pattern(1);
    request.params.mode = 2;
    request.params.timeout = Some(-1);
    let tables = Tables { exists: false, ..Tables::ready() };
    assert_eq!(admit(&request, &tables, &tables), Err(ObjectStatus::Param));
}

#[test]
fn flag_set_accepts_zero_bits() {
    let mut request = req(Service::FlagSet);
    request.params.pattern = Pattern(0);
    assert!(admit_req(&request).is_ok());
}

#[test]
fn the_index_is_checked_before_the_creation_record() {
    let mut request = req(Service::FlagSet);
    request.id = EncodedId::of(1, 121);
    let tables = Tables { exists: false, ..Tables::ready() };
    assert_eq!(admit(&request, &tables, &tables), Err(ObjectStatus::InvalidIndex));
}

#[test]
fn admit_returns_the_validated_parameters() {
    let mut request = req(Service::FlagTimedWait);
    request.params.pattern = Pattern(0x10);
    request.params.mode = 1;
    request.params.timeout = Some(5);
    let admitted = admit_req(&request).unwrap();
    assert_eq!(admitted.object, Object::Flag(flag1(Domain::A)));
    assert_eq!(admitted.want, Pattern(0x10));
    assert_eq!(admitted.mode, MatchMode::Any);
    assert_eq!(admitted.timeout, Some(Timeout::Units(5)));
}

#[test]
fn a_second_flag_waiter_is_refused() {
    assert_eq!(flag_wait_locked(true), Err(ObjectStatus::QueueOverflow));
    assert_eq!(flag_wait_locked(false), Ok(()));
}

#[test]
fn the_match_reports_the_whole_pattern() {
    let mut pattern = Pattern(0x30);
    assert_eq!(check(&mut pattern, Pattern(0x10), MatchMode::All, false), Some(Pattern(0x30)));
    assert_eq!(pattern, Pattern(0x30));
}

#[test]
fn all_bits_are_required() {
    let mut pattern = Pattern(0x30);
    assert_eq!(check(&mut pattern, Pattern(0x50), MatchMode::All, false), None);
    assert_eq!(pattern, Pattern(0x30));
}

#[test]
fn any_bit_suffices() {
    let mut pattern = Pattern(0x30);
    assert_eq!(check(&mut pattern, Pattern(0x50), MatchMode::Any, false), Some(Pattern(0x30)));
}

#[test]
fn clear_on_match_zeroes_the_whole_word() {
    let mut pattern = Pattern(0x31);
    assert_eq!(check(&mut pattern, Pattern(0x01), MatchMode::All, true), Some(Pattern(0x31)));
    assert_eq!(pattern, Pattern(0));
}

#[test]
fn mode_words_above_one_are_refused() {
    assert_eq!(MatchMode::from_word(0), Ok(MatchMode::All));
    assert_eq!(MatchMode::from_word(1), Ok(MatchMode::Any));
    assert_eq!(MatchMode::from_word(2), Err(ObjectStatus::Param));
    assert_eq!(MatchMode::from_word(0xffff_ffff), Err(ObjectStatus::Param));
}

#[test]
fn priority_order_stops_at_the_first_greater_priority() {
    assert!(!precedes(QueueOrder::Priority, 3, 3));
    assert!(precedes(QueueOrder::Priority, 3, 5));
    assert!(!precedes(QueueOrder::Priority, 3, 1));
    assert!(!precedes(QueueOrder::Priority, 5, 3));
}

#[test]
fn fifo_never_precedes() {
    assert!(!precedes(QueueOrder::Fifo, 0, 5));
    assert!(!precedes(QueueOrder::Fifo, 9, 0));
}

#[test]
fn attribute_bit_zero_selects_priority_order() {
    assert_eq!(QueueOrder::from_attribute(0), QueueOrder::Fifo);
    assert_eq!(QueueOrder::from_attribute(1), QueueOrder::Priority);
    assert_eq!(QueueOrder::from_attribute(2), QueueOrder::Fifo);
    assert_eq!(QueueOrder::from_attribute(3), QueueOrder::Priority);
}

#[test]
fn timeouts_encode_forever_poll_and_units() {
    assert_eq!(Timeout::from_stock(-1), Ok(Timeout::Forever));
    assert_eq!(Timeout::from_stock(0), Ok(Timeout::Poll));
    assert_eq!(Timeout::from_stock(5), Ok(Timeout::Units(5)));
    assert_eq!(Timeout::from_stock(-2), Err(ObjectStatus::Param));
    assert_eq!(Timeout::Forever.deadline(7), None);
    assert_eq!(Timeout::Poll.deadline(7), None);
    assert_eq!(Timeout::Units(5).deadline(7), Some(12));
    assert_eq!(Timeout::Units(5).deadline(0xffff_fffe), Some(3));
}

#[test]
fn status_codes_are_the_stock_codes() {
    assert_eq!(
        [
            ObjectStatus::Context,
            ObjectStatus::InvalidClass,
            ObjectStatus::InvalidIndex,
            ObjectStatus::AlreadyExists,
            ObjectStatus::NotCreated,
            ObjectStatus::Param,
            ObjectStatus::QueueOverflow,
        ]
        .map(ObjectStatus::code),
        [-25, -18, -18, -41, -42, -17, -28]
    );
}

#[test]
fn deleted_and_released_share_the_abi_code() {
    assert_eq!(WaitOutcome::<Pattern>::Released.code(), -49);
    assert_eq!(WaitOutcome::<Pattern>::Deleted.code(), -49);
    assert_eq!(WaitOutcome::<Pattern>::TimedOut.code(), -50);
    assert_eq!(WaitOutcome::Satisfied(Pattern(3)).code(), 0);
}

#[test]
fn a_poll_without_a_match_is_a_timeout() {
    assert_eq!(poll_result(None), WaitOutcome::TimedOut);
    assert_eq!(poll_result(Some(Pattern(3))), WaitOutcome::Satisfied(Pattern(3)));
    assert_eq!(poll_result(None).code(), -50);
}

struct Mock {
    words: RefCell<BTreeMap<u32, u32>>,
    marker: u32,
    released: Cell<bool>,
    interrupts: Cell<SavedInterrupts>,
    creation_mask: Cell<bool>,
    held: Cell<bool>,
}

impl Default for Mock {
    fn default() -> Self {
        Self::marking(u32::MAX)
    }
}

impl Mock {
    fn marking(marker: u32) -> Self {
        Self {
            words: RefCell::new(BTreeMap::new()),
            marker,
            released: Cell::new(false),
            interrupts: Cell::new(SavedInterrupts(0x40)),
            creation_mask: Cell::new(false),
            held: Cell::new(false),
        }
    }

    fn word(&self, at: GuestAddress) -> u32 {
        *self.words.borrow().get(&at.0).unwrap_or(&0)
    }

    fn set(&self, at: GuestAddress, value: u32) {
        self.words.borrow_mut().insert(at.0, value);
    }

    fn set_byte(&self, at: GuestAddress, value: u8) {
        let aligned = GuestAddress(at.0 & !3);
        let shift = 8 * (at.0 & 3);
        let kept = self.word(aligned) & !(0xff << shift);
        self.set(aligned, kept | (value as u32) << shift);
    }
}

impl Machine for Mock {
    fn load_word(&self, at: GuestAddress) -> u32 {
        self.word(at)
    }

    fn load_byte(&self, at: GuestAddress) -> u8 {
        let aligned = GuestAddress(at.0 & !3);
        (self.word(aligned) >> (8 * (at.0 & 3))) as u8
    }

    fn store_byte(&self, at: GuestAddress, value: u8) {
        assert_ne!(self.interrupts.get().0 & 0x80, 0, "store with IRQ unmasked");
        self.set_byte(at, value);
    }

    fn store_word(&self, at: GuestAddress, value: u32) {
        let state = self.interrupts.get().0;
        assert_ne!(state & 0x80, 0, "store with IRQ unmasked");
        if self.creation_mask.get() {
            assert_eq!(state & 0xc0, 0xc0, "creation store with FIQ unmasked");
            self.creation_mask.set(false);
        }
        if at.0 == self.marker && value != 0 {
            assert!(self.released.get(), "publication marker stored before the release");
        }
        self.released.set(false);
        self.set(at, value);
    }

    fn mask_interrupts(&self) -> SavedInterrupts {
        self.creation_mask.set(true);
        self.interrupts.replace(SavedInterrupts(0xc0))
    }

    fn restore_interrupts(&self, saved: SavedInterrupts) {
        self.interrupts.set(saved);
    }

    fn dmb(&self) {
        self.released.set(true);
    }
}

impl ObjectLock for Mock {
    fn with_object_lock_after<R>(&self, _lock: LockRecord, body: impl FnOnce() -> R, after: impl FnOnce(&R)) -> R {
        assert!(!self.held.get(), "object lock acquired twice");
        self.creation_mask.set(false);
        let saved = self.interrupts.get();
        self.interrupts.set(SavedInterrupts(saved.0 | 0x80));
        self.held.set(true);
        let result = body();
        assert!(self.held.get(), "object lock released while not held");
        self.held.set(false);
        after(&result);
        self.interrupts.set(saved);
        result
    }

    fn with_object_lock<R>(&self, _lock: LockRecord, body: impl FnOnce() -> R) -> R {
        assert!(!self.held.get(), "object lock acquired twice");
        self.creation_mask.set(false);
        let saved = self.interrupts.get();
        self.interrupts.set(SavedInterrupts(saved.0 | 0x80));
        self.held.set(true);
        let result = body();
        assert!(self.held.get(), "object lock released while not held");
        self.held.set(false);
        self.interrupts.set(saved);
        result
    }
}

#[test]
fn create_flag_writes_the_record_and_self_links_the_queue() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let m = Mock::marking(words.flag_config(slot).exists().0);
    let id = FlagId {
        domain: Domain::A,
        slot,
    };
    assert_eq!(
        create_flag(&m, id, FlagCreate { attributes: FlagAttributes(4), initial: Pattern(0x21) }),
        Ok(id)
    );
    let config = words.flag_config(slot);
    assert_eq!(m.word(config.attributes()), 4);
    assert_eq!(m.word(config.initial()), 0x21);
    assert_eq!(m.word(config.exists()), 1);
    let control = words.flag_control(slot);
    assert_eq!(m.word(control.queue().next()), control.0 .0);
    assert_eq!(m.word(control.queue().prev()), control.0 .0);
    assert_eq!(m.word(control.pattern()), 0x21);
    assert_eq!(m.interrupts.get(), SavedInterrupts(0x40));
}

#[test]
fn create_flag_refuses_an_existing_record_without_writing() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let m = Mock::marking(words.flag_config(slot).exists().0);
    let config = words.flag_config(slot);
    m.set(config.exists(), 1);
    assert_eq!(
        create_flag(
            &m,
            FlagId {
                domain: Domain::A,
                slot
            },
            FlagCreate {
                attributes: FlagAttributes(4),
                initial: Pattern(0x21)
            }
        ),
        Err(ObjectStatus::AlreadyExists)
    );
    assert_eq!(m.word(config.attributes()), 0);
    assert_eq!(m.word(config.initial()), 0);
    assert_eq!(m.interrupts.get(), SavedInterrupts(0x40));
}

#[test]
fn create_mailbox_writes_the_record_and_clears_the_ends() {
    let words = Domain::A.words();
    let slot = MailboxSlot::new(1, 1).unwrap();
    let m = Mock::marking(words.mailbox_config(slot).exists().0);
    let id = MailboxId {
        domain: Domain::A,
        slot,
    };
    assert_eq!(
        create_mailbox(
            &m,
            id,
            MailboxCreate {
                attributes: MailboxAttributes(2),
                priority_limit: PriorityLimit(32)
            }
        ),
        Ok(id)
    );
    let config = words.mailbox_config(slot);
    assert_eq!(m.word(config.attributes()), 2);
    assert_eq!(m.word(config.priority_limit()), 32);
    assert_eq!(m.word(config.exists()), 1);
    let control = words.mailbox_control(slot);
    assert_eq!(m.word(control.queue().next()), control.0 .0);
    assert_eq!(m.word(control.head()), 0);
    assert_eq!(m.word(control.tail()), 0);
    assert_eq!(m.interrupts.get(), SavedInterrupts(0x40));
}

#[test]
fn create_mailbox_refuses_an_existing_record() {
    let words = Domain::A.words();
    let slot = MailboxSlot::new(1, 1).unwrap();
    let m = Mock::marking(words.mailbox_config(slot).exists().0);
    let config = words.mailbox_config(slot);
    m.set(config.exists(), 1);
    assert_eq!(
        create_mailbox(
            &m,
            MailboxId {
                domain: Domain::A,
                slot
            },
            MailboxCreate {
                attributes: MailboxAttributes(2),
                priority_limit: PriorityLimit(32)
            }
        ),
        Err(ObjectStatus::AlreadyExists)
    );
    assert_eq!(m.word(config.attributes()), 0);
    assert_eq!(m.interrupts.get(), SavedInterrupts(0x40));
}

#[test]
fn create_flag_publishes_after_the_release() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(4, 120).unwrap();
    let m = Mock::marking(words.flag_config(slot).exists().0);
    let id = FlagId {
        domain: Domain::A,
        slot,
    };
    assert_eq!(
        create_flag(&m, id, FlagCreate { attributes: FlagAttributes(0), initial: Pattern(0) }),
        Ok(id)
    );
    assert_eq!(m.word(words.flag_config(slot).exists()), 1);
    assert_eq!(m.interrupts.get(), SavedInterrupts(0x40));
}

#[test]
fn a_refused_create_leaves_the_record_unchanged() {
    let words = Domain::B.words();
    let slot = FlagSlot::new(2, 120).unwrap();
    let m = Mock::marking(words.flag_config(slot).exists().0);
    let config = words.flag_config(slot);
    let control = words.flag_control(slot);
    m.set(config.exists(), 1);
    m.set(config.attributes(), 0x1111);
    m.set(config.initial(), 0x2222);
    m.set(control.pattern(), 0x3333);
    m.set(control.queue().next(), 0x4444);
    assert_eq!(
        create_flag(
            &m,
            FlagId {
                domain: Domain::B,
                slot
            },
            FlagCreate {
                attributes: FlagAttributes(4),
                initial: Pattern(0x21)
            }
        ),
        Err(ObjectStatus::AlreadyExists)
    );
    assert_eq!(m.word(config.attributes()), 0x1111);
    assert_eq!(m.word(config.initial()), 0x2222);
    assert_eq!(m.word(control.pattern()), 0x3333);
    assert_eq!(m.word(control.queue().next()), 0x4444);
    assert_eq!(m.interrupts.get(), SavedInterrupts(0x40));
}

#[test]
fn a_create_through_admit_writes_the_entry_domain_table() {
    let mut request = req(Service::FlagCreate);
    let m = Mock::marking(Domain::B.words().flag_config(FlagSlot::new(5, 120).unwrap()).exists().0);
    request.entry_domain = EntryDomain::new(Domain::B);
    request.id = EncodedId::of(2, 5);
    let admitted = admit_req(&request).unwrap();
    let id = match admitted.object {
        Object::Flag(id) => id,
        Object::Mailbox(_) => panic!("flag create admitted a mailbox"),
    };
    assert_eq!(id.domain, Domain::B);
    assert_eq!(
        create_flag(&m, id, FlagCreate { attributes: FlagAttributes(4), initial: Pattern(1) }),
        Ok(id)
    );
    assert_eq!(m.word(Domain::B.words().flag_config(id.slot).exists()), 1);
    assert_eq!(m.word(Domain::A.words().flag_config(id.slot).exists()), 0);
}

fn flag_fixture(m: &Mock, domain: Domain, slot: FlagSlot, attributes: u32, initial: u32) -> FlagId {
    let id = FlagId { domain, slot };
    assert_eq!(
        create_flag(
            m,
            id,
            FlagCreate {
                attributes: FlagAttributes(attributes),
                initial: Pattern(initial)
            }
        ),
        Ok(id)
    );
    id
}

fn mailbox_fixture(m: &Mock, slot: MailboxSlot, attributes: u32, limit: u32) -> MailboxId {
    let id = MailboxId {
        domain: Domain::A,
        slot,
    };
    assert_eq!(
        create_mailbox(
            m,
            id,
            MailboxCreate {
                attributes: MailboxAttributes(attributes),
                priority_limit: PriorityLimit(limit)
            }
        ),
        Ok(id)
    );
    id
}

#[test]
fn clear_flag_masks_the_pattern_under_the_lock() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let m = Mock::marking(words.flag_config(slot).exists().0);
    let id = flag_fixture(&m, Domain::A, slot, 0, 0x31);
    let pattern = words.flag_control(slot).pattern();
    assert_eq!(clear_flag(&m, id, KeepMask(0x11)), Ok(()));
    assert_eq!(m.word(pattern), 0x11);
    assert!(!m.held.get());
    assert_eq!(m.interrupts.get(), SavedInterrupts(0x40));
}

#[test]
fn a_zero_wanted_mask_is_refused() {
    assert_eq!(WantedMask::new(Pattern(0)), Err(ObjectStatus::Param));
    assert_eq!(
        WantedMask::new(Pattern(1)).map(WantedMask::pattern),
        Ok(Pattern(1))
    );
}

#[test]
fn poll_flag_reports_the_whole_pattern() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let m = Mock::marking(words.flag_config(slot).exists().0);
    let id = flag_fixture(&m, Domain::A, slot, 0, 0x30);
    let out = GuestAddress(0x4000_1000);
    assert_eq!(
        poll_flag(
            &m,
            id,
            WantedMask::new(Pattern(0x10)).unwrap(),
            MatchMode::All,
            OutWord(out)
        ),
        Ok(WaitOutcome::Satisfied(Pattern(0x30)))
    );
    assert_eq!(m.word(out), 0x30);
    assert_eq!(m.word(words.flag_control(slot).pattern()), 0x30);
    assert!(!m.held.get());
    assert_eq!(m.interrupts.get(), SavedInterrupts(0x40));
}

#[test]
fn poll_flag_clears_on_match_after_reporting() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let m = Mock::marking(words.flag_config(slot).exists().0);
    let id = flag_fixture(&m, Domain::A, slot, FlagConfig::CLEAR_ON_MATCH, 0x31);
    let out = GuestAddress(0x4000_1000);
    assert_eq!(
        poll_flag(
            &m,
            id,
            WantedMask::new(Pattern(0x01)).unwrap(),
            MatchMode::All,
            OutWord(out)
        ),
        Ok(WaitOutcome::Satisfied(Pattern(0x31)))
    );
    assert_eq!(m.word(out), 0x31);
    assert_eq!(m.word(words.flag_control(slot).pattern()), 0);
}

#[test]
fn poll_flag_refuses_a_queued_waiter_before_matching() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let m = Mock::marking(words.flag_config(slot).exists().0);
    let id = flag_fixture(&m, Domain::A, slot, 0, 0x30);
    let control = words.flag_control(slot);
    let out = GuestAddress(0x4000_1000);
    m.set(control.queue().next(), 0x1234);
    m.set(out, 0xdead);
    assert_eq!(
        poll_flag(
            &m,
            id,
            WantedMask::new(Pattern(0x10)).unwrap(),
            MatchMode::All,
            OutWord(out)
        ),
        Err(ObjectStatus::QueueOverflow)
    );
    assert_eq!(m.word(out), 0xdead);
    assert_eq!(m.word(control.pattern()), 0x30);
}

#[test]
fn poll_flag_without_a_match_times_out_and_leaves_the_output() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let m = Mock::marking(words.flag_config(slot).exists().0);
    let id = flag_fixture(&m, Domain::A, slot, 0, 0x30);
    let out = GuestAddress(0x4000_1000);
    m.set(out, 0xdead);
    assert_eq!(
        poll_flag(
            &m,
            id,
            WantedMask::new(Pattern(0x40)).unwrap(),
            MatchMode::All,
            OutWord(out)
        ),
        Ok(WaitOutcome::TimedOut)
    );
    assert_eq!(m.word(out), 0xdead);
    assert_eq!(m.word(words.flag_control(slot).pattern()), 0x30);
}

#[test]
fn poll_receive_unlinks_the_head_message() {
    let words = Domain::A.words();
    let slot = MailboxSlot::new(1, 1).unwrap();
    let m = Mock::marking(words.mailbox_config(slot).exists().0);
    let id = mailbox_fixture(&m, slot, 0, 32);
    let control = words.mailbox_control(slot);
    let out = GuestAddress(0x4000_1000);
    let first = Message(GuestAddress(0x4000_0100));
    let second = Message(GuestAddress(0x4000_0200));
    m.set(control.head(), first.0 .0);
    m.set(first.next(), second.0 .0);
    assert_eq!(
        poll_receive(&m, id, OutWord(out)),
        Ok(WaitOutcome::Satisfied(first))
    );
    assert_eq!(m.word(out), first.0 .0);
    assert_eq!(m.word(control.head()), second.0 .0);
    assert!(!m.held.get());
    assert_eq!(m.interrupts.get(), SavedInterrupts(0x40));
}

#[test]
fn poll_receive_on_an_empty_mailbox_times_out() {
    let words = Domain::A.words();
    let slot = MailboxSlot::new(1, 1).unwrap();
    let m = Mock::marking(words.mailbox_config(slot).exists().0);
    let id = mailbox_fixture(&m, slot, 0, 32);
    let out = GuestAddress(0x4000_1000);
    m.set(out, 0xdead);
    assert_eq!(poll_receive(&m, id, OutWord(out)), Ok(WaitOutcome::TimedOut));
    assert_eq!(m.word(out), 0xdead);
    assert_eq!(m.word(words.mailbox_control(slot).head()), 0);
}

#[test]
fn poll_flag_reads_the_object_domain_table() {
    let a = Domain::A.words();
    let b = Domain::B.words();
    let slot = FlagSlot::new(2, 120).unwrap();
    let m = Mock::marking(b.flag_config(slot).exists().0);
    let id = flag_fixture(&m, Domain::B, slot, 0, 0x0f);
    let out = GuestAddress(0x4000_1000);
    assert_eq!(
        poll_flag(
            &m,
            id,
            WantedMask::new(Pattern(0x01)).unwrap(),
            MatchMode::All,
            OutWord(out)
        ),
        Ok(WaitOutcome::Satisfied(Pattern(0x0f)))
    );
    assert_eq!(m.word(out), 0x0f);
    assert_eq!(m.word(a.flag_control(slot).pattern()), 0);
    assert_eq!(m.word(a.flag_config(slot).exists()), 0);
}

#[test]
fn a_receive_output_aliasing_the_next_link_still_pops() {
    let words = Domain::A.words();
    let slot = MailboxSlot::new(1, 1).unwrap();
    let m = Mock::marking(words.mailbox_config(slot).exists().0);
    let id = mailbox_fixture(&m, slot, 0, 32);
    let control = words.mailbox_control(slot);
    let first = Message(GuestAddress(0x4000_0100));
    let scratch = GuestAddress(0x4000_1000);
    m.set(control.head(), first.0 .0);
    m.set(first.next(), 0);
    assert_eq!(
        poll_receive(&m, id, OutWord(first.next())),
        Ok(WaitOutcome::Satisfied(first))
    );
    assert_eq!(
        poll_receive(&m, id, OutWord(scratch)),
        Ok(WaitOutcome::TimedOut)
    );
    assert_eq!(m.word(control.head()), 0);
}

#[test]
fn a_poll_output_aliasing_the_attributes_keeps_the_clear() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let m = Mock::marking(words.flag_config(slot).exists().0);
    let id = flag_fixture(&m, Domain::A, slot, FlagConfig::CLEAR_ON_MATCH, 0x31);
    let config = words.flag_config(slot);
    let out = GuestAddress(0x4000_1000);
    assert_eq!(
        poll_flag(
            &m,
            id,
            WantedMask::new(Pattern(0x01)).unwrap(),
            MatchMode::All,
            OutWord(config.attributes())
        ),
        Ok(WaitOutcome::Satisfied(Pattern(0x31)))
    );
    assert_eq!(m.word(words.flag_control(slot).pattern()), 0);
    assert_eq!(
        poll_flag(
            &m,
            id,
            WantedMask::new(Pattern(0x01)).unwrap(),
            MatchMode::All,
            OutWord(out)
        ),
        Ok(WaitOutcome::TimedOut)
    );
}

#[test]
fn a_poll_output_aliasing_the_pattern_loses_to_the_clear() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let m = Mock::marking(words.flag_config(slot).exists().0);
    let id = flag_fixture(&m, Domain::A, slot, FlagConfig::CLEAR_ON_MATCH, 0x31);
    let pattern = words.flag_control(slot).pattern();
    assert_eq!(
        poll_flag(
            &m,
            id,
            WantedMask::new(Pattern(0x01)).unwrap(),
            MatchMode::All,
            OutWord(pattern)
        ),
        Ok(WaitOutcome::Satisfied(Pattern(0x31)))
    );
    assert_eq!(m.word(pattern), 0);
}

#[test]
fn a_receive_output_aliasing_the_head_reads_the_next_message() {
    let words = Domain::A.words();
    let slot = MailboxSlot::new(1, 1).unwrap();
    let m = Mock::marking(words.mailbox_config(slot).exists().0);
    let id = mailbox_fixture(&m, slot, 0, 32);
    let control = words.mailbox_control(slot);
    let first = Message(GuestAddress(0x4000_0100));
    let second = Message(GuestAddress(0x4000_0200));
    m.set(control.head(), first.0 .0);
    m.set(first.next(), second.0 .0);
    assert_eq!(
        poll_receive(&m, id, OutWord(control.head())),
        Ok(WaitOutcome::Satisfied(first))
    );
    assert_eq!(m.word(control.head()), second.0 .0);
}

fn install(sim: &Sim, owner: Domain) {
    sim.install(owner, true);
}

fn queue_flag_waiter(
    sim: &Sim,
    words: &DomainWords,
    control: FlagControl,
    task_slot: TaskSlot,
    domain: Domain,
    want: u32,
    mode: u32,
    timer: u32,
) -> (Task, FlagWait) {
    let task = words.task(task_slot);
    let wait = words.flag_wait(task_slot);
    sim.set(task.domain(), if domain == Domain::A { 1 } else { 2 });
    sim.set(task.wait(), wait.0 .0);
    sim.set_byte(task.priority(), 5);
    sim.set_byte(task.state(), 0x32);
    sim.set(wait.pattern(), want);
    sim.set(wait.mode(), mode);
    sim.set(wait.base().result(), timer);
    sim.set(control.queue().next(), task.0 .0);
    sim.set(control.queue().prev(), task.0 .0);
    sim.set(task.next(), control.0 .0);
    sim.set(task.prev(), control.0 .0);
    (task, wait)
}

fn queue_receiver(
    sim: &Sim,
    words: &DomainWords,
    control: MailboxControl,
    task_slot: TaskSlot,
    domain: Domain,
) -> (Task, MailboxWait) {
    let task = words.task(task_slot);
    let wait = words.mailbox_wait(task_slot);
    sim.set(task.domain(), if domain == Domain::A { 1 } else { 2 });
    sim.set(task.wait(), wait.0 .0);
    sim.set_byte(task.priority(), 5);
    sim.set_byte(task.state(), 0x32);
    sim.set(wait.base().result(), 0);
    sim.set(control.queue().next(), task.0 .0);
    sim.set(control.queue().prev(), task.0 .0);
    sim.set(task.next(), control.0 .0);
    sim.set(task.prev(), control.0 .0);
    (task, wait)
}

#[test]
fn set_flag_without_a_waiter_sets_the_pattern() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let sim = Sim::new(Domain::A);
    let id = FlagId {
        domain: Domain::A,
        slot,
    };
    assert_eq!(
        create_flag(&sim, id, FlagCreate { attributes: FlagAttributes(0), initial: Pattern(0x01) }),
        Ok(id)
    );
    set_flag(&sim, id, Pattern(0x10));
    assert_eq!(sim.word(words.flag_control(slot).pattern()), 0x11);
    assert_eq!(sim.selected(Domain::A), 0);
    assert!(!sim.doorbell(0) && !sim.doorbell(1));
}

#[test]
fn set_flag_makes_a_satisfied_waiter_ready() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = FlagId {
        domain: Domain::A,
        slot,
    };
    assert_eq!(
        create_flag(&sim, id, FlagCreate { attributes: FlagAttributes(0), initial: Pattern(0x01) }),
        Ok(id)
    );
    let control = words.flag_control(slot);
    let (task, wait) = queue_flag_waiter(
        &sim,
        &words,
        control,
        TaskSlot::new(7).unwrap(),
        Domain::A,
        0x10,
        0,
        0,
    );
    sim.set_byte(task.priority(), 0);
    set_flag(&sim, id, Pattern(0x10));
    assert_eq!(sim.word(control.pattern()), 0x11);
    assert_eq!(sim.word(wait.result()), 0x11);
    assert_eq!(sim.word(control.queue().next()), control.0 .0);
    assert_eq!(sim.word(task.state()) & 0xff, 1);
    assert_eq!(sim.ready(Domain::A, 0), task.0 .0);
    assert_eq!(sim.selected(Domain::A), task.0 .0);
    assert_eq!(sim.running(Domain::A), task.0 .0);
    assert!(!sim.doorbell(0) && !sim.doorbell(1));
}

#[test]
fn set_flag_leaves_an_unsatisfied_waiter_queued() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = FlagId {
        domain: Domain::A,
        slot,
    };
    assert_eq!(
        create_flag(&sim, id, FlagCreate { attributes: FlagAttributes(0), initial: Pattern(0x01) }),
        Ok(id)
    );
    let control = words.flag_control(slot);
    let (task, wait) = queue_flag_waiter(
        &sim,
        &words,
        control,
        TaskSlot::new(7).unwrap(),
        Domain::A,
        0x40,
        0,
        0,
    );
    sim.set(wait.result(), 0xdead);
    set_flag(&sim, id, Pattern(0x10));
    assert_eq!(sim.word(control.pattern()), 0x11);
    assert_eq!(sim.word(wait.result()), 0xdead);
    assert_eq!(sim.word(control.queue().next()), task.0 .0);
    assert_eq!(sim.word(task.state()) & 0xff, 0x32);
    assert!(!sim.ready_bit(Domain::A, 5));
    assert_eq!(sim.selected(Domain::A), 0);
}

#[test]
fn set_flag_clears_the_whole_pattern_when_the_attribute_says_so() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = FlagId {
        domain: Domain::A,
        slot,
    };
    assert_eq!(
        create_flag(
            &sim,
            id,
            FlagCreate {
                attributes: FlagAttributes(FlagConfig::CLEAR_ON_MATCH),
                initial: Pattern(0x01)
            }
        ),
        Ok(id)
    );
    let control = words.flag_control(slot);
    let (task, wait) = queue_flag_waiter(
        &sim,
        &words,
        control,
        TaskSlot::new(7).unwrap(),
        Domain::A,
        0x01,
        0,
        0,
    );
    set_flag(&sim, id, Pattern(0x10));
    assert_eq!(sim.word(wait.result()), 0x11);
    assert_eq!(sim.word(control.pattern()), 0);
    assert_eq!(sim.ready(Domain::A, 5), task.0 .0);
}

#[test]
fn set_flag_rings_the_peer_for_a_remote_waiter() {
    for entry in [Domain::A, Domain::B] {
        let words = Domain::A.words();
        let slot = FlagSlot::new(3, 120).unwrap();
        let sim = Sim::new(entry);
        install(&sim, Domain::B);
        let id = FlagId {
            domain: Domain::A,
            slot,
        };
        assert_eq!(
            create_flag(&sim, id, FlagCreate { attributes: FlagAttributes(0), initial: Pattern(0x01) }),
            Ok(id)
        );
        let control = words.flag_control(slot);
        let (task, _) = queue_flag_waiter(
            &sim,
            &words,
            control,
            TaskSlot::new(7).unwrap(),
            Domain::B,
            0x10,
            0,
            0,
        );
        set_flag(&sim, id, Pattern(0x10));
        assert_eq!(sim.ready(Domain::B, 5), task.0 .0);
        assert_eq!(sim.selected(Domain::B), task.0 .0);
        match entry {
            Domain::A => {
                assert_eq!(sim.running(Domain::B), 0);
                assert!(sim.doorbell(1));
            }
            Domain::B => {
                assert_eq!(sim.running(Domain::B), task.0 .0);
                assert!(!sim.doorbell(0) && !sim.doorbell(1));
            }
        }
    }
}

#[test]
fn a_satisfied_set_cancels_the_waiters_timer() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = FlagId {
        domain: Domain::A,
        slot,
    };
    assert_eq!(
        create_flag(&sim, id, FlagCreate { attributes: FlagAttributes(0), initial: Pattern(0x01) }),
        Ok(id)
    );
    let control = words.flag_control(slot);
    let timer = GuestAddress(0x4000_0500);
    let (_, wait) = queue_flag_waiter(
        &sim,
        &words,
        control,
        TaskSlot::new(7).unwrap(),
        Domain::A,
        0x10,
        0,
        timer.0,
    );
    sim.arm(timer);
    set_flag(&sim, id, Pattern(0x10));
    assert!(!sim.armed(timer));
    assert_eq!(sim.word(wait.base().result()), 0);
}

#[test]
fn send_makes_a_receiver_ready() {
    let words = Domain::A.words();
    let slot = MailboxSlot::new(1, 1).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = MailboxId {
        domain: Domain::A,
        slot,
    };
    assert_eq!(
        create_mailbox(
            &sim,
            id,
            MailboxCreate {
                attributes: MailboxAttributes(0),
                priority_limit: PriorityLimit(32)
            }
        ),
        Ok(id)
    );
    let control = words.mailbox_control(slot);
    let (task, wait) = queue_receiver(&sim, &words, control, TaskSlot::new(7).unwrap(), Domain::A);
    let message = Message(GuestAddress(0x4000_0300));
    sim.set(message.next(), 0xdead);
    send(&sim, id, message, None);
    assert_eq!(sim.word(wait.message()), message.0 .0);
    assert_eq!(sim.word(control.queue().next()), control.0 .0);
    assert_eq!(sim.word(message.next()), 0xdead);
    assert_eq!(sim.ready(Domain::A, 5), task.0 .0);
    assert_eq!(sim.running(Domain::A), task.0 .0);
}

#[test]
fn send_to_an_empty_mailbox_appends_fifo() {
    let words = Domain::A.words();
    let slot = MailboxSlot::new(1, 1).unwrap();
    let sim = Sim::new(Domain::A);
    let id = MailboxId {
        domain: Domain::A,
        slot,
    };
    assert_eq!(
        create_mailbox(
            &sim,
            id,
            MailboxCreate {
                attributes: MailboxAttributes(0),
                priority_limit: PriorityLimit(32)
            }
        ),
        Ok(id)
    );
    let control = words.mailbox_control(slot);
    let first = Message(GuestAddress(0x4000_0300));
    let second = Message(GuestAddress(0x4000_0400));
    send(&sim, id, first, None);
    send(&sim, id, second, None);
    assert_eq!(sim.word(control.head()), first.0 .0);
    assert_eq!(sim.word(control.tail()), second.0 .0);
    assert_eq!(sim.word(first.next()), second.0 .0);
    assert_eq!(sim.word(second.next()), 0);
    assert_eq!(sim.selected(Domain::A), 0);
}

#[test]
fn send_with_priority_orders_the_queue() {
    let words = Domain::A.words();
    let slot = MailboxSlot::new(1, 1).unwrap();
    let sim = Sim::new(Domain::A);
    let id = MailboxId {
        domain: Domain::A,
        slot,
    };
    assert_eq!(
        create_mailbox(
            &sim,
            id,
            MailboxCreate {
                attributes: MailboxAttributes(2),
                priority_limit: PriorityLimit(32)
            }
        ),
        Ok(id)
    );
    let control = words.mailbox_control(slot);
    let a = Message(GuestAddress(0x4000_0300));
    let b = Message(GuestAddress(0x4000_0400));
    let c = Message(GuestAddress(0x4000_0500));
    sim.set(a.priority(), 3);
    sim.set(b.priority(), 1);
    sim.set(c.priority(), 3);
    send(&sim, id, a, Some(MessagePriority::new(3, PriorityLimit(32)).unwrap()));
    send(&sim, id, b, Some(MessagePriority::new(1, PriorityLimit(32)).unwrap()));
    send(&sim, id, c, Some(MessagePriority::new(3, PriorityLimit(32)).unwrap()));
    assert_eq!(sim.word(control.head()), b.0 .0);
    assert_eq!(sim.word(b.next()), a.0 .0);
    assert_eq!(sim.word(a.next()), c.0 .0);
    assert_eq!(sim.word(c.next()), 0);
}

#[test]
fn send_rings_the_peer_for_a_remote_receiver() {
    for entry in [Domain::A, Domain::B] {
        let words = Domain::A.words();
        let slot = MailboxSlot::new(1, 1).unwrap();
        let sim = Sim::new(entry);
        install(&sim, Domain::B);
        let id = MailboxId {
            domain: Domain::A,
            slot,
        };
        assert_eq!(
            create_mailbox(
                &sim,
                id,
                MailboxCreate {
                    attributes: MailboxAttributes(0),
                    priority_limit: PriorityLimit(32)
                }
            ),
            Ok(id)
        );
        let control = words.mailbox_control(slot);
        let (task, wait) =
            queue_receiver(&sim, &words, control, TaskSlot::new(7).unwrap(), Domain::B);
        let message = Message(GuestAddress(0x4000_0300));
        send(&sim, id, message, None);
        assert_eq!(sim.word(wait.message()), message.0 .0);
        assert_eq!(sim.ready(Domain::B, 5), task.0 .0);
        match entry {
            Domain::A => {
                assert_eq!(sim.running(Domain::B), 0);
                assert!(sim.doorbell(1));
            }
            Domain::B => {
                assert_eq!(sim.running(Domain::B), task.0 .0);
                assert!(!sim.doorbell(0) && !sim.doorbell(1));
            }
        }
    }
}

fn running_task(sim: &Sim, words: &DomainWords, domain: Domain, slot: u32) -> (Task, TaskSlot) {
    let task_slot = TaskSlot::new(slot).unwrap();
    let task = words.task(task_slot);
    sim.set(task.domain(), if domain == Domain::A { 1 } else { 2 });
    sim.set_byte(task.state(), TaskState::READY.bits());
    sim.set_running(domain, task);
    (task, task_slot)
}

fn create_flag_at(sim: &Sim, slot: FlagSlot, attributes: u32, initial: u32) -> FlagId {
    let id = FlagId { domain: Domain::A, slot };
    assert_eq!(
        create_flag(
            sim,
            id,
            FlagCreate { attributes: FlagAttributes(attributes), initial: Pattern(initial) }
        ),
        Ok(id)
    );
    id
}

fn create_mailbox_at(sim: &Sim, slot: MailboxSlot, attributes: u32) -> MailboxId {
    let id = MailboxId { domain: Domain::A, slot };
    assert_eq!(
        create_mailbox(
            sim,
            id,
            MailboxCreate { attributes: MailboxAttributes(attributes), priority_limit: PriorityLimit(32) }
        ),
        Ok(id)
    );
    id
}

fn pending_flag(result: Result<FlagBlocking, ObjectStatus>) -> FlagWaitPending {
    match result {
        Ok(FlagBlocking::Pending(pending)) => pending,
        other => panic!("expected a pending flag wait, got {other:?}"),
    }
}

fn pending_receive(result: Result<ReceiveBlocking, ObjectStatus>) -> ReceivePending {
    match result {
        Ok(ReceiveBlocking::Pending(pending)) => pending,
        other => panic!("expected a pending receive, got {other:?}"),
    }
}

#[test]
fn wait_flag_without_a_match_arms_the_waiter() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = create_flag_at(&sim, slot, 0, 0);
    let (task, task_slot) = running_task(&sim, &words, Domain::A, 7);
    let out = GuestAddress(0x4000_0600);
    pending_flag(wait_flag(
        &sim,
        EntryDomain::new(Domain::A),
        id,
        WantedMask::new(Pattern(0x10)).unwrap(),
        MatchMode::All,
        OutWord(out),
        Timeout::Forever,
    ));
    let wait = words.flag_wait(task_slot);
    let control = words.flag_control(slot);
    assert_eq!(sim.word(wait.pattern()), 0x10);
    assert_eq!(sim.word(wait.mode()), 0);
    assert_eq!(sim.word(wait.base().result()), 0);
    assert_eq!(sim.word(wait.base().queue()), control.0 .0);
    assert_eq!(sim.word(wait.base().owner()), words.context(Domain::A).0 .0);
    assert_eq!(sim.word(task.state()) & 0xff, 0x32);
    assert_eq!(sim.word(task.wait()), wait.0 .0);
    assert_eq!(sim.word(control.queue().next()), task.0 .0);
    assert_eq!(sim.word(out), 0);
}

#[test]
fn timed_wait_arms_the_per_task_timer_entry() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = create_flag_at(&sim, slot, 0, 0);
    let (task, task_slot) = running_task(&sim, &words, Domain::A, 7);
    sim.set(words.context(Domain::A).tick_now(), words.tick_now.0);
    sim.set(words.tick_now, 100);
    pending_flag(wait_flag(
        &sim,
        EntryDomain::new(Domain::A),
        id,
        WantedMask::new(Pattern(0x10)).unwrap(),
        MatchMode::All,
        OutWord(GuestAddress(0x4000_0600)),
        Timeout::Units(5),
    ));
    let wait = words.flag_wait(task_slot);
    let entry = words.flag_timer(task_slot);
    assert_eq!(sim.word(wait.base().result()), entry.0);
    assert_eq!(sim.word(entry.offset(0x04)), sim.timeout_callback().0);
    assert_eq!(sim.word(entry.offset(0x08)), task.0 .0);
    assert!(sim.armed(entry));
    assert_eq!(sim.deadline(entry), Some(105));
}

#[test]
fn wait_flag_with_a_match_reports_the_pattern() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = create_flag_at(&sim, slot, 0, 0x11);
    let (task, _) = running_task(&sim, &words, Domain::A, 7);
    let out = GuestAddress(0x4000_0600);
    assert_eq!(
        wait_flag(
            &sim,
            EntryDomain::new(Domain::A),
            id,
            WantedMask::new(Pattern(0x10)).unwrap(),
            MatchMode::All,
            OutWord(out),
            Timeout::Forever,
        ),
        Ok(FlagBlocking::Ready(WaitOutcome::Satisfied(Pattern(0x11))))
    );
    assert_eq!(sim.word(out), 0x11);
    assert_eq!(sim.word(task.wait()), 0);
}

#[test]
fn wait_flag_refuses_a_second_waiter() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = create_flag_at(&sim, slot, 0, 0);
    queue_flag_waiter(
        &sim,
        &words,
        words.flag_control(slot),
        TaskSlot::new(7).unwrap(),
        Domain::A,
        0x10,
        0,
        0,
    );
    running_task(&sim, &words, Domain::A, 8);
    assert_eq!(
        wait_flag(
            &sim,
            EntryDomain::new(Domain::A),
            id,
            WantedMask::new(Pattern(0x10)).unwrap(),
            MatchMode::All,
            OutWord(GuestAddress(0x4000_0600)),
            Timeout::Forever,
        ),
        Err(ObjectStatus::QueueOverflow)
    );
}

#[test]
fn receive_pops_the_head_before_the_output_write() {
    let words = Domain::A.words();
    let slot = MailboxSlot::new(1, 1).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = create_mailbox_at(&sim, slot, 0);
    let message = Message(GuestAddress(0x4000_0300));
    send(&sim, id, message, None);
    let out = GuestAddress(0x4000_0600);
    assert_eq!(
        receive(&sim, EntryDomain::new(Domain::A), id, OutWord(out), Timeout::Forever),
        Ok(ReceiveBlocking::Ready(WaitOutcome::Satisfied(message)))
    );
    assert_eq!(sim.word(out), message.0 .0);
    assert_eq!(sim.word(words.mailbox_control(slot).head()), 0);
}

#[test]
fn a_blocking_receive_output_aliasing_the_next_link_still_pops() {
    let words = Domain::A.words();
    let slot = MailboxSlot::new(1, 1).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = create_mailbox_at(&sim, slot, 0);
    let message = Message(GuestAddress(0x4000_0300));
    send(&sim, id, message, None);
    assert_eq!(
        receive(&sim, EntryDomain::new(Domain::A), id, OutWord(message.next()), Timeout::Forever),
        Ok(ReceiveBlocking::Ready(WaitOutcome::Satisfied(message)))
    );
    assert_eq!(sim.word(words.mailbox_control(slot).head()), 0);
}

#[test]
fn receive_on_an_empty_mailbox_arms_the_receiver() {
    let words = Domain::A.words();
    let slot = MailboxSlot::new(1, 1).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = create_mailbox_at(&sim, slot, 0);
    let (task, task_slot) = running_task(&sim, &words, Domain::A, 7);
    let out = GuestAddress(0x4000_0600);
    pending_receive(receive(
        &sim,
        EntryDomain::new(Domain::A),
        id,
        OutWord(out),
        Timeout::Forever,
    ));
    let wait = words.mailbox_wait(task_slot);
    let control = words.mailbox_control(slot);
    assert_eq!(sim.word(task.state()) & 0xff, 0x32);
    assert_eq!(sim.word(task.wait()), wait.0 .0);
    assert_eq!(sim.word(wait.base().result()), 0);
    assert_eq!(sim.word(wait.base().queue()), control.0 .0);
    assert_eq!(sim.word(wait.base().owner()), words.context(Domain::A).0 .0);
    assert_eq!(sim.word(control.queue().next()), task.0 .0);
    assert_eq!(sim.word(out), 0);
}

#[test]
fn a_premature_resume_keeps_the_continuation() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = create_flag_at(&sim, slot, 0, 0x01);
    let (task, _) = running_task(&sim, &words, Domain::A, 7);
    let out = GuestAddress(0x4000_0600);
    sim.set(out, 0xdead_beef);
    let armed = pending_flag(wait_flag(
        &sim,
        EntryDomain::new(Domain::A),
        id,
        WantedMask::new(Pattern(0x10)).unwrap(),
        MatchMode::All,
        OutWord(out),
        Timeout::Forever,
    ));
    let armed = match resume_flag(&sim, armed) {
        FlagResume::Pending(pending) => pending,
        other => panic!("expected the pending continuation, got {other:?}"),
    };
    assert_eq!(sim.word(out), 0xdead_beef);
    assert_eq!(sim.word(task.state()) & 0xff, 0x32);
    set_flag(&sim, id, Pattern(0x10));
    assert_eq!(
        resume_flag(&sim, armed),
        FlagResume::Completed(WaitOutcome::Satisfied(Pattern(0x11)))
    );
    assert_eq!(sim.word(out), 0x11);
}

#[test]
fn resume_after_timeout_reports_timed_out_and_leaves_the_output() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = create_flag_at(&sim, slot, 0, 0);
    let (task, task_slot) = running_task(&sim, &words, Domain::A, 7);
    let out = GuestAddress(0x4000_0600);
    sim.set(out, 0xdead_beef);
    let armed = pending_flag(wait_flag(
        &sim,
        EntryDomain::new(Domain::A),
        id,
        WantedMask::new(Pattern(0x10)).unwrap(),
        MatchMode::All,
        OutWord(out),
        Timeout::Units(5),
    ));
    let armed = match resume_flag(&sim, armed) {
        FlagResume::Pending(pending) => pending,
        other => panic!("expected the pending continuation, got {other:?}"),
    };
    let wait = words.flag_wait(task_slot);
    sim.set(wait.base().result(), 0xffff_ffce);
    sim.set_byte(task.state(), 0x01);
    assert_eq!(resume_flag(&sim, armed), FlagResume::Completed(WaitOutcome::TimedOut));
    assert_eq!(sim.word(out), 0xdead_beef);
}

#[test]
fn resume_after_forced_release_reports_released_and_leaves_the_output() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = create_flag_at(&sim, slot, 0, 0);
    let (task, task_slot) = running_task(&sim, &words, Domain::A, 7);
    let out = GuestAddress(0x4000_0600);
    sim.set(out, 0xdead_beef);
    let armed = pending_flag(wait_flag(
        &sim,
        EntryDomain::new(Domain::A),
        id,
        WantedMask::new(Pattern(0x10)).unwrap(),
        MatchMode::All,
        OutWord(out),
        Timeout::Forever,
    ));
    let armed = match resume_flag(&sim, armed) {
        FlagResume::Pending(pending) => pending,
        other => panic!("expected the pending continuation, got {other:?}"),
    };
    let wait = words.flag_wait(task_slot);
    sim.set(wait.base().result(), 0xffff_ffcf);
    sim.set_byte(task.state(), 0x01);
    assert_eq!(resume_flag(&sim, armed), FlagResume::Completed(WaitOutcome::Released));
    assert_eq!(sim.word(out), 0xdead_beef);
}

#[test]
fn resume_receive_after_wake_writes_the_message() {
    let words = Domain::A.words();
    let slot = MailboxSlot::new(1, 1).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = create_mailbox_at(&sim, slot, 0);
    running_task(&sim, &words, Domain::A, 7);
    let out = GuestAddress(0x4000_0600);
    sim.set(out, 0xdead_beef);
    let armed = pending_receive(receive(
        &sim,
        EntryDomain::new(Domain::A),
        id,
        OutWord(out),
        Timeout::Forever,
    ));
    let armed = match resume_receive(&sim, armed) {
        ReceiveResume::Pending(pending) => pending,
        other => panic!("expected the pending continuation, got {other:?}"),
    };
    assert_eq!(sim.word(out), 0xdead_beef);
    let message = Message(GuestAddress(0x4000_0300));
    send(&sim, id, message, None);
    assert_eq!(
        resume_receive(&sim, armed),
        ReceiveResume::Completed(WaitOutcome::Satisfied(message))
    );
    assert_eq!(sim.word(out), message.0 .0);
}

#[test]
fn per_task_timer_entries_are_distinct() {
    let words = Domain::A.words();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let first = create_flag_at(&sim, FlagSlot::new(1, 120).unwrap(), 0, 0);
    let second = create_flag_at(&sim, FlagSlot::new(2, 120).unwrap(), 0, 0);
    sim.set(words.context(Domain::A).tick_now(), words.tick_now.0);
    sim.set(words.tick_now, 100);
    running_task(&sim, &words, Domain::A, 7);
    pending_flag(wait_flag(
        &sim,
        EntryDomain::new(Domain::A),
        first,
        WantedMask::new(Pattern(0x10)).unwrap(),
        MatchMode::All,
        OutWord(GuestAddress(0x4000_0600)),
        Timeout::Units(5),
    ));
    running_task(&sim, &words, Domain::A, 8);
    pending_flag(wait_flag(
        &sim,
        EntryDomain::new(Domain::A),
        second,
        WantedMask::new(Pattern(0x10)).unwrap(),
        MatchMode::All,
        OutWord(GuestAddress(0x4000_0610)),
        Timeout::Units(9),
    ));
    let entry_first = words.flag_timer(TaskSlot::new(7).unwrap());
    let entry_second = words.flag_timer(TaskSlot::new(8).unwrap());
    assert_ne!(entry_first, entry_second);
    assert_eq!(
        sim.word(words.flag_wait(TaskSlot::new(7).unwrap()).base().result()),
        entry_first.0
    );
    assert_eq!(
        sim.word(words.flag_wait(TaskSlot::new(8).unwrap()).base().result()),
        entry_second.0
    );
    assert!(sim.armed(entry_first) && sim.armed(entry_second));
    assert_eq!(sim.deadline(entry_first), Some(105));
    assert_eq!(sim.deadline(entry_second), Some(109));
}

#[test]
fn receive_queue_orders_receivers_by_priority() {
    let words = Domain::A.words();
    let slot = MailboxSlot::new(1, 1).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = create_mailbox_at(&sim, slot, 1);
    let (first, _) = running_task(&sim, &words, Domain::A, 7);
    sim.set_byte(first.priority(), 5);
    pending_receive(receive(&sim, EntryDomain::new(Domain::A), id, OutWord(GuestAddress(0x4000_0600)), Timeout::Forever));
    let (second, _) = running_task(&sim, &words, Domain::A, 8);
    sim.set_byte(second.priority(), 3);
    pending_receive(receive(&sim, EntryDomain::new(Domain::A), id, OutWord(GuestAddress(0x4000_0610)), Timeout::Forever));
    let control = words.mailbox_control(slot);
    assert_eq!(sim.word(control.queue().next()), second.0 .0);
    assert_eq!(sim.word(second.next()), first.0 .0);
    assert_eq!(sim.word(first.next()), control.0 .0);
}

#[test]
fn receive_queue_appends_receivers_in_fifo() {
    let words = Domain::A.words();
    let slot = MailboxSlot::new(1, 1).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = create_mailbox_at(&sim, slot, 0);
    let (first, _) = running_task(&sim, &words, Domain::A, 7);
    pending_receive(receive(&sim, EntryDomain::new(Domain::A), id, OutWord(GuestAddress(0x4000_0600)), Timeout::Forever));
    let (second, _) = running_task(&sim, &words, Domain::A, 8);
    pending_receive(receive(&sim, EntryDomain::new(Domain::A), id, OutWord(GuestAddress(0x4000_0610)), Timeout::Forever));
    let control = words.mailbox_control(slot);
    assert_eq!(sim.word(control.queue().next()), first.0 .0);
    assert_eq!(sim.word(first.next()), second.0 .0);
    assert_eq!(sim.word(second.next()), control.0 .0);
}

#[test]
fn handler_set_releases_through_the_waiters_domain_and_defers_dispatch() {
    for entry in [Domain::A, Domain::B] {
        let words = Domain::A.words();
        let slot = FlagSlot::new(3, 120).unwrap();
        let sim = Sim::new(entry);
        install(&sim, Domain::B);
        let id = FlagId { domain: Domain::A, slot };
        assert_eq!(
            create_flag(&sim, id, FlagCreate { attributes: FlagAttributes(0), initial: Pattern(0x01) }),
            Ok(id)
        );
        let (task, wait) = queue_flag_waiter(
            &sim,
            &words,
            words.flag_control(slot),
            TaskSlot::new(7).unwrap(),
            Domain::B,
            0x10,
            0,
            0,
        );
        set_flag_handler(&sim, id, Pattern(0x10));
        assert_eq!(sim.ready(Domain::B, 5), task.0 .0);
        assert_eq!(sim.word(wait.result()), 0x11);
        assert_eq!(sim.word(wait.base().result()), 0);
        assert_eq!(sim.running(Domain::B), 0);
        assert_eq!(sim.ready(Domain::A, 5), 0);
        match entry {
            Domain::A => {
                assert!(sim.doorbell(1));
                assert_eq!(sim.delayed(), 0);
            }
            Domain::B => {
                assert!(!sim.doorbell(0) && !sim.doorbell(1));
                assert_eq!(sim.delayed(), 1);
            }
        }
    }
}

#[test]
fn the_resume_output_write_masks_only_i() {
    for initial in [0u32, 0x40] {
        let words = Domain::A.words();
        let slot = FlagSlot::new(3, 120).unwrap();
        let sim = Sim::new(Domain::A);
        install(&sim, Domain::A);
        sim.set_interrupts(initial);
        let id = create_flag_at(&sim, slot, 0, 0x01);
        running_task(&sim, &words, Domain::A, 7);
        let out = GuestAddress(0x4000_0600);
        sim.set(out, 0xdead_beef);
        let armed = pending_flag(wait_flag(
            &sim,
            EntryDomain::new(Domain::A),
            id,
            WantedMask::new(Pattern(0x10)).unwrap(),
            MatchMode::All,
            OutWord(out),
            Timeout::Forever,
        ));
        let armed = match resume_flag(&sim, armed) {
            FlagResume::Pending(pending) => pending,
            other => panic!("expected the pending continuation, got {other:?}"),
        };
        set_flag(&sim, id, Pattern(0x10));
        assert_eq!(
            resume_flag(&sim, armed),
            FlagResume::Completed(WaitOutcome::Satisfied(Pattern(0x11)))
        );
        assert_eq!(sim.word(out), 0x11);
        assert_eq!(sim.last_store_mask(), initial | 0x80);
        assert_eq!(sim.interrupts(), initial);
    }
}

#[test]
fn delete_flag_releases_a_queued_waiter_with_minus_49() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = create_flag_at(&sim, slot, 0, 0x01);
    let (task, wait) = queue_flag_waiter(
        &sim,
        &words,
        words.flag_control(slot),
        TaskSlot::new(7).unwrap(),
        Domain::A,
        0x10,
        0,
        0,
    );
    assert_eq!(delete_flag(&sim, id), Ok(()));
    assert_eq!(sim.word(wait.base().result()), 0xffff_ffcf);
    assert_eq!(sim.word(task.state()) & 0xff, 0x01);
    assert_eq!(sim.ready(Domain::A, 5), task.0 .0);
    assert_eq!(sim.running(Domain::A), task.0 .0);
    let control = words.flag_control(slot);
    assert_eq!(sim.word(control.queue().next()), control.0 .0);
    assert_eq!(sim.word(control.queue().prev()), control.0 .0);
    assert_eq!(sim.word(control.pattern()), 0);
    assert_eq!(sim.word(words.flag_config(slot).exists()), 0);
    assert_eq!(create_flag_at(&sim, slot, 0, 0), id);
}

#[test]
fn delete_flag_cancels_the_timed_waiters_timer() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = create_flag_at(&sim, slot, 0, 0);
    let timer = GuestAddress(0x4000_0500);
    let (_, wait) = queue_flag_waiter(
        &sim,
        &words,
        words.flag_control(slot),
        TaskSlot::new(7).unwrap(),
        Domain::A,
        0x10,
        0,
        timer.0,
    );
    sim.arm(timer);
    assert_eq!(delete_flag(&sim, id), Ok(()));
    assert!(!sim.armed(timer));
    assert_eq!(sim.word(wait.base().result()), 0xffff_ffcf);
}

#[test]
fn delete_mailbox_drains_queued_messages_in_delivery_order() {
    let words = Domain::A.words();
    let slot = MailboxSlot::new(1, 1).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = create_mailbox_at(&sim, slot, 0);
    let first = Message(GuestAddress(0x4000_0300));
    let second = Message(GuestAddress(0x4000_0310));
    send(&sim, id, first, None);
    send(&sim, id, second, None);
    let drained: Vec<Message> = delete_mailbox(&sim, id).unwrap().collect();
    assert_eq!(drained, vec![first, second]);
    let control = words.mailbox_control(slot);
    assert_eq!(sim.word(control.head()), 0);
    assert_eq!(sim.word(control.tail()), 0);
    assert_eq!(sim.word(control.queue().next()), control.0 .0);
    let config = words.mailbox_config(slot);
    assert_eq!(sim.word(config.attributes()), 2);
    assert_eq!(sim.word(config.priority_limit()), 32);
    assert_eq!(sim.word(config.exists()), 0);
    assert_eq!(create_mailbox_at(&sim, slot, 0), id);
}

#[test]
fn delete_mailbox_releases_a_queued_receiver() {
    let words = Domain::A.words();
    let slot = MailboxSlot::new(1, 1).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    let id = create_mailbox_at(&sim, slot, 0);
    let (task, task_slot) = running_task(&sim, &words, Domain::A, 7);
    sim.set_byte(task.priority(), 5);
    let receiver = pending_receive(receive(
        &sim,
        EntryDomain::new(Domain::A),
        id,
        OutWord(GuestAddress(0x4000_0600)),
        Timeout::Forever,
    ));
    let drained: Vec<Message> = delete_mailbox(&sim, id).unwrap().collect();
    assert!(drained.is_empty());
    let wait = words.mailbox_wait(task_slot);
    assert_eq!(sim.word(wait.base().result()), 0xffff_ffcf);
    assert_eq!(sim.word(task.state()) & 0xff, 0x01);
    assert_eq!(sim.ready(Domain::A, 5), task.0 .0);
    assert_eq!(
        resume_receive(&sim, receiver),
        ReceiveResume::Completed(WaitOutcome::Released)
    );
}

#[test]
fn delete_releases_a_cross_domain_waiter_in_its_domain() {
    let words = Domain::A.words();
    let slot = FlagSlot::new(3, 120).unwrap();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::B);
    let id = FlagId { domain: Domain::A, slot };
    assert_eq!(
        create_flag(&sim, id, FlagCreate { attributes: FlagAttributes(0), initial: Pattern(0x01) }),
        Ok(id)
    );
    let (task, _) = queue_flag_waiter(
        &sim,
        &words,
        words.flag_control(slot),
        TaskSlot::new(7).unwrap(),
        Domain::B,
        0x10,
        0,
        0,
    );
    assert_eq!(delete_flag(&sim, id), Ok(()));
    assert_eq!(sim.ready(Domain::B, 5), task.0 .0);
    assert_eq!(sim.ready(Domain::A, 5), 0);
    assert!(sim.doorbell(1));
}

#[test]
fn delete_mailbox_releases_two_receivers_in_their_domains() {
    let a = Domain::A.words();
    let b = Domain::B.words();
    let sim = Sim::new(Domain::A);
    install(&sim, Domain::A);
    install(&sim, Domain::B);
    let mailbox = create_mailbox_at(&sim, MailboxSlot::new(1, 1).unwrap(), 0);
    let (task_a, slot_a) = running_task(&sim, &a, Domain::A, 7);
    sim.set_byte(task_a.priority(), 5);
    let out_a = GuestAddress(0x4000_0600);
    sim.set(out_a, 0xdead_beef);
    let receiver_a = pending_receive(receive(
        &sim,
        EntryDomain::new(Domain::A),
        mailbox,
        OutWord(out_a),
        Timeout::Forever,
    ));
    let (task_b, slot_b) = running_task(&sim, &b, Domain::B, 8);
    sim.set_byte(task_b.priority(), 5);
    sim.set(a.context(Domain::B).tick_now(), b.tick_now.0);
    sim.set(b.tick_now, 100);
    let out_b = GuestAddress(0x4000_0610);
    sim.set(out_b, 0xdead_beef);
    let receiver_b = pending_receive(receive(
        &sim,
        EntryDomain::new(Domain::B),
        mailbox,
        OutWord(out_b),
        Timeout::Units(5),
    ));
    let timer_b = b.mailbox_timer(slot_b);
    assert!(sim.armed(timer_b));
    let drained: Vec<Message> = delete_mailbox(&sim, mailbox).unwrap().collect();
    assert!(drained.is_empty());
    assert_eq!(sim.word(a.mailbox_wait(slot_a).base().result()), 0xffff_ffcf);
    assert_eq!(sim.word(b.mailbox_wait(slot_b).base().result()), 0xffff_ffcf);
    assert_eq!(
        resume_receive(&sim, receiver_a),
        ReceiveResume::Completed(WaitOutcome::Released)
    );
    assert_eq!(
        resume_receive(&sim, receiver_b),
        ReceiveResume::Completed(WaitOutcome::Released)
    );
    assert_eq!(sim.word(out_a), 0xdead_beef);
    assert_eq!(sim.word(out_b), 0xdead_beef);
    assert!(!sim.armed(timer_b));
    assert_eq!(sim.ready(Domain::A, 5), task_a.0 .0);
    assert_eq!(sim.ready(Domain::B, 5), task_b.0 .0);
    assert_eq!(sim.running(Domain::A), task_a.0 .0);
    assert!(sim.doorbell(1));
    let control = a.mailbox_control(mailbox.slot);
    assert_eq!(sim.word(control.queue().next()), control.0 .0);
    assert_eq!(sim.word(control.head()), 0);
    assert_eq!(sim.word(control.tail()), 0);
    assert_eq!(sim.word(a.mailbox_config(mailbox.slot).exists()), 0);
}

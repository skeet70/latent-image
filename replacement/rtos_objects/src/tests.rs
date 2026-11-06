use crate::layout::{FlagConfig, GuestAddress, LockRecord, Message};
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

fn req(service: Service) -> Request {
    Request {
        service,
        entry: entry(),
        entry_domain: EntryDomain::new(Domain::A),
        id: EncodedId::of(1, 1),
        records: Records {
            flags: 120,
            mailboxes: 1,
        },
        facts: Facts {
            exists: true,
            attributes: 0,
            priority_limit: 32,
        },
        params: Params::NONE,
    }
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
            admit(&request).map(|admitted| admitted.object),
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
        admit(&request).map(|admitted| admitted.object),
        Ok(Object::Flag(flag1(Domain::A)))
    );
}

#[test]
fn task_forms_refuse_handler_context() {
    let mut request = req(Service::FlagSet);
    request.entry.handler_context = 1;
    assert_eq!(admit(&request), Err(ObjectStatus::Context));
}

#[test]
fn the_handler_form_refuses_task_context() {
    assert_eq!(admit(&req(Service::FlagSetHandler)), Err(ObjectStatus::Context));
    let mut handler = req(Service::FlagSetHandler);
    handler.entry.handler_context = 1;
    assert!(admit(&handler).is_ok());
}

#[test]
fn masked_irq_is_refused() {
    let mut request = req(Service::FlagClear);
    request.entry.irq_masked = true;
    assert_eq!(admit(&request), Err(ObjectStatus::Context));
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
        assert_eq!(admit(&request), Err(ObjectStatus::Context), "{service:?}");
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
        assert!(admit(&request).is_ok(), "{service:?}");
    }
}

#[test]
fn send_checks_existence_before_the_priority() {
    let mut request = req(Service::MailboxSend);
    request.facts = Facts {
        exists: false,
        attributes: 2,
        priority_limit: 32,
    };
    request.params.message_priority = Some(0);
    assert_eq!(admit(&request), Err(ObjectStatus::NotCreated));
}

#[test]
fn send_refuses_a_priority_outside_the_limit() {
    for priority in [0, 33, 0xffff_ffff] {
        let mut request = req(Service::MailboxSend);
        request.facts.attributes = 2;
        request.params.message_priority = Some(priority);
        assert_eq!(admit(&request), Err(ObjectStatus::Param), "{priority}");
    }
    let mut accepted = req(Service::MailboxSend);
    accepted.facts.attributes = 2;
    accepted.params.message_priority = Some(32);
    assert_eq!(
        admit(&accepted).map(|admitted| admitted.priority),
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
    let mut request = req(Service::MailboxSend);
    request.facts = Facts {
        exists: true,
        attributes: 2,
        priority_limit: 0xffff_ffff,
    };
    request.params.message_priority = Some(0xffff_ffff);
    assert_eq!(admit(&request), Err(ObjectStatus::Param));
}

#[test]
fn send_ignores_the_priority_without_the_attribute() {
    let mut request = req(Service::MailboxSend);
    request.params.message_priority = Some(0);
    assert_eq!(admit(&request).map(|admitted| admitted.priority), Ok(None));
}

#[test]
fn timed_receive_checks_the_timeout_before_existence() {
    let mut request = req(Service::MailboxTimedReceive);
    request.facts.exists = false;
    request.params.timeout = Some(-2);
    assert_eq!(admit(&request), Err(ObjectStatus::Param));
}

#[test]
fn flag_wait_refuses_a_zero_pattern_before_existence() {
    let mut request = req(Service::FlagWait);
    request.facts.exists = false;
    assert_eq!(admit(&request), Err(ObjectStatus::Param));
}

#[test]
fn flag_timed_wait_checks_the_mode_before_existence() {
    let mut request = req(Service::FlagTimedWait);
    request.facts.exists = false;
    request.params.pattern = Pattern(1);
    request.params.mode = 2;
    request.params.timeout = Some(-1);
    assert_eq!(admit(&request), Err(ObjectStatus::Param));
}

#[test]
fn flag_set_accepts_zero_bits() {
    let mut request = req(Service::FlagSet);
    request.params.pattern = Pattern(0);
    assert!(admit(&request).is_ok());
}

#[test]
fn the_index_is_checked_before_the_creation_record() {
    let mut request = req(Service::FlagSet);
    request.id = EncodedId::of(1, 121);
    request.facts.exists = false;
    assert_eq!(admit(&request), Err(ObjectStatus::InvalidIndex));
}

#[test]
fn admit_returns_the_validated_parameters() {
    let mut request = req(Service::FlagTimedWait);
    request.params.pattern = Pattern(0x10);
    request.params.mode = 1;
    request.params.timeout = Some(5);
    let admitted = admit(&request).unwrap();
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
}

impl Machine for Mock {
    fn load_word(&self, at: GuestAddress) -> u32 {
        self.word(at)
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
    let admitted = admit(&request).unwrap();
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

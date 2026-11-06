use crate::*;

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
        Ok(Some(MessagePriority::new(32, 32).unwrap()))
    );
}

#[test]
fn a_negative_priority_is_refused_whatever_the_limit() {
    assert_eq!(MessagePriority::new(0xffff_ffff, 0xffff_ffff), Err(ObjectStatus::Param));
    assert_eq!(MessagePriority::new(0x8000_0000, 0xffff_ffff), Err(ObjectStatus::Param));
    assert_eq!(MessagePriority::new(0, 0xffff_ffff), Err(ObjectStatus::Param));
    assert_eq!(
        MessagePriority::new(1, 0xffff_ffff),
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
            ObjectStatus::NotCreated,
            ObjectStatus::Param,
            ObjectStatus::QueueOverflow,
        ]
        .map(ObjectStatus::code),
        [-25, -18, -18, -42, -17, -28]
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

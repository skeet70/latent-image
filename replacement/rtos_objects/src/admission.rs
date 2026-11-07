use crate::id::{
    own_flag_slot, own_mailbox_slot, resolve_flag_with, resolve_mailbox_with, EncodedId, EntryDomain,
    FlagId, MailboxId,
};
use crate::layout::{Domain, MailboxConfig, Message};
use crate::matching::{MatchMode, Pattern};
use crate::message::{MessagePriority, PriorityLimit};
use crate::status::{ObjectStatus, WaitOutcome};
use crate::timeout::Timeout;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Service {
    FlagCreate,
    FlagDelete,
    FlagSet,
    FlagSetHandler,
    FlagClear,
    FlagWait,
    FlagPoll,
    FlagTimedWait,
    MailboxCreate,
    MailboxDelete,
    MailboxSend,
    MailboxReceive,
    MailboxPoll,
    MailboxTimedReceive,
}

impl Service {
    fn in_handler(self) -> bool {
        self == Self::FlagSetHandler
    }

    pub const fn blocks(self) -> bool {
        matches!(
            self,
            Self::FlagWait | Self::FlagTimedWait | Self::MailboxReceive | Self::MailboxTimedReceive
        )
    }

    fn owns_its_table(self) -> bool {
        matches!(
            self,
            Self::FlagCreate | Self::FlagDelete | Self::MailboxCreate | Self::MailboxDelete
        )
    }
}

/// The words a body reads before its first lock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    pub handler_context: u32,
    pub irq_masked: bool,
    pub dispatch_enabled: u32,
}

/// The selected domain record's published table counts, read when the class is resolved.
pub trait Records {
    fn flags(&self, domain: Domain) -> u32;
    fn mailboxes(&self, domain: Domain) -> u32;
}

/// The creation record and message words a body reads at its stock points.
pub trait Facts {
    fn exists(&self, object: Object) -> bool;
    fn attributes(&self, object: Object) -> u32;
    fn priority_limit(&self, object: Object) -> u32;
    fn message_priority(&self, message: Message) -> u32;
}

/// The raw parameters a body reads before its first lock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Params {
    pub pattern: Pattern,
    pub mode: u32,
    pub timeout: Option<i32>,
    pub message: Option<Message>,
}

impl Params {
    pub const NONE: Self = Self {
        pattern: Pattern(0),
        mode: 0,
        timeout: None,
        message: None,
    };
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Object {
    Flag(FlagId),
    Mailbox(MailboxId),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Admitted {
    pub object: Object,
    pub want: Pattern,
    pub mode: MatchMode,
    pub timeout: Option<Timeout>,
    pub priority: Option<MessagePriority>,
}

pub struct Request {
    pub service: Service,
    pub entry: Entry,
    pub entry_domain: EntryDomain,
    pub id: EncodedId,
    pub params: Params,
}

/// The stock order: context, IRQ mask, dispatch word, class, index, family parameters, then the
/// creation record. Create and delete have no admission at all.
pub fn admit<R: Records, F: Facts>(
    request: &Request,
    records: &R,
    facts: &F,
) -> Result<Admitted, ObjectStatus> {
    if request.service.owns_its_table() {
        return own_table(request, records);
    }
    if (request.entry.handler_context != 0) != request.service.in_handler() {
        return Err(ObjectStatus::Context);
    }
    if request.entry.irq_masked {
        return Err(ObjectStatus::Context);
    }
    if request.service.blocks() && request.entry.dispatch_enabled == 0 {
        return Err(ObjectStatus::Context);
    }
    match request.service {
        Service::FlagSet | Service::FlagSetHandler | Service::FlagClear => {
            let id = resolve_flag_with(request.entry_domain, request.id, |domain| records.flags(domain))?;
            if !facts.exists(Object::Flag(id)) {
                return Err(ObjectStatus::NotCreated);
            }
            Ok(flag(id, Pattern(0), MatchMode::All, None))
        }
        Service::FlagWait | Service::FlagPoll | Service::FlagTimedWait => {
            let id = resolve_flag_with(request.entry_domain, request.id, |domain| records.flags(domain))?;
            if request.params.pattern.0 == 0 {
                return Err(ObjectStatus::Param);
            }
            let mode = MatchMode::from_word(request.params.mode)?;
            let timeout = match request.service {
                Service::FlagTimedWait => Some(Timeout::from_stock(
                    request.params.timeout.ok_or(ObjectStatus::Param)?,
                )?),
                _ => None,
            };
            if !facts.exists(Object::Flag(id)) {
                return Err(ObjectStatus::NotCreated);
            }
            Ok(flag(id, request.params.pattern, mode, timeout))
        }
        Service::MailboxSend => {
            let id = resolve_mailbox_with(request.entry_domain, request.id, |domain| {
                records.mailboxes(domain)
            })?;
            let object = Object::Mailbox(id);
            if !facts.exists(object) {
                return Err(ObjectStatus::NotCreated);
            }
            let priority = if facts.attributes(object) & MailboxConfig::MESSAGE_PRIORITY != 0 {
                let message = request.params.message.ok_or(ObjectStatus::Param)?;
                let raw = facts.message_priority(message);
                let limit = PriorityLimit::from_word(facts.priority_limit(object));
                Some(MessagePriority::new(raw, limit)?)
            } else {
                None
            };
            Ok(Admitted {
                object,
                want: Pattern(0),
                mode: MatchMode::All,
                timeout: None,
                priority,
            })
        }
        Service::MailboxReceive | Service::MailboxPoll | Service::MailboxTimedReceive => {
            let id = resolve_mailbox_with(request.entry_domain, request.id, |domain| {
                records.mailboxes(domain)
            })?;
            let timeout = match request.service {
                Service::MailboxTimedReceive => Some(Timeout::from_stock(
                    request.params.timeout.ok_or(ObjectStatus::Param)?,
                )?),
                _ => None,
            };
            if !facts.exists(Object::Mailbox(id)) {
                return Err(ObjectStatus::NotCreated);
            }
            Ok(Admitted {
                object: Object::Mailbox(id),
                want: Pattern(0),
                mode: MatchMode::All,
                timeout,
                priority: None,
            })
        }
        Service::FlagCreate | Service::FlagDelete | Service::MailboxCreate | Service::MailboxDelete => {
            unreachable!()
        }
    }
}

/// The flag bodies' first post-lock check: a second waiter is refused before the match is tried.
pub fn flag_wait_locked(has_waiter: bool) -> Result<(), ObjectStatus> {
    if has_waiter {
        Err(ObjectStatus::QueueOverflow)
    } else {
        Ok(())
    }
}

/// A poll or a zero timeout that finds no match is -50, not a refusal.
pub fn poll_result(matched: Option<Pattern>) -> WaitOutcome<Pattern> {
    match matched {
        Some(pattern) => WaitOutcome::Satisfied(pattern),
        None => WaitOutcome::TimedOut,
    }
}

fn own_table<R: Records>(request: &Request, records: &R) -> Result<Admitted, ObjectStatus> {
    let domain = request.entry_domain.domain();
    let object = match request.service {
        Service::FlagCreate | Service::FlagDelete => Object::Flag(FlagId {
            domain,
            slot: own_flag_slot(request.id, records.flags(domain))?,
        }),
        _ => Object::Mailbox(MailboxId {
            domain,
            slot: own_mailbox_slot(request.id, records.mailboxes(domain))?,
        }),
    };
    Ok(Admitted {
        object,
        want: Pattern(0),
        mode: MatchMode::All,
        timeout: None,
        priority: None,
    })
}

fn flag(id: FlagId, want: Pattern, mode: MatchMode, timeout: Option<Timeout>) -> Admitted {
    Admitted {
        object: Object::Flag(id),
        want,
        mode,
        timeout,
        priority: None,
    }
}

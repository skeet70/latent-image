use crate::id::{own_flag_slot, own_mailbox_slot, resolve_flag, resolve_mailbox, EncodedId, EntryDomain, FlagId, MailboxId};
use crate::layout::MailboxConfig;
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

    fn blocks(self) -> bool {
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

/// The selected context record's published table counts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Records {
    pub flags: u32,
    pub mailboxes: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Facts {
    pub exists: bool,
    pub attributes: u32,
    pub priority_limit: u32,
}

/// The raw parameters a body reads before its first lock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Params {
    pub pattern: Pattern,
    pub mode: u32,
    pub timeout: Option<i32>,
    pub message_priority: Option<u32>,
}

impl Params {
    pub const NONE: Self = Self {
        pattern: Pattern(0),
        mode: 0,
        timeout: None,
        message_priority: None,
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
    pub records: Records,
    pub facts: Facts,
    pub params: Params,
}

/// The stock order: context, IRQ mask, dispatch word, class, index, family parameters, then the
/// creation record. Create and delete have no admission at all.
pub fn admit(request: &Request) -> Result<Admitted, ObjectStatus> {
    if request.service.owns_its_table() {
        return own_table(request);
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
            let id = resolve_flag(request.entry_domain, request.id, request.records.flags)?;
            exists(request.facts)?;
            Ok(flag(id, Pattern(0), MatchMode::All, None))
        }
        Service::FlagWait | Service::FlagPoll | Service::FlagTimedWait => {
            let id = resolve_flag(request.entry_domain, request.id, request.records.flags)?;
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
            exists(request.facts)?;
            Ok(flag(id, request.params.pattern, mode, timeout))
        }
        Service::MailboxSend => {
            let id = resolve_mailbox(request.entry_domain, request.id, request.records.mailboxes)?;
            exists(request.facts)?;
            let priority = if request.facts.attributes & MailboxConfig::MESSAGE_PRIORITY != 0 {
                let raw = request.params.message_priority.ok_or(ObjectStatus::Param)?;
                Some(MessagePriority::new(
                    raw,
                    PriorityLimit::from_word(request.facts.priority_limit),
                )?)
            } else {
                None
            };
            Ok(Admitted {
                object: Object::Mailbox(id),
                want: Pattern(0),
                mode: MatchMode::All,
                timeout: None,
                priority,
            })
        }
        Service::MailboxReceive | Service::MailboxPoll | Service::MailboxTimedReceive => {
            let id = resolve_mailbox(request.entry_domain, request.id, request.records.mailboxes)?;
            let timeout = match request.service {
                Service::MailboxTimedReceive => Some(Timeout::from_stock(
                    request.params.timeout.ok_or(ObjectStatus::Param)?,
                )?),
                _ => None,
            };
            exists(request.facts)?;
            Ok(Admitted {
                object: Object::Mailbox(id),
                want: Pattern(0),
                mode: MatchMode::All,
                timeout,
                priority: None,
            })
        }
        _ => unreachable!(),
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

fn own_table(request: &Request) -> Result<Admitted, ObjectStatus> {
    let domain = request.entry_domain.domain();
    let object = match request.service {
        Service::FlagCreate | Service::FlagDelete => Object::Flag(FlagId {
            domain,
            slot: own_flag_slot(request.id, request.records.flags)?,
        }),
        _ => Object::Mailbox(MailboxId {
            domain,
            slot: own_mailbox_slot(request.id, request.records.mailboxes)?,
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

fn exists(facts: Facts) -> Result<(), ObjectStatus> {
    if facts.exists {
        Ok(())
    } else {
        Err(ObjectStatus::NotCreated)
    }
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

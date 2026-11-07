use crate::id::{
    own_flag_slot, own_mailbox_slot, resolve_flag_in, resolve_mailbox_in, target, EncodedId,
    EntryDomain, FlagId, MailboxId,
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlagService {
    Set,
    SetHandler,
    Clear,
    Wait,
    Poll,
    TimedWait,
}

impl FlagService {
    pub const fn handler(self) -> bool {
        matches!(self, Self::SetHandler)
    }

    pub const fn blocks(self) -> bool {
        matches!(self, Self::Wait | Self::TimedWait)
    }

    pub const fn timed(self) -> bool {
        matches!(self, Self::TimedWait)
    }

    pub const fn service(self) -> Service {
        match self {
            Self::Set => Service::FlagSet,
            Self::SetHandler => Service::FlagSetHandler,
            Self::Clear => Service::FlagClear,
            Self::Wait => Service::FlagWait,
            Self::Poll => Service::FlagPoll,
            Self::TimedWait => Service::FlagTimedWait,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MailboxService {
    Send,
    Receive,
    Poll,
    TimedReceive,
}

impl MailboxService {
    pub const fn blocks(self) -> bool {
        matches!(self, Self::Receive | Self::TimedReceive)
    }

    pub const fn timed(self) -> bool {
        matches!(self, Self::TimedReceive)
    }

    pub const fn service(self) -> Service {
        match self {
            Self::Send => Service::MailboxSend,
            Self::Receive => Service::MailboxReceive,
            Self::Poll => Service::MailboxPoll,
            Self::TimedReceive => Service::MailboxTimedReceive,
        }
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
pub struct AdmittedFlag {
    pub id: FlagId,
    pub want: Pattern,
    pub mode: MatchMode,
    pub timeout: Option<Timeout>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AdmittedMailbox {
    pub id: MailboxId,
    pub timeout: Option<Timeout>,
    pub priority: Option<MessagePriority>,
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EntryPolicy {
    Task,
    BlockingTask,
    Handler,
}

impl EntryPolicy {
    fn handler(self) -> bool {
        self == EntryPolicy::Handler
    }

    fn blocking(self) -> bool {
        self == EntryPolicy::BlockingTask
    }
}

#[derive(Clone, Copy)]
struct EntryAdmission {
    entry: Entry,
    entry_domain: EntryDomain,
    id: EncodedId,
}

#[cfg_attr(feature = "family", inline(never))]
fn admit_entry(header: &EntryAdmission, policy: EntryPolicy) -> Result<Domain, ObjectStatus> {
    validate_entry(&header.entry, policy)?;
    target(header.entry_domain, header.id.class())
}

/// The stock order: context, IRQ mask, dispatch word, class, index, family parameters, then the
/// creation record. Create and delete have no admission at all.
pub fn admit_flag<R: Records, F: Facts>(
    request: &Request,
    service: FlagService,
    records: &R,
    facts: &F,
) -> Result<AdmittedFlag, ObjectStatus> {
    let header = EntryAdmission {
        entry: request.entry,
        entry_domain: request.entry_domain,
        id: request.id,
    };
    let policy = if service.handler() {
        EntryPolicy::Handler
    } else if service.blocks() {
        EntryPolicy::BlockingTask
    } else {
        EntryPolicy::Task
    };
    let domain = admit_entry(&header, policy)?;
    let id = resolve_flag_in(domain, request.id, records.flags(domain))?;
    let (want, mode, timeout) = match service {
        FlagService::Set | FlagService::SetHandler | FlagService::Clear => {
            (Pattern(0), MatchMode::All, None)
        }
        FlagService::Wait | FlagService::Poll | FlagService::TimedWait => {
            if request.params.pattern.0 == 0 {
                return Err(ObjectStatus::Param);
            }
            let mode = MatchMode::from_word(request.params.mode)?;
            let timeout = if service.timed() {
                Some(Timeout::from_stock(
                    request.params.timeout.ok_or(ObjectStatus::Param)?,
                )?)
            } else {
                None
            };
            (request.params.pattern, mode, timeout)
        }
    };
    if !facts.exists(Object::Flag(id)) {
        return Err(ObjectStatus::NotCreated);
    }
    Ok(AdmittedFlag {
        id,
        want,
        mode,
        timeout,
    })
}

pub fn admit_mailbox<R: Records, F: Facts>(
    request: &Request,
    service: MailboxService,
    records: &R,
    facts: &F,
) -> Result<AdmittedMailbox, ObjectStatus> {
    let header = EntryAdmission {
        entry: request.entry,
        entry_domain: request.entry_domain,
        id: request.id,
    };
    let policy = if service.blocks() {
        EntryPolicy::BlockingTask
    } else {
        EntryPolicy::Task
    };
    let domain = admit_entry(&header, policy)?;
    let id = resolve_mailbox_in(domain, request.id, records.mailboxes(domain))?;
    let object = Object::Mailbox(id);
    let send = matches!(service, MailboxService::Send);
    if send && !facts.exists(object) {
        return Err(ObjectStatus::NotCreated);
    }
    let priority = if send && facts.attributes(object) & MailboxConfig::MESSAGE_PRIORITY != 0 {
        let message = request.params.message.ok_or(ObjectStatus::Param)?;
        let raw = facts.message_priority(message);
        let limit = PriorityLimit::from_word(facts.priority_limit(object));
        Some(MessagePriority::new(raw, limit)?)
    } else {
        None
    };
    let timeout = if !send && service.timed() {
        Some(Timeout::from_stock(
            request.params.timeout.ok_or(ObjectStatus::Param)?,
        )?)
    } else {
        None
    };
    if !send && !facts.exists(object) {
        return Err(ObjectStatus::NotCreated);
    }
    Ok(AdmittedMailbox { id, timeout, priority })
}

/// Runtime dispatch for the host tests; the stage calls the typed functions.
pub fn admit<R: Records, F: Facts>(
    request: &Request,
    records: &R,
    facts: &F,
) -> Result<Admitted, ObjectStatus> {
    match request.service {
        Service::FlagCreate | Service::FlagDelete | Service::MailboxCreate | Service::MailboxDelete => {
            own_table(request, records, request.service)
        }
        Service::FlagSet => {
            admit_flag(request, FlagService::Set, records, facts).map(Admitted::from_flag)
        }
        Service::FlagSetHandler => {
            admit_flag(request, FlagService::SetHandler, records, facts).map(Admitted::from_flag)
        }
        Service::FlagClear => {
            admit_flag(request, FlagService::Clear, records, facts).map(Admitted::from_flag)
        }
        Service::FlagWait => {
            admit_flag(request, FlagService::Wait, records, facts).map(Admitted::from_flag)
        }
        Service::FlagPoll => {
            admit_flag(request, FlagService::Poll, records, facts).map(Admitted::from_flag)
        }
        Service::FlagTimedWait => {
            admit_flag(request, FlagService::TimedWait, records, facts).map(Admitted::from_flag)
        }
        Service::MailboxSend => {
            admit_mailbox(request, MailboxService::Send, records, facts).map(Admitted::from_mailbox)
        }
        Service::MailboxReceive => admit_mailbox(request, MailboxService::Receive, records, facts)
            .map(Admitted::from_mailbox),
        Service::MailboxPoll => {
            admit_mailbox(request, MailboxService::Poll, records, facts).map(Admitted::from_mailbox)
        }
        Service::MailboxTimedReceive => {
            admit_mailbox(request, MailboxService::TimedReceive, records, facts)
                .map(Admitted::from_mailbox)
        }
    }
}

impl Admitted {
    fn from_flag(flag: AdmittedFlag) -> Self {
        Self {
            object: Object::Flag(flag.id),
            want: flag.want,
            mode: flag.mode,
            timeout: flag.timeout,
            priority: None,
        }
    }

    fn from_mailbox(mailbox: AdmittedMailbox) -> Self {
        Self {
            object: Object::Mailbox(mailbox.id),
            want: Pattern(0),
            mode: MatchMode::All,
            timeout: mailbox.timeout,
            priority: mailbox.priority,
        }
    }
}

fn validate_entry(entry: &Entry, policy: EntryPolicy) -> Result<(), ObjectStatus> {
    if (entry.handler_context != 0) != policy.handler() {
        return Err(ObjectStatus::Context);
    }
    if entry.irq_masked {
        return Err(ObjectStatus::Context);
    }
    if policy.blocking() && entry.dispatch_enabled == 0 {
        return Err(ObjectStatus::Context);
    }
    Ok(())
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

fn own_table<R: Records>(
    request: &Request,
    records: &R,
    service: Service,
) -> Result<Admitted, ObjectStatus> {
    let domain = request.entry_domain.domain();
    let object = match service {
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

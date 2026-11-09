use crate::layout::{Domain, FlagSlot, MailboxSlot};
use crate::status::ObjectStatus;

/// The raw r0 word a stock slot receives; only the entry decodes it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct EncodedId(pub i32);

impl EncodedId {
    pub const fn of(class: u16, index: u16) -> Self {
        Self(((class as i32) << 16) | index as i32)
    }

    pub const fn class(self) -> i32 {
        self.0 >> 16
    }

    pub const fn index(self) -> i32 {
        (self.0 << 16) >> 16
    }
}

/// The loader whose slot body runs; a slot of loader A runs on either core.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct EntryDomain(Domain);

impl EntryDomain {
    pub const fn new(domain: Domain) -> Self {
        Self(domain)
    }

    pub const fn domain(self) -> Domain {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FlagId {
    pub domain: Domain,
    pub slot: FlagSlot,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MailboxId {
    pub domain: Domain,
    pub slot: MailboxSlot,
}

pub(crate) fn target(entry: EntryDomain, class: i32) -> Result<Domain, ObjectStatus> {
    match class {
        0 => Ok(entry.domain()),
        1 => Ok(Domain::A),
        2 => Ok(Domain::B),
        _ => Err(ObjectStatus::InvalidClass),
    }
}

/// The index check and slot construction once the class has selected the domain.
#[cfg_attr(feature = "family", inline(never))]
pub fn resolve_flag_in(domain: Domain, id: EncodedId, count: u32) -> Result<FlagId, ObjectStatus> {
    let slot = FlagSlot::new(index(id)?, count).ok_or(ObjectStatus::InvalidIndex)?;
    Ok(FlagId { domain, slot })
}

pub fn resolve_flag(entry: EntryDomain, id: EncodedId, count: u32) -> Result<FlagId, ObjectStatus> {
    let domain = target(entry, id.class())?;
    resolve_flag_in(domain, id, count)
}

/// Reads the count only after the class and index are valid, and only for the selected domain.
pub fn resolve_flag_with(
    entry: EntryDomain,
    id: EncodedId,
    count: impl FnOnce(Domain) -> u32,
) -> Result<FlagId, ObjectStatus> {
    let domain = target(entry, id.class())?;
    resolve_flag_in(domain, id, count(domain))
}

#[cfg_attr(feature = "family", inline(never))]
pub fn resolve_mailbox_in(domain: Domain, id: EncodedId, count: u32) -> Result<MailboxId, ObjectStatus> {
    let slot = MailboxSlot::new(index(id)?, count).ok_or(ObjectStatus::InvalidIndex)?;
    Ok(MailboxId { domain, slot })
}

pub fn resolve_mailbox(entry: EntryDomain, id: EncodedId, count: u32) -> Result<MailboxId, ObjectStatus> {
    let domain = target(entry, id.class())?;
    resolve_mailbox_in(domain, id, count)
}

pub fn resolve_mailbox_with(
    entry: EntryDomain,
    id: EncodedId,
    count: impl FnOnce(Domain) -> u32,
) -> Result<MailboxId, ObjectStatus> {
    let domain = target(entry, id.class())?;
    resolve_mailbox_in(domain, id, count(domain))
}

/// Create and delete ignore the class half: `uxth(id)` indexes the entry domain's own table.
pub fn own_flag_slot(id: EncodedId, count: u32) -> Result<FlagSlot, ObjectStatus> {
    FlagSlot::new(uxth(id), count).ok_or(ObjectStatus::InvalidIndex)
}

pub fn own_mailbox_slot(id: EncodedId, count: u32) -> Result<MailboxSlot, ObjectStatus> {
    MailboxSlot::new(uxth(id), count).ok_or(ObjectStatus::InvalidIndex)
}

fn index(id: EncodedId) -> Result<u32, ObjectStatus> {
    let index = id.index();
    if index <= 0 {
        return Err(ObjectStatus::InvalidIndex);
    }
    Ok(index as u32)
}

fn uxth(id: EncodedId) -> u32 {
    (id.0 as u32) & 0xffff
}

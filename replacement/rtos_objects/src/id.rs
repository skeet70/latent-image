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

pub fn target(executing: Domain, class: i32) -> Result<Domain, ObjectStatus> {
    match class {
        0 => Ok(executing),
        1 => Ok(Domain::A),
        2 => Ok(Domain::B),
        _ => Err(ObjectStatus::InvalidClass),
    }
}

pub fn resolve_flag(executing: Domain, id: EncodedId, count: u32) -> Result<FlagId, ObjectStatus> {
    let domain = target(executing, id.class())?;
    let slot = FlagSlot::new(index(id)?, count).ok_or(ObjectStatus::InvalidIndex)?;
    Ok(FlagId { domain, slot })
}

pub fn resolve_mailbox(executing: Domain, id: EncodedId, count: u32) -> Result<MailboxId, ObjectStatus> {
    let domain = target(executing, id.class())?;
    let slot = MailboxSlot::new(index(id)?, count).ok_or(ObjectStatus::InvalidIndex)?;
    Ok(MailboxId { domain, slot })
}

/// Create and delete ignore the class half: `uxth(id)` indexes the executing kernel's own table.
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

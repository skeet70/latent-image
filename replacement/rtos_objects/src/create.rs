use crate::id::{FlagId, MailboxId};
use crate::layout::GuestAddress;
use crate::matching::Pattern;
use crate::message::PriorityLimit;
use crate::status::ObjectStatus;

/// The I/F bits the stock mask pair returned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct SavedInterrupts(pub u32);

/// A flag creation record's attribute word, arbitrary bits preserved.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct FlagAttributes(pub u32);

/// A mailbox creation record's attribute word, arbitrary bits preserved.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct MailboxAttributes(pub u32);

pub trait Machine {
    fn load_word(&self, at: GuestAddress) -> u32;
    fn store_word(&self, at: GuestAddress, value: u32);
    fn mask_interrupts(&self) -> SavedInterrupts;
    fn restore_interrupts(&self, saved: SavedInterrupts);
    fn dmb(&self);
}

pub struct FlagCreate {
    pub attributes: FlagAttributes,
    pub initial: Pattern,
}

pub struct MailboxCreate {
    pub attributes: MailboxAttributes,
    pub priority_limit: PriorityLimit,
}

/// `+0x80`. The table is the id's domain, which `admit` set to the entry domain.
pub fn create_flag<M: Machine>(m: &M, id: FlagId, create: FlagCreate) -> Result<FlagId, ObjectStatus> {
    let words = id.domain.words();
    let config = words.flag_config(id.slot);
    let saved = m.mask_interrupts();
    let result = if m.load_word(config.exists()) != 0 {
        Err(ObjectStatus::AlreadyExists)
    } else {
        m.store_word(config.attributes(), create.attributes.0);
        m.store_word(config.initial(), create.initial.0);
        let control = words.flag_control(id.slot);
        m.store_word(control.queue().next(), control.0 .0);
        m.store_word(control.queue().prev(), control.0 .0);
        m.store_word(control.pattern(), m.load_word(config.initial()));
        m.dmb();
        m.store_word(config.exists(), 1);
        Ok(id)
    };
    m.restore_interrupts(saved);
    result
}

/// `+0xcc`. `tail` is written 0 here, which stock leaves to the first FIFO append.
pub fn create_mailbox<M: Machine>(
    m: &M,
    id: MailboxId,
    create: MailboxCreate,
) -> Result<MailboxId, ObjectStatus> {
    let words = id.domain.words();
    let config = words.mailbox_config(id.slot);
    let saved = m.mask_interrupts();
    let result = if m.load_word(config.exists()) != 0 {
        Err(ObjectStatus::AlreadyExists)
    } else {
        m.store_word(config.attributes(), create.attributes.0);
        m.store_word(config.priority_limit(), create.priority_limit.0);
        let control = words.mailbox_control(id.slot);
        m.store_word(control.queue().next(), control.0 .0);
        m.store_word(control.queue().prev(), control.0 .0);
        m.store_word(control.head(), 0);
        m.store_word(control.tail(), 0);
        m.dmb();
        m.store_word(config.exists(), 1);
        Ok(id)
    };
    m.restore_interrupts(saved);
    result
}

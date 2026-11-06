use crate::id::{FlagId, MailboxId};
use crate::layout::GuestAddress;
use crate::status::ObjectStatus;

/// The words a create reads and writes, plus the stock I/F mask pair
/// (A `0x42560` / B `0xd2560`, restored by A `0x42568` / B `0xd2568`).
pub trait Machine {
    fn load_word(&self, at: GuestAddress) -> u32;
    fn store_word(&self, at: GuestAddress, value: u32);
    fn mask_interrupts(&self) -> u32;
    fn restore_interrupts(&self, saved: u32);
}

pub struct FlagCreate {
    pub attributes: u32,
    pub initial: u32,
}

pub struct MailboxCreate {
    pub attributes: u32,
    pub priority_limit: u32,
}

/// `+0x80`. The table is the id's domain, which `admit` set to the entry domain.
pub fn create_flag<M: Machine>(m: &M, id: FlagId, create: FlagCreate) -> Result<FlagId, ObjectStatus> {
    let words = id.domain.words();
    let config = words.flag_config(id.slot);
    let saved = m.mask_interrupts();
    let result = if m.load_word(config.exists()) != 0 {
        Err(ObjectStatus::AlreadyExists)
    } else {
        m.store_word(config.attributes(), create.attributes);
        m.store_word(config.initial(), create.initial);
        m.store_word(config.exists(), 1);
        let control = words.flag_control(id.slot);
        m.store_word(control.queue().next(), control.0 .0);
        m.store_word(control.queue().prev(), control.0 .0);
        m.store_word(control.pattern(), m.load_word(config.initial()));
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
        m.store_word(config.attributes(), create.attributes);
        m.store_word(config.priority_limit(), create.priority_limit);
        m.store_word(config.exists(), 1);
        let control = words.mailbox_control(id.slot);
        m.store_word(control.queue().next(), control.0 .0);
        m.store_word(control.queue().prev(), control.0 .0);
        m.store_word(control.head(), 0);
        m.store_word(control.tail(), 0);
        Ok(id)
    };
    m.restore_interrupts(saved);
    result
}

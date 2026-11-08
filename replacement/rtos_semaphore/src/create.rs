use crate::count::{Count, Maximum, SemaphoreAttributes};
use crate::id::SemaphoreId;
use crate::layout::GuestAddress;
use crate::status::SemaphoreStatus;

/// The I/F bits the stock mask pair returned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct SavedInterrupts(pub u32);

pub trait Machine {
    fn load_word(&self, at: GuestAddress) -> u32;
    fn load_byte(&self, at: GuestAddress) -> u8;
    fn store_word(&self, at: GuestAddress, value: u32);
    fn store_byte(&self, at: GuestAddress, value: u8);
    /// The stock pair at A `0x42560`/`0x42568` and B `0xd2560`/`0xd2568`.
    fn mask_interrupts(&self) -> SavedInterrupts;
    fn restore_interrupts(&self, saved: SavedInterrupts);
    /// Restore the saved F bit and keep I set: the dispatcher's switch must stay IRQ-masked.
    fn restore_f(&self, saved: SavedInterrupts);
    fn dmb(&self);
}

pub struct SemaphoreCreate {
    pub attributes: SemaphoreAttributes,
    pub initial: Count,
    pub maximum: Maximum,
}

/// `+0x64`. The creation record is published last; stock writes its exists word before the count.
#[inline(always)]
pub fn create_semaphore<M: Machine>(
    m: &M,
    id: SemaphoreId,
    read: impl FnOnce() -> SemaphoreCreate,
) -> Result<(), SemaphoreStatus> {
    let words = id.domain.words();
    let config = words.semaphore_config(id.slot);
    let control = words.semaphore_control(id.slot);
    let saved = m.mask_interrupts();
    let result = if m.load_word(config.exists()) != 0 {
        Err(SemaphoreStatus::AlreadyExists)
    } else {
        let create = read();
        m.store_word(config.attributes(), create.attributes.0);
        m.store_word(config.initial(), create.initial.get());
        m.store_word(config.maximum(), create.maximum.get());
        m.store_word(control.count(), create.initial.get());
        m.dmb();
        m.store_word(config.exists(), 1);
        Ok(())
    };
    m.restore_interrupts(saved);
    result
}

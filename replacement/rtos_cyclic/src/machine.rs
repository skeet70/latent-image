use crate::layout::{Domain, GuestAddress, KernelContext};
use crate::types::{CyclicArgument, CyclicHandler, SavedInterrupts};

pub trait Machine {
    fn load_word(&self, at: GuestAddress) -> u32;
    fn store_word(&self, at: GuestAddress, value: u32);
    fn mask_interrupts(&self) -> SavedInterrupts;
    fn restore_interrupts(&self, saved: SavedInterrupts);
    fn handler_context(&self) -> u32;
    fn irq_masked(&self) -> bool;
    fn irq_set(&self);
    fn irq_clear(&self);
    fn current_context(&self) -> KernelContext;
    fn context(&self, domain: Domain) -> KernelContext;
    fn acquire_lock(&self, context: KernelContext) -> bool;
    fn release_lock(&self, context: KernelContext);
    /// True when the entry is in the heap afterwards.
    fn insert_timer(&self, context: KernelContext, entry: GuestAddress, expiry: u32) -> bool;
    /// True when the entry is detached afterwards.
    fn remove_timer(&self, context: KernelContext, entry: GuestAddress) -> bool;
    fn invoke(&self, handler: CyclicHandler, argument: CyclicArgument);
}

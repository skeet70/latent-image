use crate::id::PoolId;
use crate::layout::GuestAddress;
use crate::machine::Machine;
use crate::status::PoolStatus;

/// A pool creation record's attribute word, arbitrary bits preserved.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct PoolAttributes(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PoolCreate {
    pub attributes: PoolAttributes,
    pub block_count: u32,
    pub block_size: u32,
    pub buffer: GuestAddress,
}

impl PoolCreate {
    /// The rounded stride the creation record stores.
    #[inline(always)]
    pub const fn stride(&self) -> u32 {
        self.block_size.wrapping_add(3) & !3
    }

    /// The region end the owned record stores, `buffer + stride * count`.
    #[inline(always)]
    pub const fn end(&self) -> GuestAddress {
        GuestAddress(self.buffer.0.wrapping_add(self.stride().wrapping_mul(self.block_count)))
    }
}

/// Slot `+0xe4`. Stock publishes `exists` before resetting the control block; the owned create
/// resets the control block and issues `dmb` before the marker.
#[inline(never)]
#[link_section = ".text.fp_body_create_pool"]
pub fn create_pool<M: Machine>(m: &M, id: PoolId, create: PoolCreate) -> Result<(), PoolStatus> {
    let words = id.domain.words();
    let config = words.fixed_pool_config(id.slot);
    let control = words.fixed_pool_control(id.slot);
    let saved = m.mask_interrupts();
    let result = if m.load_word(config.exists()) != 0 {
        Err(PoolStatus::AlreadyExists)
    } else {
        m.store_word(config.attributes(), create.attributes.0);
        m.store_word(config.block_size(), create.stride());
        m.store_word(config.begin(), create.buffer.0);
        m.store_word(config.end(), create.end().0);
        m.store_word(control.queue().next(), control.0 .0);
        m.store_word(control.queue().prev(), control.0 .0);
        m.store_word(control.next_free(), create.buffer.0);
        m.store_word(control.recycled(), 0);
        m.dmb();
        m.store_word(config.exists(), 1);
        Ok(())
    };
    m.restore_interrupts(saved);
    result
}

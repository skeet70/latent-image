use crate::id::PoolId;
use crate::layout::GuestAddress;
use crate::machine::Machine;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BlockAddress(pub GuestAddress);

/// The allocator A `0x4b41c` / B `0xdb3dc`: pop the free list, else bump while below the region
/// end. The caller holds the pool's object lock.
#[inline(always)]
pub fn take<M: Machine>(m: &M, id: PoolId) -> Option<BlockAddress> {
    let words = id.domain.words();
    let config = words.fixed_pool_config(id.slot);
    let control = words.fixed_pool_control(id.slot);
    let recycled = m.load_word(control.recycled());
    if recycled != 0 {
        m.store_word(control.recycled(), m.load_word(GuestAddress(recycled)));
        return Some(BlockAddress(GuestAddress(recycled)));
    }
    let next = m.load_word(control.next_free());
    if next < m.load_word(config.end()) {
        m.store_word(control.next_free(), next.wrapping_add(m.load_word(config.block_size())));
        return Some(BlockAddress(GuestAddress(next)));
    }
    None
}

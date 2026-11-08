use crate::id::PoolId;
use crate::layout::GuestAddress;
use crate::machine::Machine;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BlockAddress(pub GuestAddress);

/// The allocator A `0x4b41c` / B `0xdb3dc`: pop the free list, else bump below the region end. The
/// output is stored before the frontier advances; the caller holds the object lock.
#[inline(never)]
#[link_section = ".text.fp_body_take"]
pub fn take<M: Machine>(m: &M, id: PoolId, out: GuestAddress) -> Option<BlockAddress> {
    let words = id.domain.words();
    let config = words.fixed_pool_config(id.slot);
    let control = words.fixed_pool_control(id.slot);
    let recycled = m.load_word(control.recycled());
    if recycled != 0 {
        m.store_word(control.recycled(), m.load_word(GuestAddress(recycled)));
        m.store_word(out, recycled);
        return Some(BlockAddress(GuestAddress(recycled)));
    }
    let next = m.load_word(control.next_free());
    if next < m.load_word(config.end()) {
        m.store_word(out, next);
        m.store_word(control.next_free(), next.wrapping_add(m.load_word(config.block_size())));
        return Some(BlockAddress(GuestAddress(next)));
    }
    None
}

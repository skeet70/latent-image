use crate::id::PoolId;
use crate::layout::{FixedPoolWait, GuestAddress, Task};
use crate::machine::{DivMod, DispatchTarget, Machine, Selection, Wake};
use crate::status::PoolStatus;
use crate::take::BlockAddress;

#[inline(always)]
fn unlink<M: Machine>(m: &M, task: Task) {
    let prev = m.load_word(task.prev());
    let next = m.load_word(task.next());
    m.store_word(GuestAddress(prev), next);
    m.store_word(GuestAddress(next).offset(0x04), prev);
}

/// Slot `+0xf8`. Range and stride are checked before the lock as stock does; the owned body also
/// refuses a block that is above the bump frontier or already on the free list.
#[inline(always)]
pub fn release<M: Wake + DivMod>(m: &M, id: PoolId, block: BlockAddress) -> Result<(), PoolStatus> {
    let words = id.domain.words();
    let config = words.fixed_pool_config(id.slot);
    let control = words.fixed_pool_control(id.slot);
    let begin = m.load_word(config.begin());
    let end = m.load_word(config.end());
    let stride = m.load_word(config.block_size());
    let address = block.0 .0;
    if stride == 0 || address < begin || address >= end {
        return Err(PoolStatus::Param);
    }
    let (_, remainder) = m.divmod(address - begin, stride);
    if remainder != 0 {
        return Err(PoolStatus::Param);
    }
    let (count, _) = m.divmod(end - begin, stride);
    let lock = words.context(id.domain).object_lock();
    m.disable_irq();
    m.lock(lock);
    let result = release_locked(m, address, count, control);
    m.unlock(lock);
    if let Ok(dispatch) = result {
        if dispatch {
            m.dispatch(-71);
        }
    }
    m.enable_irq();
    result.map(|_| ())
}

#[inline(always)]
fn release_locked<M: Wake + DivMod>(
    m: &M,
    address: u32,
    count: u32,
    control: crate::layout::FixedPoolControl,
) -> Result<bool, PoolStatus> {
    if address >= m.load_word(control.next_free()) {
        return Err(PoolStatus::Param);
    }
    let mut cursor = m.load_word(control.recycled());
    let mut steps = 0;
    while cursor != 0 {
        if cursor == address || steps >= count {
            return Err(PoolStatus::Param);
        }
        cursor = m.load_word(GuestAddress(cursor));
        steps += 1;
    }
    let head = m.load_word(control.queue().next());
    if head == control.0 .0 {
        m.store_word(GuestAddress(address), m.load_word(control.recycled()));
        m.store_word(control.recycled(), address);
        return Ok(false);
    }
    let task = Task(GuestAddress(head));
    let owner_domain = m.waiter_domain(task);
    let owner = m.context(owner_domain);
    m.lock(owner.scheduler_lock());
    let wait = FixedPoolWait(GuestAddress(m.load_word(task.wait())));
    m.store_word(wait.block(), address);
    unlink(m, task);
    let changed = m.release(owner, task) == Selection::Changed;
    let dispatch = changed && m.notify(m.record_id(owner)) == DispatchTarget::Local;
    m.unlock(owner.scheduler_lock());
    Ok(dispatch)
}

use crate::id::PoolId;
use crate::layout::{GuestAddress, Task};
use crate::machine::{DispatchTarget, Selection, Wake};
use crate::status::PoolStatus;

/// Slot `+0xe8`. Stock resets the record and leaves queued waiters blocked; the owned delete
/// releases each with -49 first, under the object lock the stock body does not take.
#[inline(always)]
pub fn delete_pool<M: Wake>(m: &M, id: PoolId, handler: bool) -> Result<(), PoolStatus> {
    let words = id.domain.words();
    let config = words.fixed_pool_config(id.slot);
    let control = words.fixed_pool_control(id.slot);
    let lock = words.context(id.domain).object_lock();
    let saved = m.mask_interrupts();
    m.lock(lock);
    let mut dispatch = false;
    loop {
        let head = m.load_word(control.queue().next());
        if head == control.0 .0 {
            break;
        }
        let task = Task(GuestAddress(head));
        let owner = m.context(m.waiter_domain(task));
        m.lock(owner.scheduler_lock());
        let changed = m.cancel(owner, task) == Selection::Changed;
        dispatch |= changed && m.notify(m.record_id(owner)) == DispatchTarget::Local;
        m.unlock(owner.scheduler_lock());
    }
    let default = words.default_pool_storage;
    m.store_word(control.queue().next(), control.0 .0);
    m.store_word(control.queue().prev(), control.0 .0);
    m.store_word(control.next_free(), default.0);
    m.store_word(control.recycled(), 0);
    m.store_word(config.attributes(), 0);
    m.store_word(config.block_size(), 4);
    m.store_word(config.begin(), default.0);
    m.store_word(config.end(), default.offset(0x04).0);
    m.store_word(config.exists(), 0);
    m.unlock(lock);
    if dispatch {
        if handler {
            m.request_delayed_dispatch();
        } else {
            m.dispatch(0);
        }
    }
    m.restore_interrupts(saved);
    Ok(())
}

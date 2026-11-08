use crate::count::Count;
use crate::id::SemaphoreId;
use crate::layout::{GuestAddress, Task};
use crate::status::SemaphoreStatus;
use crate::wake::{DispatchTarget, Selection, Wake};

/// Slot `+0x68`. Stock resets the record but leaves queued waiters linked; the owned delete
/// releases each with -49 first.
#[inline(always)]
pub fn delete_semaphore<M: Wake>(m: &M, id: SemaphoreId) -> Result<(), SemaphoreStatus> {
    let words = id.domain.words();
    let control = words.semaphore_control(id.slot);
    let config = words.semaphore_config(id.slot);
    let object_lock = words.context(id.domain).object_lock();
    let saved = m.mask_interrupts();
    m.lock(object_lock);
    let mut dispatch = false;
    loop {
        let waiter = m.load_word(control.queue().next());
        if waiter == control.0 .0 {
            break;
        }
        let task = Task(GuestAddress(waiter));
        let owner = m.context(m.waiter_domain(task));
        m.lock(owner.scheduler_lock());
        let changed = m.cancel(owner, task) == Selection::Changed;
        let local = changed && m.notify(m.record_id(owner)) == DispatchTarget::Local;
        m.unlock(owner.scheduler_lock());
        dispatch |= local;
    }
    m.store_word(control.queue().next(), control.0 .0);
    m.store_word(control.queue().prev(), control.0 .0);
    m.store_word(config.attributes(), 0);
    m.store_word(config.initial(), 1);
    m.store_word(config.maximum(), 1);
    m.store_word(config.exists(), 0);
    m.store_word(control.count(), Count::from_word(1).get());
    m.unlock(object_lock);
    m.restore_f(saved);
    if dispatch {
        m.finish_dispatch(0);
    }
    m.restore_interrupts(saved);
    Ok(())
}

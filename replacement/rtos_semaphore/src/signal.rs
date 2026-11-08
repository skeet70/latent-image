use crate::count::{Count, Maximum};
use crate::id::SemaphoreId;
use crate::layout::{GuestAddress, Task};
use crate::status::SemaphoreStatus;
use crate::wake::{unlink, DispatchTarget, Selection, Wake, WakeMode};

/// Slots `+0x6c` and `+0x70`. The count is unchanged when a waiter is readied.
#[inline(always)]
pub fn signal<M: Wake>(
    m: &M,
    id: SemaphoreId,
    mode: WakeMode,
) -> Result<(), SemaphoreStatus> {
    let words = id.domain.words();
    let control = words.semaphore_control(id.slot);
    let config = words.semaphore_config(id.slot);
    let object_lock = words.context(id.domain).object_lock();
    m.disable_irq();
    m.lock(object_lock);
    let waiter = m.load_word(control.queue().next());
    if waiter == control.0 .0 {
        let count = Count::from_word(m.load_word(control.count()));
        let maximum = Maximum::from_word(m.load_word(config.maximum()));
        let result = if count.below(maximum) {
            m.store_word(control.count(), count.increment().get());
            Ok(())
        } else {
            Err(SemaphoreStatus::Overflow)
        };
        m.unlock(object_lock);
        m.enable_irq();
        return result;
    }
    let task = Task(GuestAddress(waiter));
    let owner = m.context(m.waiter_domain(task));
    m.lock(owner.scheduler_lock());
    unlink(m, task);
    let dispatch = match m.release(owner, task) {
        Selection::Changed => m.notify(m.record_id(owner)) == DispatchTarget::Local,
        Selection::Unchanged => false,
    };
    m.unlock(owner.scheduler_lock());
    m.unlock(object_lock);
    if dispatch {
        match mode {
            WakeMode::Task => m.dispatch(-35),
            WakeMode::Handler => m.request_delayed_dispatch(),
        }
    }
    m.enable_irq();
    Ok(())
}

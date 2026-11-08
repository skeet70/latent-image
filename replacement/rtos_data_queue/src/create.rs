use crate::env::{
    release, unlink, DispatchTarget, Env, HandlerContext, Selection, WaitCompletion,
};
use crate::id::DataQueueId;
use crate::layout::{GuestAddress, Task, WaitQueue};
use crate::ring::{Capacity, RingBase};
use crate::status::ObjectStatus;

/// A creation record's attribute word, arbitrary bits preserved.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct QueueAttributes(pub u32);

pub struct DataQueueCreate {
    pub attributes: QueueAttributes,
    pub capacity: Capacity,
    pub ring: RingBase,
}

/// Slot `+0xa0`.
pub fn create_data_queue<M: Env>(
    m: &M,
    id: DataQueueId,
    create: DataQueueCreate,
) -> Result<DataQueueId, ObjectStatus> {
    let words = id.domain.words();
    let config = words.data_queue_config(id.slot);
    let saved = m.mask_if();
    let result = if m.load_word(config.exists()) != 0 {
        Err(ObjectStatus::AlreadyExists)
    } else {
        m.store_word(config.attributes(), create.attributes.0);
        m.store_word(config.capacity(), create.capacity.0);
        m.store_word(config.ring_base(), create.ring.0 .0);
        let control = words.data_queue_control(id.slot);
        m.store_word(control.sender_queue().next(), control.0 .0);
        m.store_word(control.sender_queue().prev(), control.0 .0);
        m.store_word(control.receivers().next(), control.receivers().0 .0);
        m.store_word(control.receivers().prev(), control.receivers().0 .0);
        m.store_word(control.count(), 0);
        m.store_word(control.read(), 0);
        m.store_word(control.write(), 0);
        m.dmb();
        m.store_word(config.exists(), 1);
        Ok(id)
    };
    m.restore_if(saved);
    result
}

fn release_all<M: Env>(m: &M, queue: WaitQueue) -> bool {
    let mut dispatch = false;
    loop {
        let waiter = m.load_word(queue.next());
        if waiter == queue.0 .0 {
            break;
        }
        let task = Task(GuestAddress(waiter));
        let owner_domain = m.waiter_domain(task);
        let owner = m.context(owner_domain);
        let changed = m.with_scheduler_lock(owner_domain, || {
            unlink(m, task);
            match release(m, owner, task, WaitCompletion::Released) {
                Selection::Changed => m.notify(m.record_id(owner)) == DispatchTarget::Local,
                Selection::Unchanged => false,
            }
        });
        dispatch |= changed;
    }
    dispatch
}

/// Slot `+0xa4`; every queued sender and receiver is released with `-49` before the reset.
pub fn delete_data_queue<M: Env>(
    m: &M,
    id: DataQueueId,
    caller: HandlerContext,
) -> Result<(), ObjectStatus> {
    let words = id.domain.words();
    let config = words.data_queue_config(id.slot);
    let control = words.data_queue_control(id.slot);
    let lock = words.context(id.domain).object_lock();
    m.with_object_lock_after(
        lock,
        || {
            let dispatch = release_all(m, control.sender_queue())
                | release_all(m, WaitQueue(control.receivers().0));
            m.store_word(config.attributes(), 0);
            m.store_word(config.capacity(), 1);
            m.store_word(config.ring_base(), words.data_queue_placeholder.0);
            m.store_word(config.exists(), 0);
            m.store_word(control.sender_queue().next(), control.0 .0);
            m.store_word(control.sender_queue().prev(), control.0 .0);
            m.store_word(control.receivers().next(), control.receivers().0 .0);
            m.store_word(control.receivers().prev(), control.receivers().0 .0);
            m.store_word(control.count(), 0);
            m.store_word(control.read(), 0);
            m.store_word(control.write(), 0);
            dispatch
        },
        |dispatch| {
            if *dispatch {
                match caller {
                    HandlerContext::Task => m.dispatch(0),
                    HandlerContext::Handler => m.request_delayed_dispatch(),
                }
            }
        },
    );
    Ok(())
}


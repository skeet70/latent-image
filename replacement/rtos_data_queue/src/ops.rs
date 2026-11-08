use crate::env::{
    dispatch_after, release, unlink, Deadline, Env, QueueOrder, TimerEntryAddress, WaitCompletion,
};
use crate::id::{DataQueueId, EntryDomain};
use crate::layout::{
    DataQueueControl, DataQueueWait, Domain, GuestAddress, Task, TaskState, WaitQueue,
};
use crate::ring::{OutWord, Payload, Ring};
use crate::status::{ObjectStatus, WaitOutcome};
use crate::timeout::Timeout;

const TIMER_ENTRY_CALLBACK: u32 = 0x04;
const TIMER_ENTRY_ARGUMENT: u32 = 0x08;

pub const SEND_STATE: TaskState = TaskState::WAIT
    .union(TaskState::OBJECT_QUEUED)
    .union(TaskState::OWNER_LOCK_QUEUED);

pub const RECEIVE_STATE: TaskState = TaskState::WAIT.union(TaskState::OBJECT_QUEUED);

#[derive(Debug, PartialEq, Eq)]
pub struct SendPending {
    task: Task,
    wait: DataQueueWait,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ReceivePending {
    task: Task,
    wait: DataQueueWait,
    out: OutWord,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SendBlocking {
    Ready(WaitOutcome<()>),
    Pending(SendPending),
    Retry,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReceiveBlocking {
    Ready(WaitOutcome<Payload>),
    Pending(ReceivePending),
    Retry,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SendResume {
    Completed(WaitOutcome<()>),
    Pending(SendPending),
    Corrupt(SendPending),
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReceiveResume {
    Completed(WaitOutcome<Payload>),
    Pending(ReceivePending),
    Corrupt(ReceivePending),
}

fn block<M: Env>(
    m: &M,
    entry: Domain,
    object: Domain,
    task: Task,
    wait: DataQueueWait,
    record: DataQueueControl,
    queue: WaitQueue,
    order: QueueOrder,
    state: TaskState,
    timer: Option<(TimerEntryAddress, Deadline)>,
) -> bool {
    let owner = m.context(object);
    m.with_scheduler_lock(entry, || {
        if m.load_byte(task.state()) & TaskState::READY.bits() == 0 {
            return false;
        }
        m.store_byte(task.state(), state.bits());
        m.remove_ready(m.context(entry), task);
        m.store_word(task.wait(), wait.0 .0);
        match timer {
            Some((timer_entry, deadline)) => {
                m.store_word(wait.base().result(), timer_entry.0 .0);
                m.store_word(
                    timer_entry.0.offset(TIMER_ENTRY_CALLBACK),
                    m.timeout_callback().0,
                );
                m.store_word(timer_entry.0.offset(TIMER_ENTRY_ARGUMENT), task.0 .0);
                m.arm_timer(m.context(entry), timer_entry, deadline);
            }
            None => m.store_word(wait.base().result(), 0),
        }
        m.store_word(wait.base().queue(), record.0 .0);
        m.store_word(wait.base().owner(), m.load_word(owner.owner()));
        m.queue_wait(owner, task, queue, order);
        true
    })
}

/// `Some(local)` when a receiver took the word, where `local` means the caller dispatches.
fn handoff_receiver<M: Env>(m: &M, control: DataQueueControl, payload: Payload) -> Option<bool> {
    let first = m.load_word(control.receivers().next());
    if first == control.receivers().0 .0 {
        return None;
    }
    let task = Task(GuestAddress(first));
    let owner_domain = m.waiter_domain(task);
    let owner = m.context(owner_domain);
    let wait = DataQueueWait(GuestAddress(m.load_word(task.wait())));
    m.store_word(wait.data(), payload.0);
    let local = m.with_scheduler_lock(owner_domain, || {
        unlink(m, task);
        dispatch_after(m, owner, release(m, owner, task, WaitCompletion::Satisfied))
    });
    Some(local)
}

/// `Some((received, local))` when a sender delivered its word.
fn take_sender<M: Env>(
    m: &M,
    control: DataQueueControl,
    ring: Ring,
    out: OutWord,
) -> Option<(Payload, bool)> {
    let first = m.load_word(control.sender_queue().next());
    if first == control.sender_queue().0 .0 {
        return None;
    }
    let task = Task(GuestAddress(first));
    let owner_domain = m.waiter_domain(task);
    let owner = m.context(owner_domain);
    let wait = DataQueueWait(GuestAddress(m.load_word(task.wait())));
    let sent = Payload(m.load_word(wait.data()));
    let (received, local) = m.with_scheduler_lock(owner_domain, || {
        unlink(m, task);
        let received = if ring.count(m).0 != 0 {
            let received = ring.pop(m, out);
            ring.append(m, sent);
            received
        } else {
            m.store_word(out.0, sent.0);
            sent
        };
        let local = dispatch_after(m, owner, release(m, owner, task, WaitCompletion::Satisfied));
        (received, local)
    });
    Some((received, local))
}

fn timer_for<M: Env>(
    m: &M,
    entry: Domain,
    slot: crate::layout::TaskSlot,
    timeout: Timeout,
) -> Option<(TimerEntryAddress, Deadline)> {
    match timeout {
        Timeout::Units(units) => Some((
            TimerEntryAddress(entry.words().data_queue_timer(slot)),
            Deadline(m.tick(m.context(entry)).0.wrapping_add(units)),
        )),
        _ => None,
    }
}

fn task_wait<M: Env>(
    m: &M,
    entry: EntryDomain,
) -> Result<(Task, DataQueueWait, crate::layout::TaskSlot), ObjectStatus> {
    let task = m.current(entry.domain());
    let slot = entry
        .domain()
        .words()
        .task_slot(task)
        .ok_or(ObjectStatus::Param)?;
    Ok((task, entry.domain().words().data_queue_wait(slot), slot))
}

/// Slot `+0xa8`; a full ring blocks the sender without a timer.
pub fn send<M: Env>(
    m: &M,
    entry: EntryDomain,
    id: DataQueueId,
    payload: Payload,
) -> Result<SendBlocking, ObjectStatus> {
    let words = id.domain.words();
    let config = words.data_queue_config(id.slot);
    let control = words.data_queue_control(id.slot);
    let ring = Ring::new(config, control);
    let lock = words.context(id.domain).object_lock();
    let result = m.with_object_lock_after(
        lock,
        || -> Result<(SendBlocking, bool, bool), ObjectStatus> {
            if let Some(local) = handoff_receiver(m, control, payload) {
                return Ok((SendBlocking::Ready(WaitOutcome::Satisfied(())), local, false));
            }
            if ring.append(m, payload) {
                return Ok((SendBlocking::Ready(WaitOutcome::Satisfied(())), false, false));
            }
            let (task, wait, _) = task_wait(m, entry)?;
            m.store_word(wait.data(), payload.0);
            let blocked = block(
                m,
                entry.domain(),
                id.domain,
                task,
                wait,
                control,
                control.sender_queue(),
                QueueOrder::from_attribute(m.load_word(config.attributes())),
                SEND_STATE,
                None,
            );
            Ok((
                SendBlocking::Pending(SendPending { task, wait }),
                false,
                !blocked,
            ))
        },
        |result| {
            if let Ok((_, true, _)) = result {
                m.dispatch(-53);
            }
        },
    );
    match result {
        Ok((outcome, _, false)) => Ok(outcome),
        Ok((_, _, true)) => Ok(SendBlocking::Retry),
        Err(status) => Err(status),
    }
}

/// Slot `+0xac` and `+0xb0`.
pub fn send_poll<M: Env>(
    m: &M,
    id: DataQueueId,
    payload: Payload,
    handler: bool,
) -> Result<WaitOutcome<()>, ObjectStatus> {
    let words = id.domain.words();
    let config = words.data_queue_config(id.slot);
    let control = words.data_queue_control(id.slot);
    let ring = Ring::new(config, control);
    let lock = words.context(id.domain).object_lock();
    let result = m.with_object_lock_after(
        lock,
        || {
            if let Some(local) = handoff_receiver(m, control, payload) {
                return Ok((WaitOutcome::Satisfied(()), local));
            }
            if ring.append(m, payload) {
                Ok((WaitOutcome::Satisfied(()), false))
            } else {
                Ok((WaitOutcome::TimedOut, false))
            }
        },
        |result| {
            if let Ok((_, true)) = result {
                if handler {
                    m.request_delayed_dispatch();
                } else {
                    m.dispatch(-54);
                }
            }
        },
    );
    result.map(|(outcome, _)| outcome)
}

/// Slot `+0xb4`.
pub fn send_timeout<M: Env>(
    m: &M,
    entry: EntryDomain,
    id: DataQueueId,
    payload: Payload,
    timeout: Timeout,
) -> Result<SendBlocking, ObjectStatus> {
    let words = id.domain.words();
    let config = words.data_queue_config(id.slot);
    let control = words.data_queue_control(id.slot);
    let ring = Ring::new(config, control);
    let lock = words.context(id.domain).object_lock();
    let result = m.with_object_lock_after(
        lock,
        || -> Result<(SendBlocking, bool, bool), ObjectStatus> {
            if let Some(local) = handoff_receiver(m, control, payload) {
                return Ok((SendBlocking::Ready(WaitOutcome::Satisfied(())), local, false));
            }
            if ring.append(m, payload) {
                return Ok((SendBlocking::Ready(WaitOutcome::Satisfied(())), false, false));
            }
            if timeout == Timeout::Poll {
                return Ok((SendBlocking::Ready(WaitOutcome::TimedOut), false, false));
            }
            let (task, wait, slot) = task_wait(m, entry)?;
            m.store_word(wait.data(), payload.0);
            let blocked = block(
                m,
                entry.domain(),
                id.domain,
                task,
                wait,
                control,
                control.sender_queue(),
                QueueOrder::from_attribute(m.load_word(config.attributes())),
                SEND_STATE,
                timer_for(m, entry.domain(), slot, timeout),
            );
            Ok((
                SendBlocking::Pending(SendPending { task, wait }),
                false,
                !blocked,
            ))
        },
        |result| {
            if let Ok((_, true, _)) = result {
                m.dispatch(-55);
            }
        },
    );
    match result {
        Ok((outcome, _, false)) => Ok(outcome),
        Ok((_, _, true)) => Ok(SendBlocking::Retry),
        Err(status) => Err(status),
    }
}

/// Slot `+0xb8` and `+0xbc`.
pub fn force_send<M: Env>(
    m: &M,
    id: DataQueueId,
    payload: Payload,
    handler: bool,
) -> Result<(), ObjectStatus> {
    let words = id.domain.words();
    let config = words.data_queue_config(id.slot);
    let control = words.data_queue_control(id.slot);
    let ring = Ring::new(config, control);
    if ring.capacity(m).0 == 0 {
        return Err(ObjectStatus::Capacity);
    }
    let lock = words.context(id.domain).object_lock();
    m.with_object_lock_after(
        lock,
        || {
            if let Some(local) = handoff_receiver(m, control, payload) {
                return local;
            }
            ring.force_append(m, payload);
            false
        },
        |dispatch| {
            if *dispatch {
                if handler {
                    m.request_delayed_dispatch();
                } else {
                    m.dispatch(-56);
                }
            }
        },
    );
    Ok(())
}

/// Slots `+0xc0`, `+0xc4` and `+0xc8`.
pub fn receive<M: Env>(
    m: &M,
    entry: EntryDomain,
    id: DataQueueId,
    out: OutWord,
    timeout: Timeout,
) -> Result<ReceiveBlocking, ObjectStatus> {
    let words = id.domain.words();
    let config = words.data_queue_config(id.slot);
    let control = words.data_queue_control(id.slot);
    let ring = Ring::new(config, control);
    let lock = words.context(id.domain).object_lock();
    let result = m.with_object_lock_after(
        lock,
        || -> Result<(ReceiveBlocking, bool, bool), ObjectStatus> {
            if let Some((received, local)) = take_sender(m, control, ring, out) {
                return Ok((
                    ReceiveBlocking::Ready(WaitOutcome::Satisfied(received)),
                    local,
                    false,
                ));
            }
            if ring.count(m).0 != 0 {
                return Ok((
                    ReceiveBlocking::Ready(WaitOutcome::Satisfied(ring.pop(m, out))),
                    false,
                    false,
                ));
            }
            if timeout == Timeout::Poll {
                return Ok((ReceiveBlocking::Ready(WaitOutcome::TimedOut), false, false));
            }
            let (task, wait, slot) = task_wait(m, entry)?;
            let blocked = block(
                m,
                entry.domain(),
                id.domain,
                task,
                wait,
                control,
                WaitQueue(control.receivers().0),
                QueueOrder::Fifo,
                RECEIVE_STATE,
                timer_for(m, entry.domain(), slot, timeout),
            );
            Ok((
                ReceiveBlocking::Pending(ReceivePending { task, wait, out }),
                false,
                !blocked,
            ))
        },
        |result| {
            if let Ok((_, true, _)) = result {
                if timeout == Timeout::Poll {
                    m.dispatch(-58);
                } else {
                    m.dispatch(-57);
                }
            }
        },
    );
    match result {
        Ok((outcome, _, false)) => Ok(outcome),
        Ok((_, _, true)) => Ok(ReceiveBlocking::Retry),
        Err(status) => Err(status),
    }
}

pub fn resume_send<M: Env>(m: &M, pending: SendPending) -> SendResume {
    if m.load_byte(pending.task.state()) & TaskState::WAIT.bits() != 0 {
        return SendResume::Pending(pending);
    }
    match m.load_word(pending.wait.base().result()) as i32 {
        0 => SendResume::Completed(WaitOutcome::Satisfied(())),
        -49 => SendResume::Completed(WaitOutcome::Released),
        -50 => SendResume::Completed(WaitOutcome::TimedOut),
        _ => SendResume::Corrupt(pending),
    }
}

pub fn resume_receive<M: Env>(m: &M, pending: ReceivePending) -> ReceiveResume {
    if m.load_byte(pending.task.state()) & TaskState::WAIT.bits() != 0 {
        return ReceiveResume::Pending(pending);
    }
    match m.load_word(pending.wait.base().result()) as i32 {
        0 => {
            let payload = Payload(m.load_word(pending.wait.data()));
            let saved = m.mask_i();
            m.store_word(pending.out.0, payload.0);
            m.restore_i(saved);
            ReceiveResume::Completed(WaitOutcome::Satisfied(payload))
        }
        -49 => ReceiveResume::Completed(WaitOutcome::Released),
        -50 => ReceiveResume::Completed(WaitOutcome::TimedOut),
        _ => ReceiveResume::Corrupt(pending),
    }
}

use crate::create::{Machine, SavedInterrupts};
use crate::id::{FlagId, MailboxId};
use crate::layout::{FlagConfig, FlagControl, GuestAddress, LockRecord, Message};
use crate::matching::{check, KeepMask, MatchMode, Pattern, WantedMask};
use crate::status::{ObjectStatus, WaitOutcome};

/// A caller-supplied word the body writes; stock does not require it disjoint from the record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct OutWord(pub GuestAddress);

/// Releases and restores on drop.
pub struct ObjectGuard<'a, M: ObjectLock + ?Sized> {
    machine: &'a M,
    lock: LockRecord,
    saved: SavedInterrupts,
    released: bool,
}

impl<'a, M: ObjectLock + ?Sized> ObjectGuard<'a, M> {
    pub(crate) fn new(machine: &'a M, lock: LockRecord, saved: SavedInterrupts) -> Self {
        Self { machine, lock, saved, released: false }
    }

    /// The `+0x1c` release; idempotent.
    pub fn release(&mut self) {
        if !self.released {
            self.machine.release_object(self.lock);
            self.released = true;
        }
    }
}

impl<M: ObjectLock + ?Sized> Drop for ObjectGuard<'_, M> {
    fn drop(&mut self) {
        self.release();
        self.machine.restore_interrupts(self.saved);
    }
}

/// `+0x14` through `+0x18`/`+0x1c`, masked I-only.
pub trait ObjectLock: Machine {
    /// Masks I.
    fn lock_object(&self, lock: LockRecord) -> ObjectGuard<'_, Self>
    where
        Self: Sized;
    /// The `+0x1c` release; the guard calls it once.
    fn release_object(&self, lock: LockRecord);
}

/// Slot `+0x90`.
#[cfg_attr(feature = "family", inline(never))]
pub fn clear_flag<M: ObjectLock>(m: &M, id: FlagId, keep: KeepMask) -> Result<(), ObjectStatus> {
    let words = id.domain.words();
    let pattern = words.flag_control(id.slot).pattern();
    let lock = words.context(id.domain).object_lock();
    let _guard = m.lock_object(lock);
    m.store_word(pattern, m.load_word(pattern) & keep.0);
    Ok(())
}

/// The caller-output store order relative to the clear-on-match.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StoreOrder {
    ClearThenOut,
    OutThenClear,
}

/// The immediate match, clear and caller store under the object lock.
#[cfg_attr(feature = "family", inline(never))]
pub fn check_control<M: Machine>(
    m: &M,
    control: FlagControl,
    config: FlagConfig,
    want: Pattern,
    mode: MatchMode,
    out: OutWord,
    order: StoreOrder,
) -> Option<Pattern> {
    let mut pattern = Pattern(m.load_word(control.pattern()));
    let clear_on_match = m.load_word(config.attributes()) & FlagConfig::CLEAR_ON_MATCH != 0;
    match check(&mut pattern, want, mode, clear_on_match) {
        Some(reported) => {
            match order {
                StoreOrder::ClearThenOut => {
                    if clear_on_match {
                        m.store_word(control.pattern(), 0);
                    }
                    m.store_word(out.0, reported.0);
                }
                StoreOrder::OutThenClear => {
                    m.store_word(out.0, reported.0);
                    if clear_on_match {
                        m.store_word(control.pattern(), 0);
                    }
                }
            }
            Some(reported)
        }
        None => None,
    }
}

/// Slot `+0x98`.
#[cfg_attr(feature = "family", inline(never))]
pub fn poll_flag<M: ObjectLock>(
    m: &M,
    id: FlagId,
    want: WantedMask,
    mode: MatchMode,
    out: OutWord,
) -> Result<WaitOutcome<Pattern>, ObjectStatus> {
    let words = id.domain.words();
    let control = words.flag_control(id.slot);
    let config = words.flag_config(id.slot);
    let lock = words.context(id.domain).object_lock();
    let _guard = m.lock_object(lock);
    (|| {
        if m.load_word(control.queue().next()) != control.0 .0 {
            return Err(ObjectStatus::QueueOverflow);
        }
        match check_control(m, control, config, want.pattern(), mode, out, StoreOrder::OutThenClear) {
            Some(reported) => Ok(WaitOutcome::Satisfied(reported)),
            None => Ok(WaitOutcome::TimedOut),
        }
    })()
}

/// Slot `+0xdc`.
#[cfg_attr(feature = "family", inline(never))]
pub fn poll_receive<M: ObjectLock>(
    m: &M,
    id: MailboxId,
    out: OutWord,
) -> Result<WaitOutcome<Message>, ObjectStatus> {
    let words = id.domain.words();
    let control = words.mailbox_control(id.slot);
    let lock = words.context(id.domain).object_lock();
    let _guard = m.lock_object(lock);
    (|| {
        let head = m.load_word(control.head());
        if head == 0 {
            return Ok(WaitOutcome::TimedOut);
        }
        let message = Message(GuestAddress(head));
        let successor = m.load_word(message.next());
        m.store_word(out.0, head);
        m.store_word(control.head(), successor);
        Ok(WaitOutcome::Satisfied(message))
    })()
}

use crate::create::Machine;
use crate::id::{FlagId, MailboxId};
use crate::layout::{FlagConfig, GuestAddress, LockRecord, Message};
use crate::matching::{check, KeepMask, MatchMode, Pattern};
use crate::status::{ObjectStatus, WaitOutcome};

/// The object lock is held; `release` consumes it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Held(());

impl Held {
    pub const fn held() -> Self {
        Self(())
    }
}

/// The object's domain record lock at `+0x14`, acquired through `+0x18` and released through
/// `+0x1c`.
pub trait ObjectLock: Machine {
    fn disable_irq(&self);
    fn enable_irq(&self);
    fn acquire(&self, lock: LockRecord) -> Held;
    fn release(&self, lock: LockRecord, held: Held);
}

/// Slot `+0x90`.
pub fn clear_flag<M: ObjectLock>(m: &M, id: FlagId, keep: KeepMask) -> Result<(), ObjectStatus> {
    let words = id.domain.words();
    let pattern = words.flag_control(id.slot).pattern();
    let lock = words.context(id.domain).object_lock();
    m.disable_irq();
    let held = m.acquire(lock);
    m.store_word(pattern, m.load_word(pattern) & keep.0);
    m.release(lock, held);
    m.enable_irq();
    Ok(())
}

/// Slot `+0x98`.
pub fn poll_flag<M: ObjectLock>(
    m: &M,
    id: FlagId,
    want: Pattern,
    mode: MatchMode,
    out: &mut Pattern,
) -> Result<WaitOutcome<Pattern>, ObjectStatus> {
    let words = id.domain.words();
    let control = words.flag_control(id.slot);
    let config = words.flag_config(id.slot);
    let lock = words.context(id.domain).object_lock();
    m.disable_irq();
    let held = m.acquire(lock);
    let result = if m.load_word(control.queue().next()) != control.0 .0 {
        Err(ObjectStatus::QueueOverflow)
    } else {
        let mut pattern = Pattern(m.load_word(control.pattern()));
        match check(&mut pattern, want, mode, false) {
            Some(reported) => {
                *out = reported;
                if m.load_word(config.attributes()) & FlagConfig::CLEAR_ON_MATCH != 0 {
                    m.store_word(control.pattern(), 0);
                }
                Ok(WaitOutcome::Satisfied(reported))
            }
            None => Ok(WaitOutcome::TimedOut),
        }
    };
    m.release(lock, held);
    m.enable_irq();
    result
}

/// Slot `+0xdc`.
pub fn poll_receive<M: ObjectLock>(
    m: &M,
    id: MailboxId,
    out: &mut Option<Message>,
) -> Result<WaitOutcome<Message>, ObjectStatus> {
    let words = id.domain.words();
    let control = words.mailbox_control(id.slot);
    let lock = words.context(id.domain).object_lock();
    m.disable_irq();
    let held = m.acquire(lock);
    let result = match m.load_word(control.head()) {
        0 => Ok(WaitOutcome::TimedOut),
        head => {
            let message = Message(GuestAddress(head));
            *out = Some(message);
            m.store_word(control.head(), m.load_word(message.next()));
            Ok(WaitOutcome::Satisfied(message))
        }
    };
    m.release(lock, held);
    m.enable_irq();
    result
}

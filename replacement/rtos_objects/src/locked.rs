use crate::create::Machine;
use crate::id::{FlagId, MailboxId};
use crate::layout::{FlagConfig, FlagControl, GuestAddress, LockRecord, Message};
use crate::matching::{check, KeepMask, MatchMode, Pattern, WantedMask};
use crate::status::{ObjectStatus, WaitOutcome};

/// A caller-supplied word the body writes; stock does not require it disjoint from the record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct OutWord(pub GuestAddress);

/// The object domain's `+0x14` lock through `+0x18`/`+0x1c`, with the I-only mask; the
/// implementation owns the pairing.
pub trait ObjectLock: Machine {
    fn with_object_lock<R>(&self, lock: LockRecord, body: impl FnOnce() -> R) -> R;
    /// As 'with_object_lock', with `after` run between the release and the restore, so a
    /// dispatch stays inside the masked section.
    fn with_object_lock_after<R>(&self, lock: LockRecord, body: impl FnOnce() -> R, after: impl FnOnce(&R)) -> R;
}

/// Slot `+0x90`.
#[cfg_attr(feature = "family", inline(never))]
pub fn clear_flag<M: ObjectLock>(m: &M, id: FlagId, keep: KeepMask) -> Result<(), ObjectStatus> {
    let words = id.domain.words();
    let pattern = words.flag_control(id.slot).pattern();
    let lock = words.context(id.domain).object_lock();
    m.with_object_lock(lock, || m.store_word(pattern, m.load_word(pattern) & keep.0));
    Ok(())
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
) -> Option<Pattern> {
    let mut pattern = Pattern(m.load_word(control.pattern()));
    let clear_on_match = m.load_word(config.attributes()) & FlagConfig::CLEAR_ON_MATCH != 0;
    match check(&mut pattern, want, mode, clear_on_match) {
        Some(reported) => {
            if clear_on_match {
                m.store_word(control.pattern(), 0);
            }
            m.store_word(out.0, reported.0);
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
    m.with_object_lock(lock, || {
        if m.load_word(control.queue().next()) != control.0 .0 {
            return Err(ObjectStatus::QueueOverflow);
        }
        match check_control(m, control, config, want.pattern(), mode, out) {
            Some(reported) => Ok(WaitOutcome::Satisfied(reported)),
            None => Ok(WaitOutcome::TimedOut),
        }
    })
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
    m.with_object_lock(lock, || {
        let head = m.load_word(control.head());
        if head == 0 {
            return Ok(WaitOutcome::TimedOut);
        }
        let message = Message(GuestAddress(head));
        let successor = m.load_word(message.next());
        m.store_word(out.0, head);
        m.store_word(control.head(), successor);
        Ok(WaitOutcome::Satisfied(message))
    })
}

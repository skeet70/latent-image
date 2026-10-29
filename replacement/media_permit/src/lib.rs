//! X-M1 1.50 `Reccf_SvPermitMediaWrite`, message 0x1367: `FUN_00e77168`
//! resolution and `FUN_00e7e95c` effects over the retained frame services.
#![allow(unexpected_cfgs)]

use core::cmp::Ordering;

/// `FUN_00e77168`'s slot bound: slots `0..=200`.
pub const SLOT_BOUND: u32 = 0xc9;

pub const MEDIA_WRITE_EVENT: EventBit = EventBit(0xbe);

#[cfg(media_permit_defect)]
const APPLIED_EVENT: EventBit = EventBit(0xbf);
#[cfg(not(media_permit_defect))]
const APPLIED_EVENT: EventBit = MEDIA_WRITE_EVENT;

/// The cancel-wait event signalled for kind 2 and 3 frames.
pub const CANCEL_WAIT_EVENT: EventId = EventId(5);

/// Message id posted by `FUN_00e67750`.
pub const MEDIA_WRITE_MESSAGE: MessageId = MessageId(0x1367);

/// `FUN_00e67e38`'s default class for an unlisted message id.
pub const PERMIT_MESSAGE_CLASS: MessageClass = MessageClass(10);

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FrameKey(pub u32);

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SlotIndex(u8);

impl SlotIndex {
    pub const FIRST: Self = SlotIndex(0);
    pub const LAST: Self = SlotIndex((SLOT_BOUND - 1) as u8);

    pub const fn from_word(word: u32) -> Option<Self> {
        if word < SLOT_BOUND {
            Some(SlotIndex(word as u8))
        } else {
            None
        }
    }

    pub const fn word(self) -> u32 {
        self.0 as u32
    }

    pub const fn next(self) -> Option<Self> {
        Self::from_word(self.word() + 1)
    }
}

/// The layout selector read from `0x0157dd34` / `0x0157dd35`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KeyLayout {
    Primary,
    Secondary,
    KindSelected,
}

impl KeyLayout {
    pub const fn from_selectors(primary: u8, secondary: u8) -> Self {
        if primary == 1 {
            KeyLayout::Primary
        } else if secondary == 1 {
            KeyLayout::Secondary
        } else {
            KeyLayout::KindSelected
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KeyField {
    Word0,
    Word2,
    WordMinus1,
}

impl KeyField {
    pub const fn for_kind(kind: FrameKind) -> Self {
        match kind {
            FrameKind::Zero | FrameKind::Two => KeyField::Word0,
            FrameKind::One => KeyField::Word2,
            FrameKind::Three | FrameKind::Four => KeyField::WordMinus1,
        }
    }
}

/// `FUN_00e75490`'s frame kind.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Zero,
    One,
    Two,
    Three,
    Four,
}

impl FrameKind {
    pub const fn from_word(word: u32) -> Option<Self> {
        match word {
            0 => Some(FrameKind::Zero),
            1 => Some(FrameKind::One),
            2 => Some(FrameKind::Two),
            3 => Some(FrameKind::Three),
            4 => Some(FrameKind::Four),
            _ => None,
        }
    }

    pub const fn word(self) -> u32 {
        self as u32
    }
}

/// A bit index in a per-frame bitmap.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EventBit(pub u8);

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EventId(pub u32);

/// A frame command word sent through `FUN_00e8d82c`.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FrameCommand(pub u16);

impl FrameCommand {
    pub const ZERO: Self = FrameCommand(0);
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MessageId(pub u16);

/// An event class handed to `FUN_00e8d1ec`.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MessageClass(pub u32);

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PermitWord(pub u32);

impl PermitWord {
    pub const ZERO: Self = PermitWord(0);

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PermitMessage {
    pub word0: PermitWord,
    pub word1: PermitWord,
}

impl PermitMessage {
    pub const fn new(word0: PermitWord, word1: PermitWord) -> Self {
        PermitMessage { word0, word1 }
    }
}

/// The mirrored message as the handler reads it (`FUN_00e88e1c(0, ..)`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MirroredMessage {
    pub message_id: MessageId,
    pub word0: PermitWord,
    pub word1: PermitWord,
}

/// A subframe count read from `FUN_00e74c68`.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SubframeCount(pub u32);

/// A subframe index passed to `FUN_00e886ec`.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SubframeIndex(pub u8);

impl SubframeIndex {
    pub const fn from_index(index: u32) -> Self {
        SubframeIndex((index & 0xff) as u8)
    }
}

/// A frame count read from `FUN_00e87114`.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FrameCount(pub u32);

/// An index passed to `FUN_00e88804`.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FrameIndex(pub u32);

/// One of the three aggregate minimum words.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MinimumWord(pub u32);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ResolveError {
    UnknownKey,
    UnknownKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PermitStatus {
    Applied(SlotIndex),
    UnresolvedKey,
    Refused,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ReceiveStatus {
    Handled { slot: Option<SlotIndex> },
}

/// The retained frame table and layout selectors.
pub trait SlotSource {
    fn layout(&self) -> KeyLayout;
    fn slot_key(&self, slot: SlotIndex, layout: KeyLayout, field: KeyField) -> FrameKey;
    fn frame_kind(&self, slot: SlotIndex) -> Option<FrameKind>;
}

/// The retained message, bitmap, frame and command services.
pub trait PermitRuntime: SlotSource {
    fn write_mirror(&mut self, message: &PermitMessage);
    fn log_message(&mut self);
    fn mirror(&self) -> MirroredMessage;
    fn report_unresolved_key(&mut self);
    fn set_event_bit(&mut self, slot: SlotIndex, bit: EventBit);
    fn clear_capture_word(&mut self, slot: SlotIndex);
    fn store_capture_word(&mut self, slot: SlotIndex, word: PermitWord);
    fn capture_word_is_one(&self, slot: SlotIndex) -> bool;
    fn subframe_count(&self, slot: SlotIndex) -> SubframeCount;
    fn subframe(&self, slot: SlotIndex) -> Option<SlotIndex>;
    fn mapped_subframe(&self, slot: SlotIndex, index: SubframeIndex) -> Option<SlotIndex>;
    fn frames_of_kind(&self, kind: FrameKind) -> FrameCount;
    fn frame_of_kind(&self, index: FrameIndex, kind: FrameKind) -> Option<SlotIndex>;
    fn send_command(&mut self, slot: SlotIndex, command: FrameCommand);
    fn signal_cancel(&mut self, slot: SlotIndex, event: EventId);
    fn minimum(&self) -> MinimumWord;
    fn set_minimum(&mut self, word: MinimumWord);
    fn set_active_slot(&mut self, slot: SlotIndex);
    fn publish_resolution(&mut self, slot: Option<SlotIndex>);
    fn hand_back(&mut self, class: MessageClass);
}

/// The bounded key scan shared by all three layouts.
pub fn resolve(
    source: &impl SlotSource,
    layout: KeyLayout,
    key: FrameKey,
) -> Result<SlotIndex, ResolveError> {
    let mut slot = SlotIndex::FIRST;
    loop {
        let field = match layout {
            KeyLayout::KindSelected => match source.frame_kind(slot) {
                Some(kind) => KeyField::for_kind(kind),
                None => return Err(ResolveError::UnknownKind),
            },
            KeyLayout::Primary | KeyLayout::Secondary => KeyField::Word0,
        };
        if source.slot_key(slot, layout, field) == key {
            return Ok(slot);
        }
        match slot.next() {
            Some(next) => slot = next,
            None => return Err(ResolveError::UnknownKey),
        }
    }
}

fn apply_kind_one(runtime: &mut impl PermitRuntime, slot: SlotIndex, word0: PermitWord) -> bool {
    let count = runtime.subframe_count(slot);
    let mut index = 0u32;
    while index < count.0 {
        let sub = match runtime.subframe(slot) {
            Some(sub) => sub,
            None => return false,
        };
        let mapped = match runtime.mapped_subframe(sub, SubframeIndex::from_index(index)) {
            Some(mapped) => mapped,
            None => return false,
        };
        runtime.set_event_bit(mapped, APPLIED_EVENT);
        index += 1;
    }

    let mut last = slot;
    index = 0;
    while index < count.0 {
        let sub = match runtime.subframe(slot) {
            Some(sub) => sub,
            None => return false,
        };
        let mapped = match runtime.mapped_subframe(sub, SubframeIndex::from_index(index)) {
            Some(mapped) => mapped,
            None => return false,
        };
        runtime.store_capture_word(mapped, word0);
        last = mapped;
        index += 1;
    }

    let minimum = runtime.minimum();
    if runtime.capture_word_is_one(last)
        && word0.0.cmp(&count.0) == Ordering::Greater
        && word0.0 < minimum.0
    {
        runtime.set_minimum(word0);
    }

    index = 0;
    while index < count.0 {
        let sub = match runtime.subframe(slot) {
            Some(sub) => sub,
            None => return false,
        };
        let mapped = match runtime.mapped_subframe(sub, SubframeIndex::from_index(index)) {
            Some(mapped) => mapped,
            None => return false,
        };
        runtime.set_active_slot(mapped);
        runtime.send_command(mapped, FrameCommand::ZERO);
        index += 1;
    }
    true
}

fn apply_kind_broadcast(
    runtime: &mut impl PermitRuntime,
    kind: FrameKind,
    word0: PermitWord,
) -> bool {
    let count = runtime.frames_of_kind(kind);
    let mut index = 0u32;
    while index < count.0 {
        let frame = match runtime.frame_of_kind(FrameIndex(index), kind) {
            Some(frame) => frame,
            None => return false,
        };
        runtime.signal_cancel(frame, CANCEL_WAIT_EVENT);
        runtime.set_event_bit(frame, APPLIED_EVENT);
        if word0.is_zero() {
            runtime.clear_capture_word(frame);
        }
        index += 1;
    }
    true
}

/// `FUN_00e7e95c`: apply the mirrored permit to the frame named by word 1.
pub fn apply_mirrored_permit(runtime: &mut impl PermitRuntime) -> PermitStatus {
    let mirrored = runtime.mirror();
    let slot = match resolve(runtime, runtime.layout(), mirrored.word1) {
        Ok(slot) => slot,
        Err(ResolveError::UnknownKey) => {
            runtime.report_unresolved_key();
            return PermitStatus::UnresolvedKey;
        }
        Err(ResolveError::UnknownKind) => return PermitStatus::Refused,
    };

    let kind = match runtime.frame_kind(slot) {
        Some(kind) => kind,
        None => return PermitStatus::Refused,
    };

    let applied = match kind {
        FrameKind::Zero => {
            runtime.set_active_slot(slot);
            runtime.set_event_bit(slot, APPLIED_EVENT);
            if mirrored.word0.is_zero() {
                runtime.clear_capture_word(slot);
            }
            runtime.send_command(slot, FrameCommand::ZERO);
            true
        }
        FrameKind::One => apply_kind_one(runtime, slot, mirrored.word0),
        FrameKind::Two | FrameKind::Three => {
            apply_kind_broadcast(runtime, kind, mirrored.word0)
        }
        FrameKind::Four => true,
    };

    if applied {
        PermitStatus::Applied(slot)
    } else {
        PermitStatus::Refused
    }
}

/// `FUN_00e8659c` row 0x1367.
pub fn receive_permit(
    runtime: &mut impl PermitRuntime,
    message: &PermitMessage,
) -> ReceiveStatus {
    runtime.write_mirror(message);
    runtime.log_message();
    let mirrored = runtime.mirror();
    let slot = match resolve(runtime, runtime.layout(), mirrored.word1) {
        Ok(slot) => Ok(slot),
        Err(ResolveError::UnknownKey) => {
            runtime.report_unresolved_key();
            Err(())
        }
        Err(ResolveError::UnknownKind) => Err(()),
    };
    runtime.publish_resolution(slot.ok());
    let slot = match slot {
        Ok(slot) => {
            runtime.set_event_bit(slot, APPLIED_EVENT);
            Some(slot)
        }
        Err(()) => None,
    };
    runtime.hand_back(PERMIT_MESSAGE_CLASS);
    ReceiveStatus::Handled { slot }
}

/// `FUN_00e67750`: post the 8-byte permit message and dispatch it.
pub fn send_permit(runtime: &mut impl PermitRuntime, message: &PermitMessage) -> ReceiveStatus {
    runtime.log_message();
    receive_permit(runtime, message)
}

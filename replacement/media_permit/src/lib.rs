//! X-M1 1.50 `Reccf_SvPermitMediaWrite`, message 0x1367: `FUN_00e77168`
//! resolution and `FUN_00e7e95c` effects over the retained frame services.
#![allow(unexpected_cfgs)]

use core::cmp::Ordering;

/// `FUN_00e77168`'s slot bound: slots `0..=200`.
pub const SLOT_BOUND: u32 = 0xc9;

#[cfg(media_permit_defect)]
const APPLIED_EVENT: EventBit = EventBit(0xbf);
#[cfg(not(media_permit_defect))]
const APPLIED_EVENT: EventBit = EventBit(0xbe);

/// The cancel-wait event signalled for kind 2 and 3 frames.
pub const CANCEL_WAIT_EVENT: EventId = EventId(5);

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FrameKey(pub u32);

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SlotIndex(u8);

impl SlotIndex {
    pub const FIRST: Self = SlotIndex(0);

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

/// The retained `FUN_00e87ad4(slot)` result, `word[0x0157dd3c] + slot * 0x326c`,
/// captured at the stock use point.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FrameContext(pub(crate) usize);

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PermitWord(pub u32);

impl PermitWord {
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }
}

/// The mirrored message as the handler reads it (`FUN_00e88e1c(0, ..)`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MirroredMessage {
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
    /// e75490's source output is 0..4; another word is outside that domain.
    UnknownKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PermitStatus {
    Applied(SlotIndex),
    UnresolvedKey,
    /// A retained service returned outside its admitted domain.
    Refused,
}

/// The retained frame table and layout selectors.
pub trait SlotSource {
    fn layout(&self) -> KeyLayout;
    fn slot_key(&self, slot: SlotIndex, layout: KeyLayout, field: KeyField) -> FrameKey;
    fn frame_kind(&self, slot: SlotIndex) -> Option<FrameKind>;
}

/// The retained mirror, bitmap, frame and command services.
pub trait PermitRuntime: SlotSource {
    fn mirror(&self) -> MirroredMessage;
    fn report_unresolved_key(&mut self);
    fn set_event_bit(&mut self, slot: SlotIndex, bit: EventBit);
    fn frame_context(&self, slot: SlotIndex) -> Option<FrameContext>;
    fn clear_capture_word(&mut self, context: FrameContext);
    fn store_permit_word(&mut self, context: FrameContext, word: PermitWord);
    fn kind_flag_is_one(&self, context: FrameContext) -> bool;
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

fn apply_kind_one(
    runtime: &mut impl PermitRuntime,
    slot: SlotIndex,
    initial: FrameContext,
    word0: PermitWord,
) -> bool {
    let mut index = 0u32;
    while index < runtime.subframe_count(slot).0 {
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

    let mut last = initial;
    index = 0;
    while index < runtime.subframe_count(slot).0 {
        let sub = match runtime.subframe(slot) {
            Some(sub) => sub,
            None => return false,
        };
        let mapped = match runtime.mapped_subframe(sub, SubframeIndex::from_index(index)) {
            Some(mapped) => mapped,
            None => return false,
        };
        let context = match runtime.frame_context(mapped) {
            Some(context) => context,
            None => return false,
        };
        runtime.store_permit_word(context, word0);
        last = context;
        index += 1;
    }

    let count = runtime.subframe_count(slot);
    let minimum = runtime.minimum();
    if runtime.kind_flag_is_one(last)
        && word0.0.cmp(&count.0) == Ordering::Greater
        && word0.0 < minimum.0
    {
        runtime.set_minimum(MinimumWord(word0.0));
    }

    index = 0;
    while index < runtime.subframe_count(slot).0 {
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
    let mut index = 0u32;
    while index < runtime.frames_of_kind(kind).0 {
        let frame = match runtime.frame_of_kind(FrameIndex(index), kind) {
            Some(frame) => frame,
            None => return false,
        };
        runtime.signal_cancel(frame, CANCEL_WAIT_EVENT);
        runtime.set_event_bit(frame, APPLIED_EVENT);
        let context = match runtime.frame_context(frame) {
            Some(context) => context,
            None => return false,
        };
        if word0.is_zero() {
            runtime.clear_capture_word(context);
        }
        index += 1;
    }
    true
}

/// `FUN_00e7e95c`: apply the mirrored permit to the frame named by word 1.
/// Entered only by thunks `0x00e6a74c` (machine 5 action 10, class 5 in
/// states 1 and 2) and `0x00e6bb88` (machine 6 action 22, class 14 in
/// state 1); message 0x1367 is the only message mapped to either class.
pub fn apply_mirrored_permit(runtime: &mut impl PermitRuntime) -> PermitStatus {
    let mirrored = runtime.mirror();
    let slot = match resolve(runtime, runtime.layout(), FrameKey(mirrored.word1.0)) {
        Ok(slot) => slot,
        Err(ResolveError::UnknownKey) => {
            runtime.report_unresolved_key();
            return PermitStatus::UnresolvedKey;
        }
        Err(ResolveError::UnknownKind) => return PermitStatus::Refused,
    };

    let context = match runtime.frame_context(slot) {
        Some(context) => context,
        None => return PermitStatus::Refused,
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
                runtime.clear_capture_word(context);
            }
            runtime.send_command(slot, FrameCommand::ZERO);
            true
        }
        FrameKind::One => apply_kind_one(runtime, slot, context, mirrored.word0),
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



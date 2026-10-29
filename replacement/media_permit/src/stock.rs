//! ABI adapter from the owned permit behavior to the retained X-M1 1.50
//! message, frame-table, bitmap and command services.

use crate::media_permit::{
    apply_mirrored_permit, receive_permit, FrameCommand, FrameCount, FrameIndex, FrameKey,
    FrameKind, KeyField, KeyLayout, MessageClass, MessageId, MirroredMessage, MinimumWord,
    PermitMessage, PermitRuntime, PermitStatus, PermitWord, ReceiveStatus, SlotIndex, SlotSource,
    SubframeCount, SubframeIndex, MEDIA_WRITE_MESSAGE,
};

const LAYOUT_PRIMARY: usize = 0x0157_dd34;
const LAYOUT_SECONDARY: usize = 0x0157_dd35;
const CONTEXT_BASE: usize = 0x0157_dd3c;
const SLOT_KEY_PRIMARY: usize = 0x0159_6800;
const SLOT_KEY_SECONDARY: usize = 0x0159_67fc;
const SLOT_KEY_KIND: usize = 0x0159_6804;
const SLOT_KEY_STRIDE: isize = 0x1d0;
const CONTEXT_STRIDE: usize = 0x326c;
const CAPTURE_WORD: usize = 0x27c8;
const PERMIT_WORD: usize = 0x2fb0;
const KIND_FLAG: usize = 0x4a8;
const BITMAP_BASE: usize = 0x0159_6918;
const BITMAP_STRIDE: usize = 0x1d0;
const ACTIVE_SLOT: usize = 0x015a_d53c;
const MINIMA: usize = 0x0158_1f60;
const MIRROR_BYTES: usize = 0x10c;
const MIRROR_WORD0: usize = 0x0c;
const MIRROR_WORD1: usize = 0x10;
const PAYLOAD_BYTES: u32 = 8;

#[repr(C)]
struct MessageWire {
    message_id: u16,
    source_a: u8,
    source_b: u8,
    length: u32,
    payload: *const PermitMessage,
}

unsafe extern "C" {
    fn rb_mp_stock_mirror_write(message: *const MessageWire);
    fn rb_mp_stock_mirror_read(source: u32, out: *mut u8);
    fn rb_mp_stock_task_byte() -> u32;
    fn rb_mp_stock_log(task: u32, name: *const u8);
    fn rb_mp_stock_frame_kind(slot: u32) -> u32;
    fn rb_mp_stock_bit_set(base: usize, bit: u32);
    fn rb_mp_stock_context(slot: u32) -> usize;
    fn rb_mp_stock_subframe_count(slot: u32) -> u32;
    fn rb_mp_stock_subframe(slot: u32) -> i32;
    fn rb_mp_stock_map_subframe(slot: u32, index: u32, mode: u32) -> i32;
    fn rb_mp_stock_count_kind(kind: u32) -> u32;
    fn rb_mp_stock_frame_of_kind(index: u32, kind: u32) -> i32;
    fn rb_mp_stock_command(slot: u32, command: u32);
    fn rb_mp_stock_cancel(slot: u32, event: u32);
    fn rb_mp_stock_hand_back(class: u32);
    fn rb_mp_stock_fatal();
}

fn read_u8(address: usize) -> u8 {
    unsafe { core::ptr::read_volatile(address as *const u8) }
}

fn read_u32(address: usize) -> u32 {
    unsafe { core::ptr::read_volatile(address as *const u32) }
}

fn write_u32(address: usize, value: u32) {
    unsafe { core::ptr::write_volatile(address as *mut u32, value) }
}

fn context(slot: SlotIndex) -> Option<usize> {
    let base = read_u32(CONTEXT_BASE) as usize;
    if base == 0 {
        return None;
    }
    Some(base + slot.word() as usize * CONTEXT_STRIDE)
}

struct StockSlots;

impl SlotSource for StockSlots {
    fn layout(&self) -> KeyLayout {
        KeyLayout::from_selectors(read_u8(LAYOUT_PRIMARY), read_u8(LAYOUT_SECONDARY))
    }

    fn slot_key(&self, slot: SlotIndex, layout: KeyLayout, field: KeyField) -> FrameKey {
        let base = match layout {
            KeyLayout::Primary => SLOT_KEY_PRIMARY as isize,
            KeyLayout::Secondary => SLOT_KEY_SECONDARY as isize,
            KeyLayout::KindSelected => SLOT_KEY_KIND as isize,
        };
        let field_offset = match field {
            KeyField::Word0 => 0,
            KeyField::Word2 => 8,
            KeyField::WordMinus1 => -4,
        };
        let address = base + slot.word() as isize * SLOT_KEY_STRIDE + field_offset;
        FrameKey(read_u32(address as usize))
    }

    fn frame_kind(&self, slot: SlotIndex) -> Option<FrameKind> {
        FrameKind::from_word(unsafe { rb_mp_stock_frame_kind(slot.word()) })
    }
}

struct StockRuntime;

impl SlotSource for StockRuntime {
    fn layout(&self) -> KeyLayout {
        StockSlots.layout()
    }

    fn slot_key(&self, slot: SlotIndex, layout: KeyLayout, field: KeyField) -> FrameKey {
        StockSlots.slot_key(slot, layout, field)
    }

    fn frame_kind(&self, slot: SlotIndex) -> Option<FrameKind> {
        StockSlots.frame_kind(slot)
    }
}

impl PermitRuntime for StockRuntime {
    fn write_mirror(&mut self, message: &PermitMessage) {
        let payload = *message;
        let source = unsafe { rb_mp_stock_task_byte() } as u8;
        let wire = MessageWire {
            message_id: MEDIA_WRITE_MESSAGE.0,
            source_a: source,
            source_b: source,
            length: PAYLOAD_BYTES,
            payload: &payload,
        };
        unsafe { rb_mp_stock_mirror_write(&wire) };
    }

    fn log_message(&mut self) {
        let task = unsafe { rb_mp_stock_task_byte() };
        unsafe { rb_mp_stock_log(task, core::ptr::null()) };
    }

    fn mirror(&self) -> MirroredMessage {
        let mut storage = core::mem::MaybeUninit::<[u8; MIRROR_BYTES]>::uninit();
        unsafe { rb_mp_stock_mirror_read(0, storage.as_mut_ptr() as *mut u8) };
        let bytes = unsafe { storage.assume_init_ref() };
        MirroredMessage {
            message_id: MessageId(u16::from_le_bytes([bytes[0], bytes[1]])),
            word0: PermitWord(u32::from_le_bytes([
                bytes[MIRROR_WORD0],
                bytes[MIRROR_WORD0 + 1],
                bytes[MIRROR_WORD0 + 2],
                bytes[MIRROR_WORD0 + 3],
            ])),
            word1: PermitWord(u32::from_le_bytes([
                bytes[MIRROR_WORD1],
                bytes[MIRROR_WORD1 + 1],
                bytes[MIRROR_WORD1 + 2],
                bytes[MIRROR_WORD1 + 3],
            ])),
        }
    }

    fn report_unresolved_key(&mut self) {
        unsafe { rb_mp_stock_fatal() };
    }

    fn set_event_bit(&mut self, slot: SlotIndex, bit: crate::media_permit::EventBit) {
        let base = BITMAP_BASE + slot.word() as usize * BITMAP_STRIDE;
        unsafe { rb_mp_stock_bit_set(base, bit.0 as u32) };
    }

    fn clear_capture_word(&mut self, slot: SlotIndex) {
        if let Some(context) = context(slot) {
            write_u32(context + CAPTURE_WORD, 0);
        }
    }

    fn store_capture_word(&mut self, slot: SlotIndex, word: PermitWord) {
        if let Some(context) = context(slot) {
            write_u32(context + PERMIT_WORD, word.0);
        }
    }

    fn capture_word_is_one(&self, slot: SlotIndex) -> bool {
        match context(slot) {
            Some(context) => read_u32(context + KIND_FLAG) == 1,
            None => false,
        }
    }

    fn subframe_count(&self, slot: SlotIndex) -> SubframeCount {
        SubframeCount(unsafe { rb_mp_stock_subframe_count(slot.word()) })
    }

    fn subframe(&self, slot: SlotIndex) -> Option<SlotIndex> {
        let found = unsafe { rb_mp_stock_subframe(slot.word()) };
        if found < 0 {
            None
        } else {
            SlotIndex::from_word(found as u32)
        }
    }

    fn mapped_subframe(&self, slot: SlotIndex, index: SubframeIndex) -> Option<SlotIndex> {
        let found = unsafe { rb_mp_stock_map_subframe(slot.word(), index.0 as u32, 1) };
        if found < 0 {
            None
        } else {
            SlotIndex::from_word(found as u32)
        }
    }

    fn frames_of_kind(&self, kind: FrameKind) -> FrameCount {
        FrameCount(unsafe { rb_mp_stock_count_kind(kind.word()) })
    }

    fn frame_of_kind(&self, index: FrameIndex, kind: FrameKind) -> Option<SlotIndex> {
        let found = unsafe { rb_mp_stock_frame_of_kind(index.0, kind.word()) };
        if found < 0 {
            None
        } else {
            SlotIndex::from_word(found as u32)
        }
    }

    fn send_command(&mut self, slot: SlotIndex, command: FrameCommand) {
        unsafe { rb_mp_stock_command(slot.word(), command.0 as u32) };
    }

    fn signal_cancel(&mut self, slot: SlotIndex, event: crate::media_permit::EventId) {
        unsafe { rb_mp_stock_cancel(slot.word(), event.0) };
    }

    fn minimum(&self) -> MinimumWord {
        MinimumWord(read_u32(MINIMA))
    }

    fn set_minimum(&mut self, word: MinimumWord) {
        write_u32(MINIMA, word.0);
        write_u32(MINIMA + 4, word.0);
        write_u32(MINIMA + 8, word.0);
    }

    fn set_active_slot(&mut self, slot: SlotIndex) {
        write_u32(ACTIVE_SLOT, slot.word());
    }

    fn publish_resolution(&mut self, slot: Option<SlotIndex>) {
        write_u32(ACTIVE_SLOT, match slot {
            Some(slot) => slot.word(),
            None => u32::MAX,
        });
    }

    fn hand_back(&mut self, class: MessageClass) {
        unsafe { rb_mp_stock_hand_back(class.0) };
    }
}

pub(crate) fn resolve_key(key: u32) -> i32 {
    let slots = StockSlots;
    let layout = slots.layout();
    match crate::media_permit::resolve(&slots, layout, FrameKey(key)) {
        Ok(slot) => slot.word() as i32,
        Err(_) => {
            let mut runtime = StockRuntime;
            runtime.report_unresolved_key();
            -1
        }
    }
}

pub(crate) fn apply_mirror() -> u32 {
    let mut runtime = StockRuntime;
    match apply_mirrored_permit(&mut runtime) {
        PermitStatus::Applied(_) => 0,
        PermitStatus::UnresolvedKey => 1,
        PermitStatus::Refused => 2,
    }
}

pub(crate) fn send_words(word0: u32, word1: u32) -> u32 {
    let mut runtime = StockRuntime;
    let message = PermitMessage::new(PermitWord(word0), PermitWord(word1));
    match send_permit(&mut runtime, &message) {
        ReceiveStatus::Handled { .. } => 0,
    }
}

#[no_mangle]
pub extern "C" fn rb_media_permit_resolve(key: u32) -> i32 {
    resolve_key(key)
}

#[no_mangle]
pub extern "C" fn rb_media_permit_apply() -> u32 {
    apply_mirror()
}

#[no_mangle]
pub extern "C" fn rb_media_permit_send(word0: u32, word1: u32) -> u32 {
    send_words(word0, word1)
}

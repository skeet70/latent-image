//! ABI adapter from the owned permit behavior to the retained X-M1 1.50
//! message, frame-table, bitmap and command services.

use crate::media_permit::{
    apply_mirrored_permit, FrameCommand, FrameContext, FrameCount, FrameIndex, FrameKey, FrameKind,
    KeyField, KeyLayout, MirroredMessage, MinimumWord, PermitRuntime, PermitStatus, PermitWord,
    SlotIndex, SlotSource, SubframeCount, SubframeIndex,
};

const LAYOUT_PRIMARY: usize = 0x0157_dd34;
const LAYOUT_SECONDARY: usize = 0x0157_dd35;
const SLOT_KEY_PRIMARY: usize = 0x0159_6800;
const SLOT_KEY_SECONDARY: usize = 0x0159_67fc;
const SLOT_KEY_KIND: usize = 0x0159_6804;
const CAPTURE_WORD: usize = 0x27c8;
const PERMIT_WORD: usize = 0x2fb0;
const KIND_FLAG: usize = 0x4a8;
const BITMAP_BASE: usize = 0x0159_6918;

static SLOT_OFFSETS: [u32; 201] = {
    let mut offsets = [0u32; 201];
    let mut index = 0;
    while index < 201 {
        offsets[index] = (index as u32) * 0x1d0;
        index += 1;
    }
    offsets
};

fn slot_offset(slot: SlotIndex) -> usize {
    // SlotIndex is inside SLOT_OFFSETS.
    unsafe { *SLOT_OFFSETS.get_unchecked(slot.word() as usize) as usize }
}
const ACTIVE_SLOT: usize = 0x015a_d53c;
const MINIMA: usize = 0x0158_1f60;
const MIRROR_BYTES: usize = 0x10c;
const MIRROR_WORD0: usize = 0x0c;
const MIRROR_WORD1: usize = 0x10;

unsafe extern "C" {
    fn rb_mp_stock_mirror_read(source: u32, out: *mut u8);
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

fn live_context(slot: SlotIndex) -> Option<usize> {
    let address = unsafe { rb_mp_stock_context(slot.word()) };
    if address == 0 {
        None
    } else {
        Some(address)
    }
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
        let address = base + slot_offset(slot) as isize + field_offset;
        FrameKey(read_u32(address as usize))
    }

    fn frame_kind(&self, slot: SlotIndex) -> Option<FrameKind> {
        // 0xe7e95c passes the slot in r0; 0xe75490 forwards it to 0xe87ad4.
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
    fn mirror(&self) -> MirroredMessage {
        let mut storage = core::mem::MaybeUninit::<[u8; MIRROR_BYTES]>::uninit();
        unsafe { rb_mp_stock_mirror_read(0, storage.as_mut_ptr() as *mut u8) };
        let bytes = unsafe { storage.assume_init_ref() };
        MirroredMessage {
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
        let base = BITMAP_BASE + slot_offset(slot);
        unsafe { rb_mp_stock_bit_set(base, bit.0 as u32) };
    }

    fn frame_context(&self, slot: SlotIndex) -> Option<FrameContext> {
        live_context(slot).map(FrameContext)
    }

    fn clear_capture_word(&mut self, context: FrameContext) {
        write_u32(context.0 + CAPTURE_WORD, 0);
    }

    fn store_capture_word(&mut self, context: FrameContext, word: PermitWord) {
        write_u32(context.0 + PERMIT_WORD, word.0);
    }

    fn capture_word_is_one(&self, context: FrameContext) -> bool {
        read_u32(context.0 + KIND_FLAG) == 1
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

#[no_mangle]
#[link_section = ".text.rb_media_permit_resolve"]
pub extern "C" fn rb_media_permit_resolve(key: u32) -> i32 {
    resolve_key(key)
}

#[no_mangle]
#[link_section = ".text.rb_media_permit_apply"]
pub extern "C" fn rb_media_permit_apply() -> u32 {
    apply_mirror()
}



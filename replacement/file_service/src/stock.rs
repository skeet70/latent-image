//! The stock boundary: the only module with guest addresses, stock layouts and stock calls.

use crate::handles::{self, Acquired, FileCache, FileIndex, HandleMemory, Pair, Slot, Tag, TagLock};

const MAP: u32 = 0x910140;
const TAGS: u32 = 0x9101c0;
const SEQUENCE: u32 = 0x910200;
pub(crate) const TAG_SEMAPHORE: u32 = 0x3c;
pub(crate) const FOREVER: u32 = 0xffff_ffff;

extern "C" {
    pub(crate) fn file_service_semaphore_wait(semaphore: u32, timeout: u32) -> i32;
    pub(crate) fn file_service_semaphore_signal(semaphore: u32) -> i32;
    fn file_service_cache_claim() -> u32;
}

pub(crate) fn read_word(address: u32) -> u32 {
    unsafe { core::ptr::read_volatile(address as *const u32) }
}

pub(crate) fn write_word(address: u32, value: u32) {
    unsafe { core::ptr::write_volatile(address as *mut u32, value) }
}

pub(crate) fn read_byte(address: u32) -> u8 {
    unsafe { core::ptr::read_volatile(address as *const u8) }
}

pub(crate) fn write_byte(address: u32, value: u8) {
    unsafe { core::ptr::write_volatile(address as *mut u8, value) }
}

pub(crate) fn read_half(address: u32) -> u16 {
    unsafe { core::ptr::read_volatile(address as *const u16) }
}

pub(crate) fn write_half(address: u32, value: u16) {
    unsafe { core::ptr::write_volatile(address as *mut u16, value) }
}

struct StockHandles;

impl HandleMemory for StockHandles {
    fn tag(&self, slot: Slot) -> u32 {
        read_word(TAGS + 4 * slot.index() as u32)
    }

    fn set_tag(&mut self, slot: Slot, word: u32) {
        write_word(TAGS + 4 * slot.index() as u32, word)
    }

    fn sequence(&self) -> u32 {
        read_word(SEQUENCE)
    }

    fn set_sequence(&mut self, word: u32) {
        write_word(SEQUENCE, word)
    }

    fn pair(&self, slot: Slot) -> Pair {
        let at = MAP + 8 * slot.index() as u32;
        Pair { index: FileIndex(read_word(at)), tag: read_word(at + 4) }
    }

    fn set_pair(&mut self, slot: Slot, pair: Pair) {
        let at = MAP + 8 * slot.index() as u32;
        write_word(at, pair.index.0);
        write_word(at + 4, pair.tag);
    }
}

struct TagSemaphore;

impl TagLock for TagSemaphore {
    fn acquire(&mut self) -> Acquired {
        if unsafe { file_service_semaphore_wait(TAG_SEMAPHORE, FOREVER) } == 0 {
            Acquired::Yes
        } else {
            Acquired::No
        }
    }

    fn release(&mut self) {
        unsafe { file_service_semaphore_signal(TAG_SEMAPHORE) };
    }
}

struct StockFileCache;

impl FileCache for StockFileCache {
    fn claim(&mut self) -> FileIndex {
        FileIndex(unsafe { file_service_cache_claim() })
    }
}

/// Entered at 0x71ef18 by the open poster.
#[no_mangle]
pub extern "C" fn xm1_file_service_tag_allocate() -> u32 {
    handles::allocate(&mut StockHandles, &mut TagSemaphore).map_or(0, Tag::word)
}

/// Entered at 0x71eff4 by the close and release workers.
#[no_mangle]
pub extern "C" fn xm1_file_service_tag_release(tag: u32) {
    handles::release(&mut StockHandles, &mut TagSemaphore, Tag::new(tag))
}

/// Entered at 0x71ea34 by the open worker.
#[no_mangle]
pub extern "C" fn xm1_file_service_map_bind(tag: u32) -> u32 {
    handles::bind(&mut StockHandles, &mut StockFileCache, tag).0
}

/// Entered at 0x71eaac by the close and release workers.
#[no_mangle]
pub extern "C" fn xm1_file_service_map_unbind(tag: u32) {
    handles::unbind(&mut StockHandles, Tag::new(tag))
}

/// Entered at 0x71eb28 by the read, write, seek, close and detail workers.
#[no_mangle]
pub extern "C" fn xm1_file_service_map_lookup(tag: u32) -> u32 {
    handles::lookup(&StockHandles, Tag::new(tag)).0
}

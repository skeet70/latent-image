use core::arch::asm;
use core::ptr::{addr_of, read_volatile, write_volatile};
use xm1_fixed_image_layout::{BLOCK_BYTES, HEADER_BYTES, IMAGE_BASE, IMAGE_LIMIT, RECORD_FLASH, RECORD_MAGIC};
use super::media;

#[used]
#[no_mangle]
#[link_section = ".rodata.xm1_first_stage_contract"]
pub static XM1_FIXED_IMAGE_BUILD_ID: u32 = 0xfeed_1170;

#[derive(Debug)]
pub enum LoadError {
    Media(media::MediaError),
    Header,
    Integrity,
    Build,
}

unsafe fn header(index: u32) -> u32 {
    read_volatile(((IMAGE_BASE | 0x4000_0000) + index * 4) as *const u32)
}

#[inline(never)]
#[no_mangle]
pub unsafe fn xm1_first_stage_fixed_image_load() -> Result<(), LoadError> {
    media::read_units(RECORD_FLASH, IMAGE_BASE, 1).map_err(LoadError::Media)?;
    let length = header(2);
    if header(0) != RECORD_MAGIC || header(1) != IMAGE_BASE
        || length <= HEADER_BYTES || length > IMAGE_LIMIT - IMAGE_BASE {
        return Err(LoadError::Header);
    }
    let begin = header(5);
    let end = header(6);
    if begin < IMAGE_BASE + length || begin > end || end > IMAGE_LIMIT
        || (begin | end) & 3 != 0 || header(7) != 0 {
        return Err(LoadError::Header);
    }
    if length > BLOCK_BYTES {
        media::read_units(RECORD_FLASH + BLOCK_BYTES, IMAGE_BASE + BLOCK_BYTES, 1).map_err(LoadError::Media)?;
    }
    let mut crc = 0xffff_ffffu32;
    for offset in HEADER_BYTES..length {
        crc ^= read_volatile(((IMAGE_BASE | 0x4000_0000) + offset) as *const u8) as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 == 0 { 0 } else { 0xedb8_8320 };
        }
    }
    if !crc != header(3) {
        return Err(LoadError::Integrity);
    }
    if header(4) != read_volatile(addr_of!(XM1_FIXED_IMAGE_BUILD_ID)) {
        return Err(LoadError::Build);
    }
    for address in (begin..end).step_by(4) {
        write_volatile(address as *mut u32, 0);
    }
    asm!("dsb sy", "mcr p15, 0, {zero}, c7, c5, 0", "isb sy", zero = in(reg) 0u32, options(nostack));
    Ok(())
}

#[inline(never)]
#[no_mangle]
pub(super) unsafe fn xm1_first_stage_fixed_image_halt(error: super::BootError) -> ! {
    super::fail(error)
}

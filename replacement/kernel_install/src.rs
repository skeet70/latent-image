#![no_std]

use core::arch::asm;
use core::num::NonZeroU32;
use core::panic::PanicInfo;

#[repr(transparent)]
#[derive(Clone, Copy)]
struct WordAddress(u32);
#[repr(transparent)]
struct WordCount(NonZeroU32);
#[repr(transparent)]
struct CacheBytes(u32);

extern "C" {
    fn xm1_kernel_cache(address: WordAddress, bytes: CacheBytes);
}

impl WordAddress {
    #[inline(always)]
    unsafe fn load<const OFFSET: u32>(self) -> u32 {
        let value;
        unsafe { asm!("ldr {value}, [{address}, #{offset}]", value = out(reg) value, address = in(reg) self.0,
            offset = const OFFSET, options(nostack, readonly, preserves_flags)); }
        value
    }

    #[inline(always)]
    unsafe fn store<const OFFSET: i32>(self, value: u32) {
        unsafe { asm!("str {value}, [{address}, #{offset}]", value = in(reg) value, address = in(reg) self.0,
            offset = const OFFSET, options(nostack, preserves_flags)); }
    }

    #[inline(always)]
    fn offset(self, bytes: u32) -> Self { Self(self.0 + bytes) }
}

#[inline(never)]
unsafe fn zero(mut destination: WordAddress, words: WordCount) {
    let mut remaining = words.0.get();
    loop {
        unsafe { destination.store::<0>(0); }
        destination = destination.offset(4);
        remaining -= 1;
        if remaining == 0 { break; }
    }
}

/// # Safety
/// The caller supplies mapped aligned guest words, an AAPCS stack and live immutable code. Both cores and non-CPU writers must preserve the admitted headers, marker, destination objects and executing stack. The cache service returns in A32 preserving r4-r11 and SP.
#[no_mangle]
pub unsafe extern "C" fn xm1_kernel_install() {
    unsafe {
        let roots = WordAddress(0x96bd20);
        roots.store::<0>(WordAddress(0x40000).load::<16>());
        roots.store::<4>(WordAddress(0xd0000).load::<16>());
        xm1_kernel_cache(WordAddress(0x96bd20), CacheBytes(32));
        if WordAddress(0x7caf3c).load::<0>() != 3 {
            loop { asm!("b .", options(nomem, nostack)); }
        }
        zero(WordAddress(0x40a0f870), WordCount(NonZeroU32::new(120).unwrap()));
        let cursors = WordAddress(0x40a0fc30);
        cursors.store::<0>(120);
        cursors.store::<4>(120);
        cursors.store::<{ -964 }>(250);
        cursors.store::<8>(120);
        cursors.store::<12>(1);
        cursors.store::<16>(122);
        WordAddress(0x409c3500).store::<0>(0x7531);
        let mut offset = 4;
        while offset != 260 {
            let word = WordAddress(0x409c3600).offset(offset);
            word.store::<0>(0);
            word.store::<512>(0);
            offset += 4;
        }
        zero(WordAddress(0x40a0fc44), WordCount(NonZeroU32::new(360).unwrap()));
        zero(WordAddress(0x409c3904), WordCount(NonZeroU32::new(720).unwrap()));
        zero(WordAddress(0x40a0fa50), WordCount(NonZeroU32::new(120).unwrap()));
    }
}

#[panic_handler]
fn panic(_: &PanicInfo<'_>) -> ! {
    loop { core::hint::spin_loop(); }
}

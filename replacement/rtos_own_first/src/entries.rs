use core::arch::{asm, global_asm};
use core::panic::PanicInfo;
use core::mem::MaybeUninit;
use core::ptr::{read_volatile, write_volatile};

use crate::*;

struct Cpu;

impl Machine for Cpu {
    #[inline(always)]
    fn load_word(&self, at: GuestAddress) -> u32 {
        unsafe { read_volatile(at.0 as *const u32) }
    }

    #[inline(always)]
    fn load_byte(&self, at: GuestAddress) -> u8 {
        unsafe { read_volatile(at.0 as *const u8) }
    }

    #[inline(always)]
    fn store_word(&self, at: GuestAddress, value: u32) {
        unsafe { write_volatile(at.0 as *mut u32, value) }
    }

    #[inline(always)]
    fn cpsr(&self) -> Cpsr {
        let value: u32;
        unsafe { asm!("mrs {}, cpsr", out(reg) value, options(nomem, nostack, preserves_flags)) };
        Cpsr(value)
    }

    #[inline(always)]
    fn write_cpsr_control(&self, value: Cpsr) {
        unsafe { asm!("msr cpsr_c, {}", in(reg) value.0, options(nostack, preserves_flags)) };
    }

    #[inline(always)]
    fn debug_identity(&self) -> DebugIdentity {
        let value: u32;
        unsafe { asm!("mrc p14, 0, {}, c2, c0, 0", out(reg) value, options(nomem, nostack, preserves_flags)) };
        DebugIdentity(value)
    }
}

const WORDS: &DomainWords = &DOMAIN.words();

#[no_mangle]
#[inline(never)]
#[link_section = ".text.slot_60"]
pub extern "C" fn xm1_rtos_own_first_slot_60() -> Flag {
    slot_60(&Cpu, WORDS)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.slot_fc"]
pub extern "C" fn xm1_rtos_own_first_slot_fc(now: GuestAddress) -> ServiceStatus {
    slot_fc(&Cpu, WORDS, now)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.slot_100"]
pub extern "C" fn xm1_rtos_own_first_slot_100(now: GuestAddress) -> ServiceStatus {
    slot_100(&Cpu, WORDS, now)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.slot_128"]
pub extern "C" fn xm1_rtos_own_first_slot_128(id: GuestAddress) -> ServiceStatus {
    slot_128(&Cpu, WORDS, id)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.slot_12c"]
pub extern "C" fn xm1_rtos_own_first_slot_12c(id: GuestAddress) -> ServiceStatus {
    slot_12c(&Cpu, WORDS, id)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.slot_130"]
pub extern "C" fn xm1_rtos_own_first_slot_130() -> ServiceStatus {
    slot_130(&Cpu, WORDS)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.slot_134"]
pub extern "C" fn xm1_rtos_own_first_slot_134() -> ServiceStatus {
    slot_134(&Cpu, WORDS)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.slot_138"]
pub extern "C" fn xm1_rtos_own_first_slot_138() -> ServiceStatus {
    slot_138(&Cpu, WORDS)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.slot_13c"]
pub extern "C" fn xm1_rtos_own_first_slot_13c() -> ServiceStatus {
    slot_13c(&Cpu, WORDS)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.slot_148"]
pub extern "C" fn xm1_rtos_own_first_slot_148() -> Flag {
    slot_148(&Cpu, WORDS)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.slot_14c"]
pub extern "C" fn xm1_rtos_own_first_slot_14c() -> Flag {
    slot_14c(&Cpu)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.slot_150"]
pub extern "C" fn xm1_rtos_own_first_slot_150() -> Flag {
    slot_150(&Cpu, WORDS)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.slot_154"]
pub extern "C" fn xm1_rtos_own_first_slot_154() -> Flag {
    slot_154(&Cpu, WORDS)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.slot_164"]
pub extern "C" fn xm1_rtos_own_first_slot_164() -> SavedIrqMask {
    slot_164(&Cpu)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.slot_168"]
pub extern "C" fn xm1_rtos_own_first_slot_168(saved: SavedIrqMask) -> Cpsr {
    slot_168(&Cpu, saved)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.slot_16c"]
pub extern "C" fn xm1_rtos_own_first_slot_16c() -> TaskId {
    slot_16c(
        || xm1_rtos_own_first_slot_148(),
        || {
            let mut id = MaybeUninit::<TaskId>::uninit();
            if xm1_rtos_own_first_slot_128(GuestAddress(id.as_mut_ptr() as u32)) != ServiceStatus::OK {
                return None;
            }
            // +0x128 writes the id whenever it returns OK.
            Some(unsafe { id.assume_init() })
        },
    )
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.slot_1618_body"]
pub extern "C" fn xm1_rtos_own_first_slot_1618_body() -> Flag {
    slot_1618(&Cpu)
}

global_asm!(
    ".pushsection .text.slot_1618, \"ax\", %progbits",
    ".arm",
    ".global xm1_rtos_own_first_slot_1618",
    ".type xm1_rtos_own_first_slot_1618, %function",
    "xm1_rtos_own_first_slot_1618:",
    "b xm1_rtos_own_first_slot_1618_body",
    ".size xm1_rtos_own_first_slot_1618, . - xm1_rtos_own_first_slot_1618",
    ".popsection",
);

#[panic_handler]
fn panic(_: &PanicInfo<'_>) -> ! {
    loop {}
}

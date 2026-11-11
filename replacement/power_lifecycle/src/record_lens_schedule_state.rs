//! Lens scheduling publications in the canonical task 50 runtime at 0x00942ab0.

#[cfg(target_arch = "arm")]
mod native {
    use core::ptr::{read_volatile, write_volatile};

    const RUNTIME: usize = 0x0094_2ab0;

    extern "C" {
        fn xm1_power_interrupt_lock() -> u32;
        fn xm1_power_interrupt_unlock(saved: u32);
    }

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_record_helper_76f438() -> u32 {
        let saved = unsafe { xm1_power_interrupt_lock() };
        let sample = unsafe { read_volatile((RUNTIME + 0x74) as *const u32) };
        let current = unsafe { read_volatile(RUNTIME as *const u32) };
        unsafe {
            write_volatile((RUNTIME + 0x70) as *mut u32, current);
            write_volatile((RUNTIME + 0x74) as *mut u32, 0);
            xm1_power_interrupt_unlock(saved);
        }
        sample
    }

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_record_helper_76fc4c() -> u32 {
        unsafe { read_volatile((RUNTIME + 0x98) as *const u32) }
    }

    /// # Safety
    /// `output` addresses a writable, four-byte-aligned 16-byte lens snapshot.
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_record_helper_76fb54(output: *mut u8) {
        let position = unsafe { read_volatile((RUNTIME + 0x110) as *const u32) };
        unsafe { write_volatile(output.cast::<u16>(), position as u16); }
        for offset in [4usize, 8, 12] {
            let value = unsafe { read_volatile((RUNTIME + 0x110 + offset) as *const u32) };
            unsafe { write_volatile(output.add(offset).cast::<u32>(), value); }
        }
    }
}

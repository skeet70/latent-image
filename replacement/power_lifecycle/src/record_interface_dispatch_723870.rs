//! OS interface dispatch bodies 0x00723870, 0x007239a0 and 0x007257a8.
//!
//! The two configure sequences are owned by the Power services leaf:
//! `xm1_power_services::charge_timers::TimerIrqRegistry::install_irq` writes the
//! five-word record through interface slot 0x160 and enables slot 0x158 under
//! the critical section, and `TimerHandleRegistry::prepare_interface` runs slots
//! 0x15c and 0x160 on handle zero. The tick read is owned as
//! `xm1_power_services::shutdown_rtos::xm1_power_read_interface_word_174`, which
//! resolves slot 0x1618, reads the interface word at 0x174, and stores the word
//! it points at.

/// The stock-named entry for the image-4 body at 0x007257a8.
#[cfg(target_arch = "arm")]
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_7257a8(output: *mut u32) {
    unsafe { xm1_power_services::shutdown_rtos::xm1_power_read_interface_word_174(output) }
}

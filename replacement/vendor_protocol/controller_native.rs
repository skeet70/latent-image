//! The two ARM entries stock registers for the unit.
//!
//! 0x00d80d44 creates queue 36 (`0x00724cdc(36, {0x30, 0x20})`) and task
//! `{36, "usb", priority 11, stack 0xc00, entry 0x00d81218, argument 0}`
//! through `0x00725dd8`. 0x00d7b724 registers interrupt index 15, whose table
//! entry at 0x007c91d8 is line 84, priority 13, flags 1, with handler
//! 0x00d7b250. The integration installs the constructed root before either entry
//! runs.

use crate::transport::{ControllerIo, Register};

/// 0x00d7b250: the interrupt prologue. The handler body 0x00d81480 follows.
pub fn interrupt_prologue(io: &mut impl ControllerIo) {
    io.write32(Register::InterruptReset, 0x8181_0303);
    io.write16(Register::EventClearLow, 0xffff);
    io.write16(Register::EventClearMiddle, 0xffff);
    io.write16(Register::EventClearHigh, 0xffff);
}

/// The installed root's two stock entries.
pub trait NativeController {
    /// 0x00d81480: set completion-handle bit 16, publish the completion status and
    /// queue 0x340e.
    fn transport_status(&mut self);
    /// One queue-36 receive loop; false after the receive fails.
    fn run(&mut self) -> bool;
}

static mut INSTANCE: Option<&'static mut dyn NativeController> = None;

/// # Safety
/// Call once before the task and interrupt entries run; the root must outlive them.
pub unsafe fn install(instance: &'static mut dyn NativeController) {
    unsafe { INSTANCE = Some(instance); }
}

fn instance() -> Option<&'static mut dyn NativeController> {
    unsafe { INSTANCE.as_mut().map(|instance| &mut **instance) }
}

/// 0x00d7b250 with the 0x00d81480 body.
pub fn interrupt(io: &mut impl ControllerIo, controller: Option<&mut dyn NativeController>) {
    interrupt_prologue(io);
    if let Some(controller) = controller { controller.transport_status(); }
}

/// 0x00d81218.
pub fn task(controller: Option<&mut dyn NativeController>) {
    if let Some(controller) = controller { let _ = controller.run(); }
}

/// 0x00d7b250.
#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn xm1_vendor_usb_interrupt() {
    let mut io = unsafe { crate::transport_start_cpu::MappedUsbIo::new() };
    interrupt(&mut io, instance());
}

/// 0x00d81218. The kernel ends the task when the receive loop returns.
#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn xm1_vendor_usb_task() {
    task(instance());
}

#[cfg(test)]
#[path = "controller_native_tests.rs"]
mod tests;

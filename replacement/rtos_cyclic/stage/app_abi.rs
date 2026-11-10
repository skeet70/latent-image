//! A32 app callback entries over the owned caller core and registry dispatcher.

use crate::app::{
    register_callback, unregister_callback, CallbackHandler, CallbackMachine, CallbackSlot,
    CallbackSlots, CallbackStatus, CallbackToken, CriticalToken, InterfaceId, TokenOutput,
    DISPATCH_EVENT,
};
use crate::types::{CyclicCreate, PeriodWord};
use crate::EncodedId;
use crate::GuestAddress;

const INTERFACES: u32 = 0x0096_bd20;
const SLOTS: u32 = 0x409c_3500;
const ERROR_RECORD: u32 = 0x0004_1a34;
const LAST_ERROR: u32 = 0x40a0_f82c;
const DISPATCH_ERROR: u32 = 0x0072_3398;

#[inline(always)]
fn read(address: u32) -> u32 {
    unsafe { core::ptr::read_volatile(address as *const u32) }
}

#[inline(always)]
fn write(address: u32, value: u32) {
    unsafe { core::ptr::write_volatile(address as *mut u32, value) }
}

#[inline(always)]
fn root(interface: InterfaceId) -> u32 {
    read(INTERFACES.wrapping_add(interface.0.wrapping_mul(4)))
}

#[inline(always)]
fn method(interface: InterfaceId, offset: u32) -> u32 {
    read(root(interface).wrapping_add(offset))
}

struct App;

impl CallbackMachine for App {
    fn load_word(&self, at: GuestAddress) -> u32 {
        read(at.0)
    }

    fn store_word(&self, at: GuestAddress, value: u32) {
        write(at.0, value);
    }

    fn current_interface(&self) -> InterfaceId {
        let current: extern "C" fn() -> u32 =
            unsafe { core::mem::transmute(read(read(INTERFACES).wrapping_add(0x1618))) };
        InterfaceId(current())
    }

    fn create_callback(
        &self,
        interface: InterfaceId,
        id: EncodedId,
        request: CyclicCreate,
    ) -> CallbackStatus {
        let create: extern "C" fn(u32, *const u32) -> i32 =
            unsafe { core::mem::transmute(method(interface, 0x108)) };
        let words = [
            request.attributes.0,
            request.argument.0,
            request.handler.0,
            request.period.0,
            request.phase.0,
        ];
        CallbackStatus::from_code(create(id.0 as u32, words.as_ptr()))
    }

    fn start_callback(&self, interface: InterfaceId, id: EncodedId) -> CallbackStatus {
        let start: extern "C" fn(u32) -> i32 =
            unsafe { core::mem::transmute(method(interface, 0x110)) };
        CallbackStatus::from_code(start(id.0 as u32))
    }

    fn stop_callback(&self, interface: InterfaceId, id: EncodedId) {
        let stop: extern "C" fn(u32) -> i32 =
            unsafe { core::mem::transmute(method(interface, 0x114)) };
        let _ = stop(id.0 as u32);
    }

    fn delete_callback(&self, interface: InterfaceId, id: EncodedId) {
        let delete: extern "C" fn(u32) -> i32 =
            unsafe { core::mem::transmute(method(interface, 0x10c)) };
        let _ = delete(id.0 as u32);
    }

    fn enter_critical(&self, interface: InterfaceId) -> CriticalToken {
        let enter: extern "C" fn() -> u32 =
            unsafe { core::mem::transmute(method(interface, 0x164)) };
        CriticalToken(enter())
    }

    fn leave_critical(&self, interface: InterfaceId, previous: CriticalToken) {
        let leave: extern "C" fn(u32) =
            unsafe { core::mem::transmute(method(interface, 0x168)) };
        leave(previous.0);
    }

    fn record_error(&self, operation: u32, mode: i32, status: CallbackStatus) {
        let record: extern "C" fn(u32, i32, i32) = unsafe { core::mem::transmute(ERROR_RECORD) };
        record(operation, mode, status.code());
        write(LAST_ERROR, status.code() as u32);
        let dispatch: extern "C" fn(i32) = unsafe { core::mem::transmute(DISPATCH_ERROR) };
        dispatch(status.code());
    }
}

/// # Safety
/// `slot` identifies an installed callback with an AAPCS entry.
#[unsafe(naked)]
#[no_mangle]
#[link_section = ".text.app_dispatch"]
pub unsafe extern "C" fn xm1_app_dispatch_callback(_slot: CallbackSlot) {
    core::arch::naked_asm!(
        "movw r12, #{slots_low}",
        "movt r12, #{slots_high}",
        "ldr r12, [r12, r0, lsl #2]",
        "bx r12",
        slots_low = const SLOTS & 0xffff,
        slots_high = const SLOTS >> 16,
    );
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.app_register"]
pub extern "C" fn xm1_app_register_callback(
    argument: u32,
    mode: i32,
    callback: u32,
    token: *mut i32,
) -> i32 {
    register_callback(
        &App,
        CallbackSlots::at(GuestAddress(SLOTS)),
        PeriodWord(argument),
        mode,
        DISPATCH_EVENT,
        CallbackHandler::new(callback),
        TokenOutput::at(GuestAddress(token as u32)),
    )
    .code()
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.app_unregister"]
pub extern "C" fn xm1_app_unregister_callback(token: i32) -> i32 {
    unregister_callback(
        &App,
        CallbackSlots::at(GuestAddress(SLOTS)),
        CallbackToken::new(token),
    )
    .code()
}

#[cfg(all(target_arch = "arm", not(test)))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

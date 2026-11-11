use super::*;
use crate::callback_registry::kernel_slot;
use core::ptr::{read_volatile, write_volatile};

extern "C" {
    fn xm1_app_unregister_callback(token: i32) -> i32;
    fn xm1_power_semaphore_wait(id: i32, timeout: i32) -> i32;
    fn xm1_power_semaphore_signal(id: i32) -> i32;
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
    fn xm1_power_delay_us_with_residue(duration: u32) -> u32;
    fn xm1_power_device_timer_unregister(timer: u32) -> i32;
    fn xm1_power_serial_open(port: u32, mode: u32, output: *mut u32) -> i32;
    fn xm1_power_serial_configure(handle: u32, value: u32) -> i32;
    fn xm1_power_serial_write_with_residue(handle: u32, byte: u32, residue: u32) -> i32;
    fn xm1_power_serial_status(handle: u32, output: *mut u32) -> i32;
    fn xm1_power_serial_read(handle: u32, output: *mut u16) -> i32;
    fn xm1_power_serial_stop(handle: u32) -> i32;
    fn xm1_power_serial_close(handle: u32) -> i32;
}

pub struct NativeChargeTimers;
fn load(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
fn store(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }

impl ChargeShutdown for NativeChargeTimers {
    fn callback_token(&mut self) -> CallbackToken { CallbackToken(load(0x89f7ac) as i32) }
    fn unregister_callback(&mut self, token: CallbackToken) { unsafe { xm1_app_unregister_callback(token.0); } }
    fn lock_charge(&mut self) { unsafe { xm1_power_semaphore_wait(66, -1); } }
    fn clear_charge_active(&mut self) { store(0x9552f4, 0); }
    fn unlock_charge(&mut self) { unsafe { xm1_power_semaphore_signal(66); } }
    fn send_charge_packet(&mut self, packet: [u8; 2]) -> ServiceStatus { transmit_charge(self, &packet) }
}
impl TimerShutdown for NativeChargeTimers {
    fn registered(&mut self) -> bool { load(0x89f7c8) != 0 }
    fn send_unregister(&mut self) -> ServiceStatus { ServiceStatus(unsafe { xm1_power_device_timer_unregister(0) }) }
    fn clear_registered(&mut self) { store(0x89f7c8, 0); }
    fn lock_interrupts(&mut self) -> SavedIrq { SavedIrq(unsafe { xm1_power_interrupt_lock() }) }
    fn slot_handle(&mut self, slot: TimerSlot) -> Option<InterfaceHandle> {
        let handle = load(0x93c4f4 + slot.index() * 8);
        if handle == 0 { None } else { Some(InterfaceHandle(handle)) }
    }
    fn release_handle(&mut self, handle: InterfaceHandle) -> ServiceStatus { release_timer_handle(self, handle) }
    fn clear_slot(&mut self, slot: TimerSlot) { store(0x93c4f4 + slot.index() * 8, 0); }
    fn restore_interrupts(&mut self, saved: SavedIrq) { unsafe { xm1_power_interrupt_unlock(saved.0) } }
}
impl ChargeTransport for NativeChargeTimers {
    fn open_charge_serial(&mut self) -> Result<SerialHandle, ServiceStatus> {
        let mut handle = 0;
        let status = unsafe { xm1_power_serial_open(10, 2, &mut handle) };
        if status == 0 { Ok(SerialHandle(handle)) } else { Err(ServiceStatus(status)) }
    }
    fn write_charge_pin(&mut self, value: u32) { store(0xfff892c4, value); }
    fn configure(&mut self, handle: SerialHandle) -> (ServiceStatus, StatusResidue) {
        let status = unsafe { xm1_power_serial_configure(handle.0, 100) };
        (ServiceStatus(status), StatusResidue(if status == 0 { 0 } else { 0x15 }))
    }
    fn send_byte(&mut self, handle: SerialHandle, byte: u8, residue: StatusResidue) -> ServiceStatus {
        ServiceStatus(unsafe { xm1_power_serial_write_with_residue(handle.0, byte as u32, residue.0) })
    }
    fn status(&mut self, handle: SerialHandle) -> (ServiceStatus, SerialStatus) {
        // The private charge handle remains registered until close. Status writes all seven words.
        let mut words = [0u32; 7];
        let status = unsafe { xm1_power_serial_status(handle.0, words.as_mut_ptr()) };
        (ServiceStatus(status), SerialStatus { state: words[1], available: words[3] })
    }
    fn read_response(&mut self, handle: SerialHandle) -> ServiceStatus {
        let mut response = 0;
        ServiceStatus(unsafe { xm1_power_serial_read(handle.0, &mut response) })
    }
    fn delay(&mut self, duration: DelayMicros) -> TimerReload { TimerReload(unsafe { xm1_power_delay_us_with_residue(duration.0) }) }
    fn stop(&mut self, handle: SerialHandle) -> ServiceStatus { ServiceStatus(unsafe { xm1_power_serial_stop(handle.0) }) }
    fn close(&mut self, handle: SerialHandle) -> ServiceStatus { ServiceStatus(unsafe { xm1_power_serial_close(handle.0) }) }
}
impl TimerHandleRegistry for NativeChargeTimers {
    fn handle(&mut self, index: usize) -> InterfaceHandle { InterfaceHandle(load(0x940e9c + index * 4)) }
    fn prepare_interface(&mut self, index: usize) -> ServiceStatus {
        let root = load(0x96bd20);
        let current: unsafe extern "C" fn() -> u32 = unsafe { kernel_slot(root, 0x1618) };
        let interface = load(0x96bd20 + unsafe { current() } as usize * 4);
        let enter: unsafe extern "C" fn() -> u32 = unsafe { kernel_slot(interface, 0x164) };
        let stop: unsafe extern "C" fn(u32) -> i32 = unsafe { kernel_slot(interface, 0x15c) };
        let clear: unsafe extern "C" fn(u32, *const u32) -> i32 = unsafe { kernel_slot(interface, 0x160) };
        let leave: unsafe extern "C" fn(u32) = unsafe { kernel_slot(interface, 0x168) };
        let saved = unsafe { enter() };
        unsafe { stop(index as u32) };
        unsafe { clear(index as u32, core::ptr::null()) };
        unsafe { leave(saved) };
        ServiceStatus(0)
    }
    fn invalidate_interface(&mut self, index: usize) { store(0x94109c + index * 4, u32::MAX); }
    fn clear_handle(&mut self, index: usize) { store(0x940e9c + index * 4, 0); }
}

#[no_mangle]
pub extern "C" fn xm1_power_charge_stop() -> i32 { stop_charge(&mut NativeChargeTimers).0 }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_charge_transmit(bytes: *const u8, length: u32) -> i32 {
    let packet = if length == 0 { &[] } else { core::slice::from_raw_parts(bytes, length as usize) };
    transmit_charge(&mut NativeChargeTimers, packet).0
}
#[no_mangle]
pub extern "C" fn xm1_power_timers_stop() -> i32 { stop_timers(&mut NativeChargeTimers).abi() }
#[no_mangle]
pub extern "C" fn xm1_power_timer_release_handle(handle: u32) -> i32 { release_timer_handle(&mut NativeChargeTimers, InterfaceHandle(handle)).0 }

extern "C" { fn xm1_power_command_diagnostic(category: u32, code: u32) -> i32; }
impl TimerIrqRegistry for NativeChargeTimers {
    fn route(&mut self, source: IrqSource) -> Option<IrqRoute> {
        let row = 0x7c9124 + source.index() * 12;
        IrqRoute::from_abi(load(row), load(row + 4), load(row + 8))
    }
    fn lock_registry(&mut self) -> SavedIrq { SavedIrq(unsafe { xm1_power_interrupt_lock() }) }
    fn unlock_registry(&mut self, saved: SavedIrq) { unsafe { xm1_power_interrupt_unlock(saved.0) } }
    fn token_counter(&mut self) -> u32 { load(0x409c2804) }
    fn store_token_counter(&mut self, token: u32) { store(0x409c2804, token); }
    fn store_handle(&mut self, slot: usize, handle: InterfaceHandle) { store(0x940e9c + slot * 4, handle.0); }
    fn install_irq(&mut self, route: IrqRoute, handler: IrqHandler) -> ServiceStatus {
        let (polarity, mode) = match route.trigger() {
            IrqTrigger::Mode0 => (0, 2),
            IrqTrigger::Mode1 => (0, 1),
            IrqTrigger::Mode2 => (1, 2),
            IrqTrigger::Mode3 => (1, 1),
        };
        let record = [0, handler.address(), route.priority(), polarity, mode];
        let root = load(0x96bd20);
        let current: unsafe extern "C" fn() -> u32 = unsafe { kernel_slot(root, 0x1618) };
        let interface = load(0x96bd20 + unsafe { current() } as usize * 4);
        let enter: unsafe extern "C" fn() -> u32 = unsafe { kernel_slot(interface, 0x164) };
        let configure: unsafe extern "C" fn(u32, *const u32) -> i32 = unsafe { kernel_slot(interface, 0x160) };
        let enable: unsafe extern "C" fn(u32) -> i32 = unsafe { kernel_slot(interface, 0x158) };
        let leave: unsafe extern "C" fn(u32) = unsafe { kernel_slot(interface, 0x168) };
        let saved = unsafe { enter() };
        unsafe { configure(route.slot() as u32, record.as_ptr()) };
        unsafe { enable(route.slot() as u32) };
        unsafe { leave(saved) };
        ServiceStatus(0)
    }
    fn store_source(&mut self, slot: usize, source: IrqSource) { store(0x94109c + slot * 4, source.index() as u32); }
    fn registration_diagnostic(&mut self) { unsafe { xm1_power_command_diagnostic(2, 15); } }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_timer_irq_attach(source: u32, handler: u32, output: *mut u32) -> i32 {
    let Some(source) = IrqSource::from_abi(source) else {
        xm1_power_command_diagnostic(2, 15);
        return -30;
    };
    if let Err(error) = crate::charge_timers::admit_irq_handler(crate::callback_registry::CallbackWord(handler as usize)) {
        return crate::callback_registry::refused(error);
    }
    let Some(handler) = IrqHandler::from_abi(handler) else {
        xm1_power_command_diagnostic(2, 15);
        return -30;
    };
    match attach_timer_irq(&mut NativeChargeTimers, source, handler) {
        Ok(handle) => { write_volatile(output, handle.0); 0 }
        Err(status) => status.0,
    }
}

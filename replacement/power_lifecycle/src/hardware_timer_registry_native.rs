use super::*;
use core::ptr::{read_volatile, write_volatile};

extern "C" {
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
    fn xm1_power_timer_irq_attach(source: u32, handler: unsafe extern "C" fn(), output: *mut u32) -> i32;
    fn xm1_power_timer_release_handle(handle: u32) -> i32;
    fn xm1_power_command_diagnostic(category: u32, code: u32) -> i32;
}
struct Native;
impl Diagnostics for Native {
    fn command_error(&mut self, diagnostic: Diagnostic) {
        unsafe { xm1_power_command_diagnostic(2, diagnostic as u32); }
    }
}
impl Hardware for Native {
    fn write(&mut self, channel: Channel, register: Register, value: u32) {
        unsafe { write_volatile((channel.bank() + register.offset()) as *mut u32, value); }
    }
    fn attach(&mut self, channel: Channel) -> Result<IrqToken, i32> {
        let handlers: [unsafe extern "C" fn(); 4] = [irq_zero, irq_one, irq_two, irq_three];
        let output = (0x94449c + channel.index() * 4) as *mut u32;
        let status = unsafe { xm1_power_timer_irq_attach(channel.source(), handlers[channel.index()], output) };
        if status == 0 { Ok(IrqToken(unsafe { read_volatile(output) })) } else { Err(status) }
    }
    fn detach(&mut self, token: IrqToken) -> i32 { unsafe { xm1_power_timer_release_handle(token.0) } }
}
unsafe fn registry() -> &'static mut Registry { unsafe { &mut *(0x94446c as *mut Registry) } }
unsafe fn dispatch(channel: Channel) {
    let callback = unsafe { registry() }.interrupt(&mut Native, channel);
    let callback: unsafe extern "C" fn() = unsafe { core::mem::transmute(callback.0) };
    unsafe { callback(); }
}
unsafe extern "C" fn irq_zero() { unsafe { dispatch(Channel(0)); } }
unsafe extern "C" fn irq_one() { unsafe { dispatch(Channel(1)); } }
unsafe extern "C" fn irq_two() { unsafe { dispatch(Channel(2)); } }
unsafe extern "C" fn irq_three() { unsafe { dispatch(Channel(3)); } }

#[no_mangle]
pub unsafe extern "C" fn xm1_power_hardware_timer_acquire(selector: u32, output: *mut u32) -> i32 {
    let channel = match Channel::parse(selector) { Some(value) => value, None => return Error::InvalidConfiguration as i32 };
    let saved = unsafe { xm1_power_interrupt_lock() };
    let mut generation = Generation(unsafe { read_volatile(0x409c2804 as *const u32) });
    let result = unsafe { registry() }.acquire(channel, &mut generation);
    unsafe { write_volatile(0x409c2804 as *mut u32, generation.0); xm1_power_interrupt_unlock(saved); }
    match report_acquire(result, &mut Native) { Ok(handle) => { unsafe { write_volatile(output, handle.0); } 0 }, Err(error) => error as i32 }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_hardware_timer_start(handle: u32, request: *const HardwareTimerRequest) -> i32 {
    let request = unsafe { &*request };
    let units = match Units::parse(request.units) { Some(value) => value, None => return Error::InvalidConfiguration as i32 };
    let configuration = Configuration { period: Period(request.period), units, mode: Mode(request.mode), callback: request.callback.map(|value| Callback(value as usize as u32)) };
    unsafe { registry() }.start(&mut Native, Handle(handle), configuration)
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_hardware_timer_stop(handle: u32) -> i32 { unsafe { registry() }.stop(&mut Native, Handle(handle)) }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_hardware_timer_release(handle: u32) -> i32 {
    match release(unsafe { registry() }, Handle(handle), &mut Native) { Ok(()) => 0, Err(error) => error as i32 }
}

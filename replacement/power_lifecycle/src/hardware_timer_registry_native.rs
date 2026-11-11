use super::*;
use crate::callback_registry::{registry as callbacks, CallbackWord};
use core::ptr::{read_volatile, write_volatile};

extern "C" {
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
    fn xm1_power_timer_irq_attach(source: u32, handler: u32, output: *mut u32) -> i32;
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
        if let Err(error) = callbacks().ensure(handlers[channel.index()]) { return Err(crate::callback_registry::refused(error)); }
        let output = (0x94449c + channel.index() * 4) as *mut u32;
        let status = unsafe { xm1_power_timer_irq_attach(channel.source(), handlers[channel.index()] as usize as u32, output) };
        if status == 0 { Ok(IrqToken(unsafe { read_volatile(output) })) } else { Err(status) }
    }
    fn detach(&mut self, token: IrqToken) -> i32 { unsafe { xm1_power_timer_release_handle(token.0) } }
}
unsafe fn registry() -> &'static mut Registry { unsafe { &mut *(0x94446c as *mut Registry) } }
const REGISTRY: u32 = 0x94446c;
const PHASES: u32 = 0x00;
const HANDLES: u32 = 0x20;
const IRQ_TOKENS: u32 = 0x30;
const CALLBACKS: u32 = 0x40;
const MODES: u32 = 0x50;
unsafe fn read_field(offset: u32, index: usize) -> u32 { unsafe { read_volatile((REGISTRY + offset + index as u32 * 4) as *const u32) } }
unsafe fn write_field(offset: u32, index: usize, value: u32) { unsafe { write_volatile((REGISTRY + offset + index as u32 * 4) as *mut u32, value); } }
/// 0x655ccc: the interrupt arm reads and clears these words without a Rust reference.
unsafe fn interrupt(hardware: &mut impl Hardware, channel: Channel) -> Callback {
    let index = channel.index();
    if unsafe { read_field(MODES, index) } == 0 && unsafe { read_field(HANDLES, index) } != 0 && unsafe { read_field(PHASES, index) } != 1 {
        hardware.write(channel, Register::Control, 0);
        hardware.write(channel, Register::Reload, 0);
        let token = unsafe { read_field(IRQ_TOKENS, index) };
        unsafe { write_field(IRQ_TOKENS, index, 0); write_field(PHASES, index, 1); }
        if token != 0 { let _ = hardware.detach(IrqToken(token)); }
    }
    Callback(unsafe { read_field(CALLBACKS, index) })
}
unsafe fn dispatch(channel: Channel) {
    let callback = unsafe { interrupt(&mut Native, channel) };
    if let Ok(callback) = callbacks().resolve::<unsafe extern "C" fn()>(CallbackWord(callback.0 as usize)) { unsafe { callback() }; }
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
    if request.callback != 0 {
        if let Err(error) = callbacks().resolve::<unsafe extern "C" fn()>(CallbackWord(request.callback as usize)) { return crate::callback_registry::refused(error); }
    }
    let configuration = Configuration { period: Period(request.period), units, mode: Mode(request.mode), callback: if request.callback == 0 { None } else { Some(Callback(request.callback)) } };
    unsafe { registry() }.start(&mut Native, Handle(handle), configuration)
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_hardware_timer_stop(handle: u32) -> i32 { unsafe { registry() }.stop(&mut Native, Handle(handle)) }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_hardware_timer_release(handle: u32) -> i32 {
    match release(unsafe { registry() }, Handle(handle), &mut Native) { Ok(()) => 0, Err(error) => error as i32 }
}

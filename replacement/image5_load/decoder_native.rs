use super::*;

struct Native;

fn cell_pointer<T>(cell: Cell<T>) -> *mut core::sync::atomic::AtomicU32 {
    unsafe { core::ptr::addr_of!(XM1_DECODER_STATE).cast::<u8>().add(cell.offset.0).cast_mut().cast() }
}

#[repr(u32)]
enum Stage { Decoder = 0x410 }
#[repr(u32)]
enum Owner { Decoder = 0x21 }
#[repr(u32)]
enum Clock { Decoder = 0x29 }
#[repr(u32)]
enum Interrupt { Decoder = 0x62 }

extern "C" {
    fn xm1_decoder_current_task() -> u32;
    fn xm1_decoder_take(resource: Resource, timeout: u32) -> ServiceStatus;
    fn xm1_decoder_give(resource: Resource) -> ServiceStatus;
    fn xm1_decoder_route(stage: Stage, owner: Owner, level: RouteLevel);
    fn xm1_decoder_clock_take(resource: Clock, handle: *mut u32);
    fn xm1_decoder_clock_give(handle: u32);
    fn xm1_decoder_irq_attach(source: Interrupt, handler: unsafe extern "C" fn(), handle: *mut u32) -> ServiceStatus;
    fn xm1_decoder_irq_detach(handle: u32) -> ServiceStatus;
    fn xm1_decoder_marker(marker: Marker);
    fn xm1_decoder_progress_entry();
    fn xm1_decoder_complete_entry();
}

impl Bus for Native {
    fn read(&mut self, register: engine::Register) -> u32 { engine::VolatileBus.read(register) }
    fn write(&mut self, register: engine::Register, value: u32) { engine::VolatileBus.write(register, value); }
}

impl Port for Native {
    fn load<T: Word>(&mut self, cell: Cell<T>) -> T {
        T::decode(unsafe { (*cell_pointer(cell)).load(core::sync::atomic::Ordering::Relaxed) })
    }
    fn store<T: Word>(&mut self, cell: Cell<T>, value: T) {
        unsafe { (*cell_pointer(cell)).store(value.encode(), core::sync::atomic::Ordering::Relaxed); }
    }
    fn word(&mut self, address: Address) -> u32 { unsafe { core::ptr::read_volatile(address.0 as *const u32) } }
    fn byte(&mut self, address: Address) -> u8 { unsafe { core::ptr::read_volatile(address.0 as *const u8) } }
    fn task(&mut self) -> Task { Task(unsafe { xm1_decoder_current_task() }) }
    fn take(&mut self, resource: Resource) -> ServiceStatus { unsafe { xm1_decoder_take(resource, u32::MAX) } }
    fn give(&mut self, resource: Resource) -> ServiceStatus { unsafe { xm1_decoder_give(resource) } }
    fn route(&mut self, level: RouteLevel) { unsafe { xm1_decoder_route(Stage::Decoder, Owner::Decoder, level); } }
    fn reserve_clock(&mut self) { unsafe { xm1_decoder_clock_take(Clock::Decoder, cell_pointer(CLOCK).cast()); } }
    fn release_clock(&mut self, handle: Handle) { unsafe { xm1_decoder_clock_give(handle.0); } }
    fn attach(&mut self, callback: Callback) {
        let handler: unsafe extern "C" fn() = match callback { Callback::Progress => xm1_decoder_progress_entry, Callback::Final => xm1_decoder_complete_entry };
        let _ = unsafe { xm1_decoder_irq_attach(Interrupt::Decoder, handler, cell_pointer(IRQ).cast()) };
    }
    fn detach(&mut self, handle: Handle) { let _ = unsafe { xm1_decoder_irq_detach(handle.0) }; }
    fn marker(&mut self, marker: Marker) { unsafe { xm1_decoder_marker(marker); } }
}

fn retain_refusal(result: Result<(), Refusal>) {
    if result.is_err() { loop { core::hint::spin_loop(); } }
}

#[no_mangle]
#[link_section = ".decoder.progress"]
pub unsafe extern "C" fn xm1_decoder_progress() { retain_refusal(Decoder::new(Native).progress()); }

#[no_mangle]
#[link_section = ".decoder.complete"]
pub unsafe extern "C" fn xm1_decoder_complete() { retain_refusal(Decoder::new(Native).complete()); }

#[no_mangle]
#[link_section = ".decoder.init"]
pub unsafe extern "C" fn xm1_decoder_init(destination: u32, staging: u32) {
    retain_refusal(Decoder::new(Native).init(Destination(destination), Source(staging)));
}

#[no_mangle]
#[link_section = ".decoder.finish"]
pub unsafe extern "C" fn xm1_decoder_finish() { retain_refusal(Decoder::new(Native).finish()); }

#[no_mangle]
#[link_section = ".decoder.feed"]
pub unsafe extern "C" fn xm1_decoder_feed(loaded: u32) { retain_refusal(Decoder::new(Native).feed(Bytes(loaded))); }

#[cfg(all(target_arch = "arm", decoder_standalone))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! { loop { core::hint::spin_loop(); } }

use super::{
    consume, release, reserve, rotate, select, Address, Bus, ConsumerMode, Group, Pool, Route,
    SecondaryBank, Slot,
};
use core::ptr::{read_volatile, write_volatile};

extern "C" {
    fn xm1_af_critical_enter() -> u32;
    fn xm1_af_critical_leave(token: u32);
    fn xm1_af_secondary_mode() -> u32;
}

struct Firmware;
impl Bus for Firmware {
    #[inline(always)]
    fn read_word(&mut self, address: Address) -> u32 {
        unsafe { read_volatile(address.0 as *const u32) }
    }
    #[inline(always)]
    fn write_word(&mut self, address: Address, value: u32) {
        unsafe { write_volatile(address.0 as *mut u32, value) }
    }
    #[inline(always)]
    fn enter(&mut self) -> u32 {
        unsafe { xm1_af_critical_enter() }
    }
    #[inline(always)]
    fn leave(&mut self, token: u32) {
        unsafe { xm1_af_critical_leave(token) }
    }
}

#[no_mangle]
#[link_section = ".text.af_select_adapter"]
pub unsafe extern "C" fn xm1_af_select_adapter(group: u32, count: *mut u16, base: *mut u32) {
    let pool = select(&mut Firmware, Group::from_raw(group));
    unsafe {
        write_volatile(base, pool.base.0);
        write_volatile(count, pool.count);
    }
}

#[no_mangle]
#[link_section = ".text.af_rotate"]
pub unsafe extern "C" fn xm1_af_rotate(count: u16, base: u32) {
    rotate(&mut Firmware, Pool { base: Address(base), count });
}

#[no_mangle]
#[link_section = ".text.af_reserve"]
pub unsafe extern "C" fn xm1_af_reserve(requested: u32, group: u32) -> u32 {
    reserve(&mut Firmware, Slot(requested), Group::from_raw(group)).0
}

#[no_mangle]
#[link_section = ".text.af_release"]
pub unsafe extern "C" fn xm1_af_release(group: u32) {
    release(&mut Firmware, Group::from_raw(group));
}

#[no_mangle]
#[link_section = ".af_secondary_entry"]
pub unsafe extern "C" fn xm1_af_secondary_consumer() {
    let mut firmware = Firmware;
    let token = unsafe { xm1_af_critical_enter() };
    let mode = ConsumerMode::from_raw(unsafe { xm1_af_secondary_mode() });
    let route = Route::from_raw(firmware.read_word(Address(0x0170_327c)));
    consume(
        &mut firmware,
        SecondaryBank::new(Address(0x016f_ed28)),
        mode,
        route,
    );
    unsafe { xm1_af_critical_leave(token) };
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop { core::hint::spin_loop(); }
}

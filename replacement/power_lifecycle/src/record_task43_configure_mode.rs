use core::mem::MaybeUninit;
use core::ptr::{read_volatile, write_volatile};

pub(crate) const STATE: usize = 0x016b_e004;

#[derive(Clone, Copy)]
pub(crate) struct ProfileHandle(u32);
impl ProfileHandle { pub(crate) const fn raw(self) -> u32 { self.0 } }

pub(crate) unsafe fn word(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
pub(crate) unsafe fn put(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value); } }
pub(crate) unsafe fn profile() -> ProfileHandle { ProfileHandle(unsafe { word(STATE + 0x18) }) }

#[derive(Clone, Copy)]
enum Configuration { Mode2, Mode3, Mode9, Mode10, Mode11, Mode13, Mode23, Mode29, Mode34, Other }
impl Configuration {
    fn parse(raw: u32) -> Self {
        match raw { 2 => Self::Mode2, 3 => Self::Mode3, 9 => Self::Mode9, 10 => Self::Mode10, 11 => Self::Mode11, 13 => Self::Mode13, 23 => Self::Mode23, 29 => Self::Mode29, 34 => Self::Mode34, _ => Self::Other }
    }
}

unsafe extern "C" {
    fn xm1_power_record_helper_740d50(value: u32);
    fn xm1_power_record_helper_74eb20(handle: u32) -> u32;
    fn xm1_power_record_helper_74e648(handle: u32) -> u32;
    fn xm1_power_record_helper_74e5c0(handle: u32) -> u32;
    fn xm1_power_record_helper_74e7f0(handle: u32) -> u32;
    fn xm1_power_record_helper_74e020(handle: u32, mask: u64) -> u32;
    fn xm1_power_record_helper_756840(handle: u32, output: *mut u8);
    fn xm1_power_record_helper_756a0c(handle: u32, value: u32);
    fn xm1_power_record_helper_756a54(handle: u32, value: u32);
    fn xm1_power_record_helper_756a9c(handle: u32, value: u32);
    fn xm1_power_record_helper_756dfc(handle: u32, value: u32);
    fn xm1_power_record_helper_758340(handle: u32) -> u32;
    fn xm1_power_record_helper_eecd94();
    fn xm1_power_record_helper_f46e6c(handle: u32);
    fn xm1_power_record_helper_f30828();
    fn xm1_power_record_helper_f3088c();
    fn xm1_power_record_helper_f2fcd0() -> u32;
    fn xm1_power_record_helper_fa8038(value: u32);
    fn xm1_power_record_helper_fa7ff8(value: u32);
    fn xm1_power_record_helper_fa8058() -> u32;
}

unsafe fn set_modes(first: u32, second: u32, third: u32) {
    unsafe {
        xm1_power_record_helper_756a0c(profile().raw(), first);
        xm1_power_record_helper_756a54(profile().raw(), second);
        xm1_power_record_helper_756a9c(profile().raw(), third);
    }
}

#[repr(C, align(4))]
struct ProfileSnapshot([MaybeUninit<u8>; 48]);

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f308d4(raw_mode: u32) {
    unsafe {
        put(STATE + 0x14, raw_mode);
        put(STATE + 0x28, 0);
        put(STATE + 0x20, 0);
        put(STATE + 0x24, 0);
        match Configuration::parse(raw_mode) {
            Configuration::Mode2 => {
                xm1_power_record_helper_740d50(4);
                if xm1_power_record_helper_74eb20(profile().raw()) == 0 { xm1_power_record_helper_eecd94(); }
                xm1_power_record_helper_f46e6c(profile().raw());
                if xm1_power_record_helper_74e648(profile().raw()) == 57 { xm1_power_record_helper_fa8038(1); }
                const REQUEST: usize = 0x0169_d004;
                xm1_power_record_helper_756a0c(profile().raw(), word(REQUEST + 12));
                if word(REQUEST + 12) == 0 { put(STATE + 0x28, 1); }
                if word(REQUEST + 12) <= 1 {
                    xm1_power_record_helper_756a54(profile().raw(), 1);
                    xm1_power_record_helper_756a9c(profile().raw(), 1);
                    xm1_power_record_helper_f30828();
                    xm1_power_record_helper_f3088c();
                } else {
                    xm1_power_record_helper_756a54(profile().raw(), 2);
                    xm1_power_record_helper_756a9c(profile().raw(), 1);
                    if xm1_power_record_helper_74e7f0(profile().raw()) != 0 {
                        xm1_power_record_helper_f30828();
                        xm1_power_record_helper_f3088c();
                    }
                }
                xm1_power_record_helper_756dfc(profile().raw(), 0);
                let pending = if xm1_power_record_helper_74e7f0(profile().raw()) == 0 { 2 } else { 1 };
                put(STATE + 0x20, pending);
            }
            Configuration::Mode3 => put(STATE + 0x24, 1),
            Configuration::Mode9 => {
                if xm1_power_record_helper_fa8058() == 1 { set_modes(2, 2, 2); } else { set_modes(0, 2, 0); }
                xm1_power_record_helper_fa8038(0);
                xm1_power_record_helper_fa7ff8(1);
            }
            Configuration::Mode10 => xm1_power_record_helper_fa7ff8(0),
            Configuration::Mode11 => {
                xm1_power_record_helper_740d50(4);
                if xm1_power_record_helper_74eb20(profile().raw()) == 0 { xm1_power_record_helper_eecd94(); }
                let mut snapshot = ProfileSnapshot([MaybeUninit::uninit(); 48]);
                xm1_power_record_helper_756840(profile().raw(), snapshot.0.as_mut_ptr().cast());
                if core::ptr::read(snapshot.0.as_ptr().add(32).cast::<u32>()) == 0 { xm1_power_record_helper_f46e6c(profile().raw()); }
                let first = if xm1_power_record_helper_74e5c0(profile().raw()) == 0 { 0 } else { 2 };
                set_modes(first, 2, 1);
                if xm1_power_record_helper_758340(profile().raw()) == 1 {
                    xm1_power_record_helper_f30828();
                    xm1_power_record_helper_f3088c();
                }
                put(STATE + 0x20, 1);
            }
            Configuration::Mode13 => {
                for (mask, pending) in [(0x20u64, 2), (2, 3), (0x800_0000_0000, 5), (8, 4), (0x400_0000_0000, 2)] {
                    if xm1_power_record_helper_74e020(profile().raw(), mask) != 0 {
                        put(STATE + 0x24, pending);
                        break;
                    }
                }
                set_modes(2, 2, 2);
                put(STATE + 0x20, 4);
            }
            Configuration::Mode23 => {
                const REQUEST: usize = 0x0169_d22c;
                if word(REQUEST + 4) & 1 != 0 { xm1_power_record_helper_756a0c(profile().raw(), 1); }
                if word(REQUEST + 4) & 4 != 0 { xm1_power_record_helper_756a54(profile().raw(), 1); }
                if word(REQUEST + 4) & 32 != 0 { xm1_power_record_helper_756a9c(profile().raw(), 1); }
                if word(REQUEST + 4) & 8 != 0 { xm1_power_record_helper_756dfc(profile().raw(), 0); }
            }
            Configuration::Mode29 => {
                xm1_power_record_helper_756a0c(profile().raw(), 2);
                xm1_power_record_helper_756a54(profile().raw(), 2);
                put(STATE + 0x20, 3);
            }
            Configuration::Mode34 => {
                if xm1_power_record_helper_f2fcd0() != 0 { put(STATE + 0x24, 2); }
                else { set_modes(2, 2, 2); }
            }
            Configuration::Other => set_modes(2, 2, 1),
        }
    }
}

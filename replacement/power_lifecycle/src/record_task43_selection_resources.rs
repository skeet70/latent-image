//! X-M1 1.50 selection resources, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.
//! NONMATCHING declarations are interpreted against image-5 ARM bytes.

use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Address(u32);
impl Address {
    fn offset(self, bytes: u32) -> Self { Self(self.0.wrapping_add(bytes)) }
    unsafe fn word(self) -> u32 { unsafe { read_volatile(self.0 as *const u32) } }
    unsafe fn byte(self) -> u8 { unsafe { read_volatile(self.0 as *const u8) } }
    unsafe fn set(self, value: u32) { unsafe { write_volatile(self.0 as *mut u32, value) } }
    unsafe fn pointer(self) -> Self { Self(unsafe { self.word() }) }
}
const REQUEST: Address = Address(0x0169d09c);
const CONTEXT: Address = Address(0x0169d0d8);
const VALUE: Address = Address(0x016b7510);

unsafe extern "C" {
    fn xm1_power_record_helper_ef9a94(event: u32, data: *const u8, bytes: u32);
    fn xm1_power_record_helper_efb104(kind: u32, item: u32, enabled: u32, flags: u32);
    fn xm1_power_record_helper_756734(item: u32, output: *mut u8);
    fn xm1_power_record_helper_756754(item: u32, input: *const u8);
    fn xm1_power_record_helper_69f8fc(key: u32) -> u32;
    fn xm1_power_record_helper_74f2bc(item: u32) -> u32;
    fn xm1_power_record_helper_74f1ac(item: u32) -> u32;
    fn xm1_power_record_helper_750474(item: u32) -> u32;
    fn xm1_power_record_helper_f49138(output: u32, enabled: u32);
    fn xm1_power_record_helper_1039dd4(enabled: u32, mode: u32, callback: unsafe extern "C" fn(u32));
    fn xm1_power_record_helper_efab18(item: u32, mode: u32);
    fn xm1_power_record_helper_7409f0();
    fn xm1_power_record_helper_72872c(event: u32);
    fn xm1_power_record_helper_efb04c(group: u32);
    fn xm1_power_record_helper_efb004(group: u32);
    fn xm1_power_record_helper_10029bc();
    fn xm1_power_record_helper_f2e600(main: *mut u8, first: *mut u8, second: *mut u8, extra: *mut u8);
    fn xm1_power_record_helper_1039b48(main: *const u8, first: *const u8, second: *const u8, extra: *const u8, callback: unsafe extern "C" fn(u32, *const u32));
    fn xm1_power_record_helper_7585dc(item: u32, values: u32);
    fn xm1_power_record_helper_10029d8();
    fn xm1_power_record_helper_efae88(item: u32, ready: u32);
    fn xm1_power_record_helper_75c874(item: u32) -> u32;
    fn xm1_power_record_helper_75c8a4(item: u32) -> u32;
    fn xm1_power_record_helper_75a4ac(item: u32, index: u32, output: *mut u32);
    fn xm1_power_record_helper_76d9c8() -> u32;
    fn xm1_power_record_helper_f17b1c() -> i32;
    fn xm1_power_record_helper_74e7f0(item: u32) -> u32;
    fn xm1_power_record_helper_f2f480(value: u32);
    fn xm1_power_record_helper_f2f440(value: u32);
    fn xm1_power_record_helper_f2f00c(base: u32, value: u32, reference: u32, output: *mut u32) -> i32;
    fn xm1_power_record_helper_746a80(value: u32, reference: u32) -> u32;
    fn xm1_power_record_helper_f2f400(record: *const Adjustment);
    fn xm1_power_record_helper_f17af8(value: u32);
}

#[repr(C)]
struct Adjustment { changed: u32, values: [u32; 3], positions: [u16; 4] }

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f29000() {
    unsafe {
        let context = Address(0x169d1c8);
        xm1_power_record_helper_efb104(18, Address(0x169d384).word(), u32::from(context.offset(16).word() != 1), Address(0x169d38c).word());
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2903c(item: u32, values: *const u32) {
    let mut message = [0u32; 6];
    message[0] = item;
    for index in 0..5 { message[index + 1] = unsafe { read_volatile(values.add(index)) }; }
    unsafe { xm1_power_record_helper_ef9a94(0x85, message.as_ptr().cast(), 24) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2907c(value: u32) {
    unsafe { xm1_power_record_helper_ef9a94(0x83, (&value as *const u32).cast(), 4) }
}

// The stock extent 0xf290a4..0xf290b8 changes no application state.
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_f290a4() {}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f290b8(item: u32) { unsafe { VALUE.set(item) } }

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f290d8() {
    unsafe {
        let output = REQUEST.offset(12).pointer().offset(80).word();
        let enabled = xm1_power_record_helper_750474(VALUE.word());
        if xm1_power_record_helper_69f8fc(0xa8) & 128 != 0 { xm1_power_record_helper_f49138(output, enabled); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f29120() {
    let mut record = core::mem::MaybeUninit::<[u32; 34]>::uninit();
    unsafe {
        xm1_power_record_helper_756734(VALUE.word(), record.as_mut_ptr().cast());
        let words = record.as_mut_ptr().cast::<u32>();
        words.add(1).write(0);
        words.add(26).write(0);
        xm1_power_record_helper_756754(VALUE.word(), record.as_ptr().cast());
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f29160() {
    unsafe { xm1_power_record_helper_1039dd4(u32::from(REQUEST.offset(4).word() & 64 != 0), 1, xm1_power_record_helper_f2907c) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f29194() -> i32 {
    unsafe {
        if REQUEST.offset(8).word() == 3 { return 0; }
        if xm1_power_record_helper_69f8fc(0xa8) & 1 != 0 {
            let mode = xm1_power_record_helper_69f8fc(0xb1);
            return i32::from(mode <= 7 && (1u32 << mode) & 0xf2 != 0);
        }
        if xm1_power_record_helper_74f2bc(VALUE.word()) == 1 || Address(0x16b7660).word() == 0 { return 0; }
        i32::from(xm1_power_record_helper_74f1ac(VALUE.word()) != 7)
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f29240() -> u32 { unsafe { VALUE.offset(4).word() } }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f29260() { unsafe { VALUE.offset(8).set(0) } }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f29284() -> u32 { unsafe { VALUE.offset(8).word() } }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f292a4() { unsafe { xm1_power_record_helper_efab18(VALUE.word(), 7) } }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f292c4() {
    unsafe { xm1_power_record_helper_efab18(VALUE.word(), 0); xm1_power_record_helper_7409f0(); xm1_power_record_helper_72872c(0x13000000); }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f292f0() { unsafe { xm1_power_record_helper_efb04c(21) } }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f29304() { unsafe { xm1_power_record_helper_efb004(21) } }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2a11c() { unsafe { xm1_power_record_helper_10029bc() } }

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2a12c() {
    let mut main = core::mem::MaybeUninit::<[u32; 15]>::uninit();
    let mut first = core::mem::MaybeUninit::<[u32; 4]>::uninit();
    let mut second = core::mem::MaybeUninit::<[u32; 4]>::uninit();
    let mut extra = core::mem::MaybeUninit::<[u32; 14]>::uninit();
    unsafe {
        xm1_power_record_helper_f2e600(main.as_mut_ptr().cast(), first.as_mut_ptr().cast(), second.as_mut_ptr().cast(), extra.as_mut_ptr().cast());
        let destination = main.as_mut_ptr().cast::<u32>();
        for (index, offset) in [36u32, 4, 12, 16, 8, 32].into_iter().enumerate() {
            let value = if index == 0 { CONTEXT.offset(offset).pointer().offset(4).word() } else { CONTEXT.offset(offset).word() };
            destination.add(index + 8).write(value);
        }
        let bytes = extra.as_mut_ptr().cast::<u8>();
        let source = CONTEXT.offset(24).pointer();
        for index in 0..6 { bytes.add(36 + index).write(source.offset(index as u32).byte()); }
        bytes.add(52).write(CONTEXT.offset(28).pointer().offset(4).byte());
        xm1_power_record_helper_1039b48(main.as_ptr().cast(), first.as_ptr().cast(), second.as_ptr().cast(), extra.as_ptr().cast(), xm1_power_record_helper_f2903c);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2a1cc() {
    unsafe { xm1_power_record_helper_7585dc(VALUE.word(), 0x169d384); xm1_power_record_helper_10029d8(); xm1_power_record_helper_efae88(VALUE.word(), 0); }
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_f2a200(count: u32, limit: u32) -> u32 {
    if count == 0 || limit == 0 || limit > 3 || count > limit { 1 } else { count }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2a240() -> i32 {
    unsafe {
        let item = VALUE.word();
        let selection = REQUEST.offset(56).pointer();
        let count = selection.byte() as u32;
        let limit = selection.offset(1).byte() as u32;
        let mut result = 0;
        if xm1_power_record_helper_f2a200(count, limit) == 1 && count == 1 && REQUEST.offset(8).word() != 3 {
            let mut values = core::mem::MaybeUninit::<[u32; 5]>::uninit();
            let words = values.as_mut_ptr().cast::<u32>();
            let mut index = 0u8;
            while index < selection.offset(1).byte() {
                words.add(index as usize).write(xm1_power_record_helper_75c874(REQUEST.offset(20).pointer().offset(index as u32 * 4).word()));
                index = index.wrapping_add(1);
            }
            xm1_power_record_helper_75a4ac(item, 0, words.add(3));
            let reference = xm1_power_record_helper_76d9c8();
            result = xm1_power_record_helper_f17b1c();
            let mut cache = 0;
            if xm1_power_record_helper_74e7f0(item) == 2 {
                let mut pair = core::mem::MaybeUninit::<[u32; 2]>::uninit();
                xm1_power_record_helper_75a4ac(item, 4, pair.as_mut_ptr().cast());
                if pair.as_ptr().cast::<u32>().read() != reference { cache = 1; result = 1; }
            }
            xm1_power_record_helper_f2f480(0);
            xm1_power_record_helper_f2f440(cache);
            let mut record = core::mem::MaybeUninit::<Adjustment>::uninit();
            let record_ptr = record.as_mut_ptr();
            core::ptr::addr_of_mut!((*record_ptr).changed).write(0);
            index = 0;
            while index < selection.offset(1).byte() {
                core::ptr::addr_of_mut!((*record_ptr).values).cast::<u32>().add(index as usize).write(words.add(index as usize).read());
                core::ptr::addr_of_mut!((*record_ptr).positions).cast::<u16>().add(index as usize).write(xm1_power_record_helper_75c8a4(REQUEST.offset(20).pointer().offset(index as u32 * 4).word()) as u16);
                index = index.wrapping_add(1);
            }
            if result == 0 {
                index = 0;
                while index < selection.offset(1).byte() {
                    let old = words.add(index as usize).read();
                    let mut pair = core::mem::MaybeUninit::<[u32; 2]>::uninit();
                    if xm1_power_record_helper_f2f00c(words.add(3).read(), old, reference, pair.as_mut_ptr().cast()) != 0 {
                        core::ptr::addr_of_mut!((*record_ptr).changed).write(0); result = 1; break;
                    }
                    let adjusted = pair.as_ptr().cast::<u32>().read();
                    if old != adjusted {
                        core::ptr::addr_of_mut!((*record_ptr).changed).write(1);
                        core::ptr::addr_of_mut!((*record_ptr).values).cast::<u32>().add(index as usize).write(adjusted);
                        core::ptr::addr_of_mut!((*record_ptr).positions).cast::<u16>().add(index as usize).write(xm1_power_record_helper_746a80(adjusted, reference) as u16);
                        result = 1;
                    }
                    index = index.wrapping_add(1);
                }
            }
            xm1_power_record_helper_f2f400(record_ptr);
        }
        xm1_power_record_helper_f17af8(0);
        result
    }
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_f2ab5c(event: u32) -> i32 { if event == 127 { 2 } else { 3 } }

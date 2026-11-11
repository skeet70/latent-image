//! X-M1 1.50: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; id5 f3b5b0..f3c17c.

use core::ptr::{read_volatile, write_volatile};

type StatusCallback = unsafe extern "C" fn(u32);
type PairCallback = unsafe extern "C" fn(u32, u32);
type ExtendedCallback = unsafe extern "C" fn(u32, u32, u32, u32, u32, *const u8, u32);

#[repr(C)]
pub struct CallbackPayload { pub words: [u32; 257] }

unsafe extern "C" {
    fn xm1_power_record_helper_f3b568(output: *mut u8);
    fn xm1_power_record_helper_f3b5f0(output: *mut u8);
    fn xm1_power_record_helper_f3b6c8(output: *mut u8);
    fn xm1_power_record_helper_f3b72c(output: *mut u8);
    fn xm1_power_record_helper_f3b7a0(output: *mut u8);
    fn xm1_power_record_helper_f3b9d8(output: *mut u8);
    fn xm1_power_record_helper_f3bb70(output: *mut u8);
    fn xm1_power_record_helper_f3be7c(output: *mut u8);
    fn xm1_power_record_helper_f3a768(first: u32, second: u32);
    fn xm1_power_record_helper_f3a794(status: u32);
    fn xm1_power_record_helper_f3a7bc(status: u32);
    fn xm1_power_record_helper_f3a7e4(status: u32);
    fn xm1_power_record_helper_f3a80c(status: u32);
    fn xm1_power_record_helper_f3a834(status: u32);
    fn xm1_power_record_helper_f3a85c(status: u32);
    fn xm1_power_record_helper_f3a884(status: u32);
    fn xm1_power_record_helper_f3a8ac(status: u32);
    fn xm1_power_record_helper_f3a8fc(status: u32);
    fn xm1_power_record_helper_f3a924(first: u32, second: u32);
    fn xm1_power_record_helper_f3a950(status: u32);
    fn xm1_power_record_helper_f3a9a0(status: u32);
    fn xm1_power_record_helper_f3a9c8(status: u32);
    fn xm1_power_record_helper_f3aa18(status: u32);
    fn xm1_power_record_helper_f3aa40(first: u32, second: u32, third: u32, fourth: u32, fifth: u32, row: *const u8, tail: u32);
    fn xm1_power_record_helper_efb934(status: u32, value: u32);
    fn xm1_power_record_helper_740d50(value: u32);
    fn xm1_power_record_helper_69f8fc(key: u32) -> u32;
    fn xm1_power_record_helper_72872c(value: u32);
    fn xm1_power_record_helper_ef9a94(event: u32, payload: *const u8, length: u32);
    fn xm1_power_record_helper_74e520(resource: u32) -> u32;
    fn xm1_power_record_helper_74eb20(resource: u32) -> u32;
    fn xm1_power_record_helper_75569c(resource: u32) -> u32;
    fn xm1_power_record_helper_75573c(resource: u32) -> Option<unsafe extern "C" fn(u32,u32,u32,u32,CallbackPayload)>;
    fn xm1_power_record_helper_75540c(resource: u32) -> u32;
    fn xm1_power_record_helper_7554ac(resource: u32) -> u32;
    fn xm1_power_record_helper_75554c(resource: u32) -> u32;
    fn xm1_power_record_helper_7555ec(output: *mut u32, resource: u32);
    fn xm1_power_record_helper_6a7e68() -> u32;
    fn xm1_power_record_helper_758154(resource: u32, mode: u32);
    fn xm1_power_record_helper_74fc64(resource: u32) -> u32;
    fn xm1_power_record_helper_f5206c(output: *mut u32, resource: u32);
    fn xm1_power_record_helper_103a00c(record: *const u8, callback: StatusCallback, pair: PairCallback);
    fn xm1_power_record_helper_103a254(record: *const u8, callback: StatusCallback);
    fn xm1_power_record_helper_103a2c0(record: *const u8, callback: StatusCallback);
    fn xm1_power_record_helper_103a404(record: *const u8, callback: StatusCallback);
    fn xm1_power_record_helper_103a32c(record: *const u8, callback: StatusCallback);
    fn xm1_power_record_helper_103a594(record: *const u32, value: u32, callback: StatusCallback, snapshot: *const u8);
    fn xm1_power_record_helper_103a398(record: *const u8, callback: StatusCallback);
    fn xm1_power_record_helper_103a13c(record: *const u8, callback: StatusCallback);
    fn xm1_power_record_helper_103a694(value: u32, record: *mut u8, first: StatusCallback, enabled: u32, second: StatusCallback);
    fn xm1_power_record_helper_103a708(callback: PairCallback, value: u32, record: *const u32, ratio: *const u32, tail: u32);
    fn xm1_power_record_helper_1039c7c(descriptor: *const u8, dimensions: *const u8, extended: *const u8, mode: *const u8, snapshot: *const u8, channel: *const u8, callback: *const u8, enabled: u32, completion: StatusCallback);
    fn xm1_power_record_helper_1039638(descriptor: *const u8, dimensions: *const u8, extended: *const u8, mode: *const u8, snapshot: *const u8, channel: *const u8, callback: *const u8, enabled: u32, value: u32, completion: StatusCallback);
    fn xm1_power_record_helper_1039e48(descriptor: *const u8, dimensions: *const u8, extended: *const u8, mode: *const u8, snapshot: *const u8, channel: *const u8, callback: *const u8, enabled: u32, selection: *const u32, value: u32, first: u32, second: u32, completion: StatusCallback, secondary: StatusCallback, extended_completion: ExtendedCallback);
}

unsafe fn resource() -> u32 { unsafe { read_volatile(0x016c0718 as *const u32) } }

#[repr(C, align(4))]
struct CommandRecords {
    descriptor: [u8; 16],
    dimensions: [u8; 24],
    extended: [u8; 16],
    mode: [u8; 16],
    snapshot: [u8; 56],
    channel: [u8; 56],
    callback: [u8; 12],
}

impl CommandRecords {
    unsafe fn capture() -> Self {
        unsafe {
            let mut records = Self { descriptor: [0;16], dimensions: [0;24], extended: [0;16], mode: [0;16], snapshot: [0;56], channel: [0;56], callback: [0;12] };
            xm1_power_record_helper_f3b568(records.descriptor.as_mut_ptr());
            xm1_power_record_helper_f3b5f0(records.dimensions.as_mut_ptr());
            xm1_power_record_helper_f3b6c8(records.extended.as_mut_ptr());
            xm1_power_record_helper_f3b72c(records.mode.as_mut_ptr());
            xm1_power_record_helper_f3b7a0(records.snapshot.as_mut_ptr());
            xm1_power_record_helper_f3b9d8(records.channel.as_mut_ptr());
            xm1_power_record_helper_f3bb70(records.callback.as_mut_ptr());
            records
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b5b0() {
    unsafe {
        let mut descriptor = [0u32;4];
        xm1_power_record_helper_f3b568(descriptor.as_mut_ptr().cast());
        descriptor[3] = read_volatile(0x0169d1fc as *const u32);
        xm1_power_record_helper_103a00c(descriptor.as_ptr().cast(), xm1_power_record_helper_f3a950, xm1_power_record_helper_f3a924);
    }
}

macro_rules! submit_record {
    ($name:ident, $capture:ident, $size:expr, $submit:ident, $callback:ident) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name() {
            unsafe {
                let mut record = [0u32; $size / 4];
                $capture(record.as_mut_ptr().cast());
                $submit(record.as_ptr().cast(), $callback);
            }
        }
    };
}

submit_record!(xm1_power_record_helper_f3b69c, xm1_power_record_helper_f3b5f0, 24, xm1_power_record_helper_103a254, xm1_power_record_helper_f3a834);
submit_record!(xm1_power_record_helper_f3b700, xm1_power_record_helper_f3b6c8, 16, xm1_power_record_helper_103a2c0, xm1_power_record_helper_f3a85c);
submit_record!(xm1_power_record_helper_f3b774, xm1_power_record_helper_f3b72c, 16, xm1_power_record_helper_103a404, xm1_power_record_helper_f3a884);
submit_record!(xm1_power_record_helper_f3b930, xm1_power_record_helper_f3b7a0, 56, xm1_power_record_helper_103a32c, xm1_power_record_helper_f3a80c);
submit_record!(xm1_power_record_helper_f3bb44, xm1_power_record_helper_f3b9d8, 56, xm1_power_record_helper_103a398, xm1_power_record_helper_f3a7e4);
submit_record!(xm1_power_record_helper_f3bed8, xm1_power_record_helper_f3be7c, 28, xm1_power_record_helper_103a13c, xm1_power_record_helper_f3a8fc);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3b95c() {
    unsafe {
        let mut descriptor = [0u32;7];
        descriptor[0] = xm1_power_record_helper_75540c(resource());
        descriptor[1] = xm1_power_record_helper_7554ac(resource());
        descriptor[2] = xm1_power_record_helper_75554c(resource());
        xm1_power_record_helper_7555ec(descriptor.as_mut_ptr().add(3), resource());
        let mut snapshot = [0u32;14];
        xm1_power_record_helper_f3b7a0(snapshot.as_mut_ptr().cast());
        xm1_power_record_helper_103a594(descriptor.as_ptr(), 0, xm1_power_record_helper_f3a8ac, snapshot.as_ptr().cast());
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3bb10(enabled: u32) {
    unsafe {
        let mut channel = [0u32;14];
        xm1_power_record_helper_f3b9d8(channel.as_mut_ptr().cast());
        channel[4] = enabled;
        xm1_power_record_helper_103a398(channel.as_ptr().cast(), xm1_power_record_helper_f3a7e4);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3bbac() {
    unsafe {
        write_volatile(0x016c0720 as *mut u32, 1);
        let records = CommandRecords::capture();
        let selection = 0x0169d100 as *const u32;
        let extra = [0, xm1_power_record_helper_74e520(resource()), 0];
        let value = read_volatile(selection.add(1));
        let first = read_volatile(selection.add(2));
        let second = read_volatile(selection.add(3));
        let mode = xm1_power_record_helper_74eb20(resource());
        let callback = xm1_power_record_helper_75569c(resource());
        xm1_power_record_helper_1039e48(records.descriptor.as_ptr(),records.dimensions.as_ptr(),records.extended.as_ptr(),records.mode.as_ptr(),records.snapshot.as_ptr(),records.channel.as_ptr(),records.callback.as_ptr(),u32::from((mode | callback) != 0),extra.as_ptr(),value,first,second,xm1_power_record_helper_f3a9c8,xm1_power_record_helper_f3a9a0,xm1_power_record_helper_f3aa40);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3bccc() {
    unsafe {
        write_volatile(0x016c0720 as *mut u32, 0);
        let records = CommandRecords::capture();
        let mode = xm1_power_record_helper_74eb20(resource());
        let callback = xm1_power_record_helper_75569c(resource());
        write_volatile(0x016c0724 as *mut u32, 1);
        xm1_power_record_helper_1039c7c(records.descriptor.as_ptr(),records.dimensions.as_ptr(),records.extended.as_ptr(),records.mode.as_ptr(),records.snapshot.as_ptr(),records.channel.as_ptr(),records.callback.as_ptr(),u32::from((mode | callback) != 0),xm1_power_record_helper_f3aa18);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3bd8c() {
    unsafe {
        write_volatile(0x016c0720 as *mut u32, 0);
        let records = CommandRecords::capture();
        let mode = xm1_power_record_helper_74eb20(resource());
        let callback = xm1_power_record_helper_75569c(resource());
        let enabled = u32::from((mode | callback) != 0);
        let alternate = xm1_power_record_helper_6a7e68();
        xm1_power_record_helper_758154(resource(), if alternate == 1 { 4 } else { 3 });
        let value = xm1_power_record_helper_74fc64(resource());
        write_volatile(0x016c0724 as *mut u32, 1);
        xm1_power_record_helper_1039638(records.descriptor.as_ptr(),records.dimensions.as_ptr(),records.extended.as_ptr(),records.mode.as_ptr(),records.snapshot.as_ptr(),records.channel.as_ptr(),records.callback.as_ptr(),enabled,value,xm1_power_record_helper_f3aa18);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3bf04(request: u32) { unsafe { write_volatile(0x016c0718 as *mut u32, request); } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3bf24(value: u32) { unsafe { write_volatile(0x016c0724 as *mut u32, value); } }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3bf44() {
    unsafe {
        let value = xm1_power_record_helper_75569c(resource());
        let mode = xm1_power_record_helper_74eb20(resource());
        xm1_power_record_helper_103a694(value,0x016c0728 as *mut u8,xm1_power_record_helper_f3a7bc,u32::from((mode | value) != 0),xm1_power_record_helper_f3a794);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3bfa4() {
    unsafe { xm1_power_record_helper_103a694(0,0x016c0728 as *mut u8,xm1_power_record_helper_f3a7bc,0,xm1_power_record_helper_f3a794); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3bfe0() {
    unsafe {
        if let Some(callback) = xm1_power_record_helper_75573c(resource()) {
            let mut snapshot = CallbackPayload { words: [0; 257] };
            core::ptr::copy_nonoverlapping(0x016c0738 as *const u32,snapshot.words.as_mut_ptr(),257);
            callback(read_volatile(0x016c0728 as *const u32),read_volatile(0x016c072c as *const u32),read_volatile(0x016c0730 as *const u32),read_volatile(0x016c0734 as *const u32),snapshot);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3c030() {
    unsafe {
        let selection = 0x0169d2e8 as *const u32;
        let mut ratio = [0u32;2];
        xm1_power_record_helper_f5206c(ratio.as_mut_ptr(),resource());
        xm1_power_record_helper_103a708(xm1_power_record_helper_f3a768,read_volatile(selection.add(1)),selection.add(2),ratio.as_ptr(),read_volatile(selection.add(4)));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3c08c(status: u32) {
    unsafe { xm1_power_record_helper_efb934(if status == 0 { read_volatile(0x0169d380 as *const u32) } else { (-94i32) as u32 },read_volatile(0x0169d384 as *const u32)); }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3c0f0() { unsafe { xm1_power_record_helper_740d50(4); } }


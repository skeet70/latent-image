use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy)]
pub(super) struct Profile(pub u32);
#[derive(Clone, Copy)]
pub(super) struct Address(pub u32);
impl Address {
    pub(super) fn offset(self, bytes: u32) -> Self { Self(self.0.wrapping_add(bytes)) }
    pub(super) unsafe fn word(self) -> u32 { unsafe { read_volatile(self.0 as *const u32) } }
    pub(super) unsafe fn set(self, value: u32) { unsafe { write_volatile(self.0 as *mut u32, value) } }
    pub(super) unsafe fn half(self) -> u16 { unsafe { read_volatile(self.0 as *const u16) } }
    pub(super) unsafe fn set_half(self, value: u16) { unsafe { write_volatile(self.0 as *mut u16, value) } }
    pub(super) unsafe fn byte(self) -> u8 { unsafe { read_volatile(self.0 as *const u8) } }
    pub(super) unsafe fn pointee(self) -> Self { Self(unsafe { self.word() }) }
}
pub(super) const CONTEXT: Address = Address(0x16a049c);
pub(super) const LISTS: Address = Address(0x169e580);
pub(super) unsafe fn profile() -> Profile { Profile(unsafe { CONTEXT.word() }) }
pub(super) unsafe fn owner() -> Address { unsafe { CONTEXT.offset(4).pointee() } }
pub(super) unsafe fn buffer() -> Address { unsafe { Address(ef9bc0()).offset(0x2c).pointee().pointee() } }

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct Geometry { pub words: [u32; 12] }
#[repr(C)]
#[derive(Default)]
pub(super) struct AlternateGeometry { pub words: [u32; 7] }
#[repr(C)]
pub(super) struct SelectionState { pub words: [u32; 33] }
impl Default for SelectionState {
    fn default() -> Self { Self { words: [0; 33] } }
}
#[repr(C)]
#[derive(Default)]
pub(super) struct SelectionHeader { pub words: [u32; 8] }
#[repr(C)]
#[derive(Default)]
pub(super) struct SelectionStatistics { pub words: [u32; 20] }
#[repr(C)]
#[derive(Default)]
pub(super) struct SharedRecords { pub present: u32, pub count: u32, pub records: u32 }

macro_rules! providers {
    ($($name:ident($($arg:ident: $ty:ty),*) $(-> $ret:ty)?;)+) => {
        unsafe extern "C" { $(#[link_name = concat!("xm1_power_record_helper_", stringify!($name))] pub(super) fn $name($($arg: $ty),*) $(-> $ret)?;)+ }
    };
}
providers! {
    ef9bc0() -> u32;
    ef9b50() -> u32;
    f160d0() -> u32;
    f160ac(value: u32);
    f16160(value: u32);
    f161f0();
    f1608c();
    f16664() -> u32;
    f162e4(output: u32, item: u32, source: u32, mode: u32, single: u32);
    f15f40(mode: u32);
    f1572c(mode: u32, operation: u32);
    eea030(channel: *mut u8, first: *mut u16, second: *mut u16);
    eea094() -> u32;
    ef53d0(index: u32) -> u32;
    ee8e70(output: u32);
    f4725c(first: u32, second: u32, third: u32) -> u32;
    f4727c();
    f472bc();
    f48e74(buffer: u32);
    f47468();
    f48f1c(buffer: u32);
    f48f54(buffer: u32);
    f48fd8(buffer: u32, value: u32);
    f4929c(buffer: u32);
    f612dc(item: u32, mode: u32) -> u32;
    f61584(item: u32, mode: u32) -> u32;
    f615bc(item: u32, mode: u32) -> u32;
    f1fd84();
    f2f400(input: *const u32);
    f2f440(value: u32);
    f2f480(value: u32);
}

unsafe extern "C" {
    #[link_name = "xm1_power_record_helper_105386c"]
    pub(super) fn clear_geometry_runtime();
}

macro_rules! accessors {
    ($($name:ident = $symbol:literal ($($arg:ident: $ty:ty),*) $(-> $ret:ty)?;)+) => {
        unsafe extern "C" { $(#[link_name = concat!("xm1_power_record_helper_", $symbol)] pub(super) fn $name($($arg: $ty),*) $(-> $ret)?;)+ }
    };
}
accessors! {
    combined = "746bf8"(channel: u32, value: u32) -> u32;
    resource42 = "74e5c0"(item: u32) -> u32;
    resource43 = "74e648"(item: u32) -> u32;
    resource46 = "74eba8"(item: u32) -> u32;
    resource5a = "74fa2c"(item: u32) -> u32;
    owner_setting = "750474"(item: u32) -> u32;
    mode_flag = "75093c"(item: u32) -> u32;
    mode_pair = "74ec30"(item: u32) -> u32;
    property = "69f8fc"(key: u32) -> u32;
    lens_flags = "76f340"() -> u32;
    lens_position = "76da84"() -> u32;
    lens_third = "76dad4"() -> u32;
    lens_second = "76dbb4"() -> u32;
    geometry_value = "749be4"(first: i32, second: u32, third: u32, flags: u32, fourth: u32) -> u32;
    runtime_mode = "740d3c"(value: u32);
    runtime_clear = "742a6c"(first: u32, second: u32);
    runtime_refresh = "740690"();
    set_owner_value = "74e1a4"(item: u32, value: u32);
    set_owner_record = "74e284"(item: u32, value: u32, index: u32);
    set_owner_geometry = "74e1f0"(item: u32, value: u32);
    slot_snapshot = "752928"(output: *mut u32, item: u32, slot: u32);
    slot_geometry = "7529e8"(item: u32, slot: u32, value: u32);
    slot_channel = "752a98"(item: u32, slot: u32, value: u32);
    slot_first = "752bf8"(item: u32, slot: u32, value: u32);
    slot_second = "752ca8"(item: u32, slot: u32, value: u32);
    slot_a = "752f68"(item: u32, slot: u32, value: u32);
    slot_b = "752d58"(item: u32, slot: u32, value: u32);
    slot_c = "753598"(item: u32, slot: u32, value: u32);
    slot_d = "7536f8"(item: u32, slot: u32, value: u32);
    slot_e = "7537a8"(item: u32, slot: u32, value: u32);
    slot_f = "753858"(item: u32, slot: u32, value: u32);
    slot_scale = "753648"(item: u32, slot: u32, value: u32);
    slot_g = "7539b8"(item: u32, slot: u32, value: u32);
    slot_h = "753a68"(item: u32, slot: u32, value: u32);
    slot_reset = "7535f0"(item: u32, slot: u32) -> u32;
    slot_status = "754568"(item: u32, slot: u32, value: u32);
    copy_slot = "75ae78"(destination: u32, source: *const u32);
    shared_a = "756fb8"(item: u32, value: u32);
    shared_b = "757004"(item: u32, value: u32);
    shared_c = "757050"(item: u32, value: u32);
    shared_d = "7570a0"(item: u32, value: u32);
    shared_e = "7570f0"(item: u32, value: u32);
    shared_f = "757140"(item: u32, value: u32);
    shared_g = "757190"(item: u32, value: u32);
    shared_h = "7571e0"(item: u32, value: u32);
    shared_i = "75722c"(item: u32, value: u32);
    shared_j = "757274"(item: u32, value: u32);
    shared_k = "7572c0"(item: u32, value: u32);
    shared_l = "7573f4"(item: u32, value: u32);
    read_geometry = "756840"(item: u32, output: *mut Geometry);
    write_geometry = "756878"(item: u32, input: *const Geometry);
    read_alternate = "758c68"(item: u32, output: *mut AlternateGeometry);
    read_state = "756734"(item: u32, output: *mut SelectionState);
    write_state = "756754"(item: u32, input: *const SelectionState);
    read_header = "756788"(item: u32, output: *mut SelectionHeader);
    write_header = "7567b8"(item: u32, input: *const SelectionHeader);
    read_statistics = "756f3c"(output: *mut SelectionStatistics);
    write_statistics = "756f60"(item: u32, input: *const SelectionStatistics);
    read_records = "7567ec"(item: u32, output: *mut SharedRecords);
    read_scratch = "7568b8"(item: u32, output: *mut u32);
    write_scratch = "7568f8"(item: u32, input: *const u32);
    publish_buffer = "7589e0"(item: u32, buffer: *const u32);
    clear_buffer = "758360"(item: u32, value: u32);
}

unsafe extern "C" {
    pub(super) fn xm1_power_recording_capture_trace_begin(operation: u32);
}

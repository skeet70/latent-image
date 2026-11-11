//! X-M1 1.50 recording queue and SAC/LAC dispatch, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.
//! Native entry points require mapped recording state and pinned transition data. Command callbacks and nondefault application callbacks must be live owned native functions.

#[path = "record_task43_events.rs"]
mod events;
pub use events::event_row;
#[path = "record_task43_common.rs"]
mod common;
#[path = "record_task43_initialization.rs"]
mod initialization;

use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Machine(u32);

impl Machine {
    pub const fn new(value: u32) -> Option<Self> {
        if value < 37 { Some(Self(value)) } else { None }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct CompletionStatus(pub i32);

impl CompletionStatus {
    pub const fn releases_command(self) -> bool { self.0 <= 0 || self.0 == 20 || self.0 == 21 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Continuation { Preserve, State(u16) }

pub const fn continuation(encoded: i8) -> Continuation {
    if encoded > 0 { Continuation::Preserve } else { Continuation::State(-(encoded as i16) as u16) }
}

#[repr(C)]
pub struct StorageMessage {
    pub code: u16,
    pub task: u8,
    pub destination: u8,
    pub length: u32,
    pub payload: u32,
}

macro_rules! machines {
    ($($name:ident),+ $(,)?) => {
        unsafe extern "C" { $(fn $name(handler: u32, event: u32, request: u32) -> i32;)+ }
        const MACHINES: [unsafe extern "C" fn(u32, u32, u32) -> i32; 37] = [$($name),+];
    };
}

machines!(
    xm1_power_record_task43_machine0, xm1_power_record_task43_machine1,
    xm1_power_record_task43_machine2, xm1_power_record_task43_machine3,
    xm1_power_record_task43_machine4, xm1_power_record_task43_machine5,
    xm1_power_record_task43_machine6, xm1_power_record_task43_machine7,
    xm1_power_record_task43_machine8, xm1_power_record_task43_machine9,
    xm1_power_record_task43_machine10, xm1_power_record_task43_machine11,
    xm1_power_record_task43_machine12, xm1_power_record_task43_machine13,
    xm1_power_record_task43_machine14, xm1_power_record_task43_machine15,
    xm1_power_record_task43_machine16, xm1_power_record_task43_machine17,
    xm1_power_record_task43_machine18, xm1_power_record_task43_machine19,
    xm1_power_record_task43_machine20, xm1_power_record_task43_machine21,
    xm1_power_record_task43_machine22, xm1_power_record_task43_machine23,
    xm1_power_record_task43_machine24, xm1_power_record_task43_machine25,
    xm1_power_record_task43_machine26, xm1_power_record_task43_machine27,
    xm1_power_record_task43_machine28, xm1_power_record_task43_machine29,
    xm1_power_record_task43_machine30, xm1_power_record_task43_machine31,
    xm1_power_record_task43_machine32, xm1_power_record_task43_machine33,
    xm1_power_record_task43_machine34, xm1_power_record_task43_machine35,
    xm1_power_record_task43_machine36,
);

unsafe extern "C" {
    fn xm1_pipeline_image_receive_message(channel: u32, message: *mut StorageMessage, timeout: i32) -> i32;
    fn xm1_pipeline_image_send_message(channel: u32, message: *const StorageMessage) -> i32;
    fn xm1_pipeline_image_current_task() -> u32;
    fn xm1_power_record_helper_f46c20();
    fn xm1_power_record_task43_prepare9(event: u32, request: u32);
    fn xm1_power_record_task43_prepare10(event: u32, request: u32);
    fn xm1_power_record_task43_prepare11(event: u32, request: u32);
    fn xm1_power_record_task43_prepare12(event: u32, request: u32);
    fn xm1_power_record_task43_prepare13(event: u32, request: u32);
    fn xm1_power_record_task43_prepare24(event: u32, request: u32);
    fn xm1_power_record_task43_prepare25(event: u32, request: u32);
    fn xm1_power_record_task43_prepare26(event: u32, request: u32);
    fn xm1_power_record_task43_prepare29(event: u32, request: u32);
    fn xm1_power_record_task43_prepare30(event: u32, request: u32);
}

fn word(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
fn half(address: usize) -> u16 { unsafe { read_volatile(address as *const u16) } }
fn byte(address: usize) -> u8 { unsafe { read_volatile(address as *const u8) } }
fn put_word(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) }; }
fn put_half(address: usize, value: u16) { unsafe { write_volatile(address as *mut u16, value) }; }

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_dispatch(machine: u32, event: u32, request: u32) -> i32 {
    let Some(machine) = Machine::new(machine) else { return 0; };
    let index = machine.0 as usize;
    let table = word(0xf12f70 + index * 4) as usize;
    let handlers = word(0xf133ec + index * 4) as usize;
    let row = half(0x169d468 + index * 2);
    let width = half(0xf1158c + index * 2);
    let state = half(0x169d4b4 + index * 2);
    if table == 0 { return 0; }
    let cell = row.wrapping_mul(width).wrapping_add(state) as usize;
    let selector = byte(table + cell * 2) as usize;
    let result = if word(handlers + selector * 4) == 0 { 0 } else {
        unsafe { MACHINES[index](selector as u32, event, request) }
    };
    // Every positive continuation in the pinned tables selects the null action at index one.
    if let Continuation::State(next) = continuation(byte(table + cell * 2 + 1) as i8) {
        put_half(0x169d4b4 + index * 2, next);
    }
    result
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_route(machine: u32, row: u32, event: u32, request: u32) -> i32 {
    if Machine::new(machine).is_none() { return 0; }
    put_half(0x169d468 + machine as usize * 2, row as u16);
    unsafe { xm1_power_record_task43_dispatch(machine, event, request) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_enabled(command: u32, enabled: u32) {
    unsafe { write_volatile((0x169cf24 + command as usize) as *mut u8, u8::from(enabled != 0)) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_reset() {
    for machine in 0..37 { put_half(0x169d4b4 + machine * 2, 0); }
}

fn complete(command: u32, status: CompletionStatus) {
    if status.releases_command() { unsafe { xm1_power_record_task43_enabled(command, 0) }; }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_complete0(status: i32) {
    let callback = word(0x169cf58);
    complete(0, CompletionStatus(status));
    if callback != 0 {
        let callback: unsafe extern "C" fn(i32) = unsafe { core::mem::transmute(callback as usize) };
        unsafe { callback(status) };
    }
}

/// # Safety
/// Command one must contain a null or live native callback with the status-only ABI.
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_complete1(status: i32) {
    let callback = word(0x169d000);
    complete(1, CompletionStatus(status));
    if callback != 0 {
        let callback: unsafe extern "C" fn(i32) = unsafe { core::mem::transmute(callback as usize) };
        unsafe { callback(status) };
    }
}

/// # Safety
/// Command four must contain a null or live native callback with the four-word ABI.
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_complete4(status: i32, first: u32, second: u32, third: u32) {
    let callback = word(0x169d038);
    complete(4, CompletionStatus(status));
    if callback != 0 {
        let callback: unsafe extern "C" fn(i32, u32, u32, u32) = unsafe { core::mem::transmute(callback as usize) };
        unsafe { callback(status, first, second, third) };
    }
}

/// # Safety
/// Command five must contain a null or live native callback with the two-word ABI.
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_complete5(status: i32, value: u32) {
    let callback = word(0x169d050);
    complete(5, CompletionStatus(status));
    if callback != 0 {
        let callback: unsafe extern "C" fn(i32, u32) = unsafe { core::mem::transmute(callback as usize) };
        unsafe { callback(status, value) };
    }
}

fn complete_history(command: u32, storage: usize, inserted_offset: usize, status: u32, first: u32, second: u32, third: u32, fourth: u32) {
    let callback = word(storage);
    let inserted = word(storage + inserted_offset);
    complete(command, CompletionStatus(status as i32));
    if callback != 0 {
        let callback: unsafe extern "C" fn(u32, u32, u32, u32, u32, u32) = unsafe { core::mem::transmute(callback as usize) };
        unsafe { callback(status, first, second, inserted, third, fourth) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_efa258(status: u32, first: u32, second: u32, third: u32, fourth: u32) {
    complete_history(2, 0x169d004, 24, status, first, second, third, fourth);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_efa2d0(status: u32, first: u32, second: u32, third: u32, fourth: u32) {
    complete_history(3, 0x169d020, 20, status, first, second, third, fourth);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_event(machine: u32, event: u32, request: u32) -> i32 {
    let Some(row) = events::event_row(machine, event) else { return 0; };
    unsafe {
        match machine {
            9 => xm1_power_record_task43_prepare9(event, request),
            10 => xm1_power_record_task43_prepare10(event, request),
            11 => xm1_power_record_task43_prepare11(event, request),
            12 => xm1_power_record_task43_prepare12(event, request),
            13 => xm1_power_record_task43_prepare13(event, request),
            24 => xm1_power_record_task43_prepare24(event, request),
            25 => xm1_power_record_task43_prepare25(event, request),
            26 => xm1_power_record_task43_prepare26(event, request),
            29 => xm1_power_record_task43_prepare29(event, request),
            30 => xm1_power_record_task43_prepare30(event, request),
            _ => (),
        }
        xm1_power_record_task43_route(machine, u32::from(row), event, request)
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_ef9a94(command: u32, payload: u32, length: u32) {
    let source = unsafe { xm1_pipeline_image_current_task() } as u8;
    let packet = StorageMessage {
        code: (command as u16).wrapping_add(0x3b00),
        task: if source == 0 { 0xff } else { source },
        destination: 43,
        length,
        payload,
    };
    unsafe { xm1_pipeline_image_send_message(43, &packet) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_consume(message: *const StorageMessage) -> i32 {
    let message = unsafe { &*message };
    let event = u32::from(message.code).wrapping_sub(0x3b00);
    if event < 46 {
        let destination = word(0xefbb20 + event as usize * 4) as usize;
        for offset in 0..message.length as usize {
            let value = byte(message.payload as usize + offset);
            unsafe { write_volatile((destination + offset) as *mut u8, value) };
        }
        unsafe { xm1_power_record_task43_enabled(event, 1) };
    }
    unsafe { xm1_power_record_task43_event(0, event, u32::MAX) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_task43_run() -> ! {
    unsafe { xm1_power_record_task43_reset() };
    loop {
        put_word(0x169d37c, 0x169d380);
        let message = 0x169d374 as *mut StorageMessage;
        let status = unsafe { xm1_pipeline_image_receive_message(43, message, -1) };
        if status != 0 { unsafe { xm1_power_record_helper_f46c20() }; }
        unsafe { xm1_power_record_task43_consume(message) };
    }
}

#[cfg(test)]
mod tests {
    use super::{continuation, event_row, CompletionStatus, Continuation};

    #[test]
    fn completion_releases_only_terminal_statuses() {
        for status in [-94, -1, 0, 20, 21] { assert!(CompletionStatus(status).releases_command()); }
        for status in [1, 2, 19, 22, i32::MAX] { assert!(!CompletionStatus(status).releases_command()); }
    }

    #[test]
    fn continuation_preserves_state_or_selects_absolute_state() {
        assert_eq!(continuation(1), Continuation::Preserve);
        assert_eq!(continuation(0), Continuation::State(0));
        assert_eq!(continuation(-31), Continuation::State(31));
        assert_eq!(continuation(-128), Continuation::State(128));
    }

    #[test]
    fn grouped_events_keep_explicit_selector_precedence() {
        assert_eq!(event_row(12, 0x44), Some(39));
        assert_eq!(event_row(12, 0x45), Some(46));
        assert_eq!(event_row(12, 0x65), Some(45));
        assert_eq!(event_row(12, 0x66), Some(46));
        assert_eq!(event_row(12, 0x77), Some(47));
        assert_eq!(event_row(12, 0x78), Some(48));
        assert_eq!(event_row(12, 0x9f), Some(49));
        assert_eq!(event_row(12, 0xa1), Some(50));
    }

    #[test]
    fn command_and_internal_events_reach_distinct_rows() {
        assert_eq!(event_row(0, 0x3c), Some(40));
        assert_eq!(event_row(0, 0xffff), Some(47));
        assert_eq!(event_row(4, 0x107), Some(0));
        assert_eq!(event_row(32, 0xcf), Some(3));
        assert_eq!(event_row(34, 0x102), Some(5));
        assert_eq!(event_row(37, 0), None);
    }
}

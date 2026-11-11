#![cfg(target_arch = "arm")]

use core::ptr::{read_volatile, write_volatile};

const RECORDING: usize = 0x01526a78;
const RESULTS: usize = 0x01545850;
const PREFIX: usize = 0x01545790;

extern "C" {
    fn xm1_power_recording_defer_current() -> i32;
    fn xm1_power_recording_send_task_message(operation: u32, size: u32, payload: *const u8) -> i32;
    fn xm1_power_recording_report_missing_state();
    fn xm1_power_recording_repeat_current_notification() -> i32;
    fn xm1_power_recording_capture_prepare_resource();
    fn xm1_power_recording_capture_start_resource(callback: extern "C" fn(u32));
    fn xm1_power_recording_capture_acknowledge_resource();
    fn xm1_power_recording_capture_settings_request(kind: u32, settings: u32, requested: u32, selected: u32) -> i32;
    fn xm1_power_recording_capture_parameter_request(kind: u32, settings: u32) -> i32;
    fn xm1_power_recording_capture_parameter_class(settings: u32) -> i32;
    fn xm1_power_recording_output_request(kind: u32, settings: u32) -> i32;
    fn xm1_power_recording_capture_aux_request(kind: u32, argument: u32) -> i32;
    fn xm1_power_recording_capture_motor_request(kind: u32, argument: u32, output: u32) -> i32;
    fn xm1_power_recording_capture_slot_request(request: *const u32, output: u32) -> i32;
    fn xm1_power_recording_capture_select_changed(options: u32);
    fn xm1_power_recording_capture_conversion_changed(first: u32, second: u32) -> i32;
    fn xm1_power_recording_capture_conversion_limit() -> u32;
    fn xm1_power_recording_capture_trace_begin(operation: u32);
    fn xm1_power_recording_capture_trace_end(operation: u32);
}

#[derive(Clone, Copy)]
#[repr(u16)]
enum CaptureState {
    Idle = 0,
    Starting = 1,
    Settings = 2,
    Auxiliary = 3,
    ParameterWait = 4,
    MotorWait = 5,
    ParameterRetry = 6,
    Finishing = 7,
    SettingsRetry = 8,
}

impl CaptureState {
    fn read() -> Option<Self> {
        match unsafe { read_volatile((RESULTS + 0xb4) as *const u16) } {
            0 => Some(Self::Idle), 1 => Some(Self::Starting), 2 => Some(Self::Settings),
            3 => Some(Self::Auxiliary), 4 => Some(Self::ParameterWait), 5 => Some(Self::MotorWait),
            6 => Some(Self::ParameterRetry), 7 => Some(Self::Finishing), 8 => Some(Self::SettingsRetry),
            _ => None,
        }
    }
}

fn load(offset: usize) -> u32 { unsafe { read_volatile((RECORDING + offset) as *const u32) } }
fn store(offset: usize, value: u32) { unsafe { write_volatile((RECORDING + offset) as *mut u32, value) }; }
fn flags() -> u32 { load(0x9384) }
fn add_flags(bits: u32) { store(0x9384, flags() | bits); }
fn address(offset: usize) -> u32 { (RECORDING + offset) as u32 }
fn set_state(state: CaptureState) { store(0x81f4, state as u32); }
fn result_state(state: CaptureState) {
    unsafe { write_volatile((RESULTS + 0xb4) as *mut u16, state as u16) };
}
fn transition(state: CaptureState) {
    result_state(state);
    set_state(state);
}
fn event(value: u16) { unsafe { write_volatile((PREFIX + 0xb4) as *mut u16, value) }; }
fn trace(operation: u32, begin: bool) {
    if unsafe { read_volatile(0x00dca960 as *const u32) } == 1 {
        unsafe {
            if begin { xm1_power_recording_capture_trace_begin(operation); }
            else { xm1_power_recording_capture_trace_end(operation); }
        }
    }
}
fn clear_requests() {
    for offset in (0x9370..0x937c).step_by(4) { store(offset, 0); }
    store(0x9380, 0);
}
fn reset() {
    clear_requests();
    store(0x9384, 0);
    for offset in (0x9388..0x93b0).step_by(4) { store(offset, 0); }
    store(0x93b0, load(0x81e8));
    store(0x93bc, load(0x81ec));
    store(0x93c4, load(0x81f0));
}
fn settings_request(kind: u32, selected: bool) -> i32 {
    unsafe { xm1_power_recording_capture_settings_request(kind, load(0x81e4), address(0x9370), if selected { address(0x9380) } else { 0 }) }
}
fn parameter(kind: u32) -> i32 { unsafe { xm1_power_recording_capture_parameter_request(kind, load(0x81e4)) } }
fn field(offset: usize) -> u32 { unsafe { read_volatile((load(0x81e4) as usize + offset) as *const u32) } }

extern "C" fn resource_complete(status: u32) {
    if status == 0 {
        unsafe { xm1_power_recording_send_task_message(0x1394, 0, core::ptr::null()) };
    } else {
        unsafe { xm1_power_recording_report_missing_state() };
    }
}

fn start() {
    store(0x81f8, 3);
    reset();
    add_flags(0x38f52);
    unsafe { xm1_power_recording_capture_prepare_resource() };
    store(0x93c0, 0);
    unsafe { xm1_power_recording_capture_start_resource(resource_complete) };
    set_state(CaptureState::Starting);
}

fn slot_argument() -> u32 {
    match field(0x3e4) {
        1 => 1,
        2 if field(0x368) == 0 => u32::from(unsafe { xm1_power_recording_capture_conversion_limit() } > 0xdab),
        2 => u32::from(field(0x358) > 0x22),
        _ => 0,
    }
}

fn poll_receivers(state: CaptureState) -> i32 {
    match state {
        CaptureState::Idle => 1,
        CaptureState::Settings => {
            if settings_request(0x19, true) == 1 { add_flags(2); }
            0
        }
        CaptureState::Auxiliary | CaptureState::ParameterWait => {
            if unsafe { xm1_power_recording_capture_aux_request(3, 0) } == 1 {
                add_flags(0x800);
                if flags() & 0x4000 == 0 {
                    unsafe { xm1_power_recording_capture_slot_request([1, slot_argument()].as_ptr(), 0) };
                    add_flags(0x4000);
                    trace(0x01107100, true);
                }
            }
            if flags() & 0x4000 != 0 && unsafe { xm1_power_recording_capture_slot_request([2, 0].as_ptr(), 0) } == 0 {
                add_flags(0x8000);
                trace(0x01107100, false);
            }
            0
        }
        CaptureState::MotorWait => {
            if unsafe { xm1_power_recording_capture_motor_request(2, 0, 0) } == 1 {
                add_flags(0x10000);
                trace(0x01107200, false);
            }
            0
        }
        CaptureState::SettingsRetry => {
            if settings_request(0x19, false) == 1 { add_flags(0x80); }
            0
        }
        _ => 0,
    }
}

fn launch_drive() {
    trace(0x01090700, true);
    trace(0x01090800, true);
    trace(0x01090900, true);
    trace(0x01090c00, true);
    unsafe { xm1_power_recording_output_request(load(0x93b8), load(0x81e4)) };
    add_flags(0x1000);
}
fn needs_parameter() -> bool { flags() & 0x400 != 0 && load(0x93c4) != 1 && load(0x93b0) != 0x80 && field(0x690) == 0 }
fn parameter_wait() {
    result_state(CaptureState::ParameterWait);
    add_flags(0x10000);
    trace(0x01090a00, true);
    parameter(0);
    set_state(CaptureState::ParameterWait);
}
fn after_settings(starting: bool) -> i32 {
    if flags() & 0x38a53 == 0x38a53 {
        transition(CaptureState::Idle);
        return 1;
    }
    if starting {
        if flags() & 1 == 0 { return 0; }
        unsafe { xm1_power_recording_capture_select_changed(0xa) };
        if settings_request(2, true) != 1 { transition(CaptureState::Settings); return 0; }
        add_flags(2);
    } else if flags() & 3 != 3 { return 0; }
    if !starting && load(0x93b4) == 2 { add_flags(0x210); }
    else if flags() & 0x400 != 0 { launch_drive(); }
    if needs_parameter() { parameter_wait(); }
    else { transition(CaptureState::Auxiliary); }
    unsafe { xm1_power_recording_capture_aux_request(u32::from(field(0xfc) == 1), 0) };
    0
}
fn finish_parameter() {
    add_flags(0x10000);
    if flags() & 0x100 == 0 { return; }
    launch_parameter();
}
fn launch_parameter() {
    trace(0x01090b00, true);
    if load(0x93c4) != 1 {
        let kind = if load(0x93b4) == 2 {
            if unsafe { xm1_power_recording_capture_parameter_class(load(0x81e4)) } == 0 { 4 } else { 5 }
        } else { 1 };
        if parameter(kind) != 1 { transition(CaptureState::ParameterRetry); return; }
    }
    result_state(CaptureState::Finishing);
    add_flags(0x40);
    trace(0x01090b00, false);
    finish_callback();
    set_state(CaptureState::Finishing);
}
fn finish_callback() {
    trace(0x01107300, true);
    unsafe { xm1_power_recording_send_task_message(0x1303, 0, core::ptr::null()) };
}
fn poll_completion(state: CaptureState) -> i32 {
    match state {
        CaptureState::Idle => 1,
        CaptureState::Starting => after_settings(true),
        CaptureState::Settings => after_settings(false),
        CaptureState::Auxiliary => {
            if load(0x93b4) != 2 && flags() & 0x1400 == 0x400 { launch_drive(); }
            if flags() & 4 != 0 && load(0x93c4) != 1 && load(0x93b0) != 0x80 && field(0x690) == 0 { parameter_wait(); }
            else if flags() & 0xac10 == 0xac10 {
                if load(0x93b0) == 0x80 {
                    result_state(CaptureState::MotorWait);
                    unsafe { xm1_power_recording_capture_motor_request(1, 1, 0) };
                    trace(0x01107200, true);
                    set_state(CaptureState::MotorWait);
                } else { finish_parameter(); }
            }
            0
        }
        CaptureState::ParameterWait => {
            if flags() & 0xac30 == 0xac30 {
                if load(0x93b0) == 0x80 {
                    unsafe { xm1_power_recording_repeat_current_notification() };
                } else if flags() & 0x100 != 0 { launch_parameter(); }
            }
            0
        }
        CaptureState::MotorWait => { if flags() & 0x10100 == 0x10100 { launch_parameter(); } 0 }
        CaptureState::ParameterRetry => {
            if flags() & 0x40 != 0 { result_state(CaptureState::Finishing); finish_callback(); set_state(CaptureState::Finishing); }
            0
        }
        CaptureState::Finishing => {
            let values = load(0x353c) as *const u32;
            let changed = unsafe { xm1_power_recording_capture_conversion_changed(read_volatile(values), read_volatile(values.add(1))) };
            if flags() & 0x38a53 != 0x38a53 { return 0; }
            if changed == 1 || load(0x9610) == 1 || load(0x5fd8) == 1 {
                clear_requests();
                store(0x9370, 1 << 28);
                if load(0x5fd8) == 1 { store(0x5fd8, 0); }
                if settings_request(2, false) != 1 { transition(CaptureState::SettingsRetry); return 0; }
            }
            transition(CaptureState::Idle);
            1
        }
        CaptureState::SettingsRetry => {
            if flags() & 0x38ad3 != 0x38ad3 { return 0; }
            transition(CaptureState::Idle);
            1
        }
    }
}

fn callback_event() {
    let message = unsafe { read_volatile(RECORDING as *const u16) };
    let (number, bit, end_trace) = match message {
        0x1394 => (9, 1, Some(0x01090600)),
        0x1397 => (10, 4, Some(0x01090800)),
        0x1398 => (11, 8, Some(0x01090900)),
        0x1399 => (12, 0x10, Some(0x01090700)),
        0x139c => (13, 0, None),
        0x139b => (14, 0, None),
        0x1303 => (15, 0x20000, Some(0x01107300)),
        _ => {
            event(16);
            CaptureState::read().map_or(0, poll_receivers);
            return;
        }
    };
    add_flags(bit);
    if let Some(operation) = end_trace { trace(operation, false); }
    event(number);
    match (number, CaptureState::read()) {
        (9, Some(CaptureState::Starting)) => unsafe { xm1_power_recording_capture_acknowledge_resource() },
        (10, Some(CaptureState::Auxiliary))
        | (11 | 12, Some(CaptureState::Auxiliary | CaptureState::ParameterWait)) => {
            unsafe { xm1_power_recording_output_request(0xe, load(0x81e4)) };
        }
        (13, Some(CaptureState::ParameterWait)) => {
            add_flags(0x20);
            parameter(0x1a);
            trace(0x01090a00, false);
        }
        (13, Some(CaptureState::ParameterRetry)) => {
            add_flags(0x40);
            parameter(0x1a);
            trace(0x01090b00, false);
        }
        (14, Some(CaptureState::Auxiliary | CaptureState::ParameterWait)) => {
            trace(0x01090c00, false);
            add_flags(0x200);
        }
        (15, Some(CaptureState::Finishing)) => {}
        _ => { unsafe { xm1_power_recording_defer_current() }; }
    }
}

#[no_mangle]
pub extern "C" fn xm1_power_recording_capture_request(kind: u32) -> i32 {
    store(0x81e0, kind);
    store(0x81e4, 0);
    unsafe { write_volatile((PREFIX + 0xbc) as *mut u16, 0) };
    match kind {
        4 => {
            event(5);
            match CaptureState::read() {
                Some(CaptureState::Idle) => { start(); result_state(CaptureState::Starting); 0 }
                _ => unsafe { xm1_power_recording_defer_current() },
            }
        }
        11 => {
            callback_event();
            event(17);
            CaptureState::read().map_or(0, poll_completion)
        }
        _ => {
            event(18);
            unsafe { xm1_power_recording_defer_current() }
        }
    }
}

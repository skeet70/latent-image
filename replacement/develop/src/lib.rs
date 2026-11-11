use crate::capture;
use core::ptr::{read_volatile, write_volatile};

pub mod calibration_core;
pub mod font_core;
pub mod framework;
pub mod permit;
mod pipeline;

#[derive(Clone, Copy)]
struct Context(*mut u8);

#[derive(Clone, Copy)]
struct OperationKind(u32);

#[derive(Clone, Copy)]
struct Operation(u32);

#[derive(Clone, Copy)]
struct StepResult(u32);

#[derive(Clone, Copy)]
struct PlanIndex(u8);

#[repr(u32)]
#[derive(Clone, Copy)]
enum Stage { Idle = 0, Running = 1, Suspended = 2 }

#[derive(Clone, Copy, PartialEq, Eq)]
enum SaveRequest { Primary, Secondary, Raw, Combined, Other(u32) }

impl SaveRequest {
    fn parse(word: u32) -> Self {
        match word { 0 => Self::Primary, 1 => Self::Secondary, 2 => Self::Raw, 4 => Self::Combined, other => Self::Other(other) }
    }
    fn word(self) -> u32 {
        match self { Self::Primary => 0, Self::Secondary => 1, Self::Raw => 2, Self::Combined => 4, Self::Other(word) => word }
    }
    fn step(self) -> u32 {
        match self { Self::Raw => 0x32, Self::Combined => 0x33, Self::Secondary => 0x34, _ => 0x35 }
    }
}

enum Command { Pipeline(pipeline::PipelineStep), Save(SaveRequest), Wait(u8), Post, Finish, Unassigned }

impl Command {
    fn parse(word: u32) -> Self {
        if let Some(step) = pipeline::PipelineStep::parse(word) { return Self::Pipeline(step); }
        match word {
            0x32 => Self::Save(SaveRequest::Raw),
            0x33 => Self::Save(SaveRequest::Combined),
            0x34 => Self::Save(SaveRequest::Secondary),
            0x35 => Self::Save(SaveRequest::Primary),
            0x71..=0x7c => Self::Wait((word - 0x71) as u8),
            0x7e => Self::Post,
            0x7f => Self::Finish,
            _ => Self::Unassigned,
        }
    }
}

impl Context {
    unsafe fn word(self, offset: usize) -> u32 { unsafe { read_volatile(self.0.add(offset).cast::<u32>()) } }
    unsafe fn set(self, offset: usize, value: u32) { unsafe { write_volatile(self.0.add(offset).cast::<u32>(), value) } }
    unsafe fn frame(self) -> *mut u8 { unsafe { self.word(0) as *mut u8 } }
    unsafe fn frame_word(self, offset: usize) -> u32 { unsafe { read_volatile(self.frame().add(offset).cast::<u32>()) } }
    unsafe fn kind(self) -> OperationKind { OperationKind(unsafe { self.word(4) }) }
    unsafe fn operation(self) -> Operation { Operation(unsafe { framework::xm1_develop_current(self.0) }) }
    unsafe fn stage(self, stage: Stage) { unsafe { self.set(0x40, stage as u32) } }
    unsafe fn callback(self, operation: Operation, result: StepResult, save_status: u32) {
        unsafe { framework::xm1_develop_callback(self.0, operation.0, result.0, save_status) }
    }
}

unsafe fn global(offset: usize) -> u32 { unsafe { read_volatile((0x015b0d80usize + offset) as *const u32) } }
unsafe fn set_global(offset: usize, value: u32) { unsafe { write_volatile((0x015b0d80usize + offset) as *mut u32, value) } }

unsafe fn steps(kind: OperationKind) -> Option<*const u32> {
    let mut index = 0u8;
    loop {
        let row = unsafe { (0x00ea4870 as *const u32).add(index as usize * 2) };
        let key = unsafe { read_volatile(row) };
        if key == 0x33 { return None; }
        if key == kind.0 {
            let list = unsafe { read_volatile(row.add(1)) } as *const u32;
            return if list.is_null() { None } else { Some(list) };
        }
        index = index.wrapping_add(1);
    }
}

unsafe fn command_at(list: *const u32, index: PlanIndex) -> u32 { unsafe { read_volatile(list.add(index.0 as usize)) } }

unsafe fn next_step(context: Context, list: *const u32) -> PlanIndex {
    let operation = unsafe { context.operation() };
    let mapped = unsafe { xm1_develop_operation_index(operation.0) };
    let mut index = 0u8;
    loop {
        let word = unsafe { command_at(list, PlanIndex(index)) };
        if word == 0x80 { return PlanIndex(0); }
        if word == mapped { return PlanIndex(index.wrapping_add(1)); }
        index = index.wrapping_add(1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_operation_index(operation: u32) -> u32 {
    let table = 0x00ea51a4 as *const u32;
    let mut cursor = table;
    while unsafe { read_volatile(cursor) } != 0x80 { cursor = unsafe { cursor.add(1) }; }
    unsafe { read_volatile(table.add(operation as usize)) }
}

use framework::xm1_develop_transition_supported;


/// # Safety
/// `context` is an aligned live image-5 background context.
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_dispatch(context: *mut u8) {
    let context = Context(context);
    let mut save_status = 1i32;
    unsafe { context.stage(Stage::Running); xm1_pipeline_image_signal_current_zero(); }
    let Some(list) = (unsafe { steps(context.kind()) }) else { unsafe { context.stage(Stage::Idle); } return; };
    if unsafe { command_at(list, PlanIndex(0)) } == 0x80 { return; }
    let mut result = StepResult(0);
    loop {
        let mut index = unsafe { next_step(context, list) };
        let critical = unsafe { xm1_develop_enter_critical() };
        let supported = unsafe { xm1_develop_transition_supported(context.0, command_at(list, index)) };
        if supported == 5 {
            unsafe { context.stage(Stage::Suspended); xm1_develop_leave_critical(critical); }
            let before = unsafe { context.operation() };
            unsafe { context.callback(Operation(0xff), result, 0); }
            let after = unsafe { context.operation() };
            unsafe { develop_diagnostic(context.0, 3); }
            let mut signal = 0u32;
            unsafe { xm1_pipeline_image_wait_all(1, &mut signal, -1); develop_diagnostic(context.0, 4); context.stage(Stage::Running); xm1_pipeline_image_signal_current_zero(); }
            if before.0 != after.0 { index = unsafe { next_step(context, list) }; }
        } else {
            unsafe { xm1_develop_leave_critical(critical); context.operation(); xm1_pipeline_image_signal_current_zero(); }
        }
        unsafe { xm1_develop_mark_task_active(context.frame()); }
        let word = unsafe { command_at(list, index) };
        match Command::parse(word) {
            Command::Pipeline(step) => result = StepResult(unsafe { step.run(context.0) }),
            Command::Save(kind) => result = StepResult(unsafe { save_step(context, kind, &mut save_status) } as u32),
            Command::Wait(step) => result = StepResult(unsafe { xm1_develop_wait_step(context.0, step as u32) } as u32),
            Command::Post => unsafe { context.stage(Stage::Idle); xm1_develop_post(context.0, 0); },
            Command::Finish => unsafe { context.stage(Stage::Idle); develop_finish_trace(context.kind().0, 1); context.callback(Operation(0xfe), result, save_status as u32); },
            Command::Unassigned => {}
        }
        let current_command = unsafe { command_at(list, index) };
        if current_command != 0x7f && result.0 != 0 && result.0 != 5 {
            if unsafe { global(0x41fc) } != 0xb {
                unsafe { context.callback(Operation(0xfe), result, save_status as u32); }
            } else if current_command != 0x7e {
                unsafe { xm1_develop_post(context.0, result.0); }
            }
            return;
        }
        if unsafe { command_at(list, PlanIndex(index.0.wrapping_add(1))) } == 0x80 { return; }
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_wait_step(context: *mut u8, step: u32) -> i32 {
    let plan = unsafe { framework::xm1_develop_plan_lookup(0x80, step) };
    unsafe { capture::rb_capture_step_wait(context, plan) }
}

unsafe fn complete_save(context: Context, plan: *const capture::StepPlan, completed: bool, status_out: *mut i32) -> i32 {
    let result = unsafe { framework::xm1_develop_complete(context.0, plan, completed as i32) };
    let output = if completed { unsafe { read_volatile(status_out) } } else { 1 };
    unsafe { write_volatile(status_out, output); }
    result
}


unsafe fn save_step(context: Context, kind: SaveRequest, status_out: *mut i32) -> i32 {
    let plan = unsafe { framework::xm1_develop_plan_lookup(kind.step(), 13) };
    if unsafe { framework::xm1_develop_precondition(context.0, kind.word()) } == 0
        || (kind == SaveRequest::Combined && unsafe { context.frame_word(0x322c) } != 2) {
        unsafe { framework::xm1_develop_store(context.0, read_volatile((plan as *const u32).add(4))); }
        return 0;
    }
    unsafe { write_volatile(status_out, 1); }
    let mut status = unsafe { framework::xm1_develop_begin(context.0, plan) };
    if status != 0 { return status; }
    if unsafe { context.frame_word(0x27cc) } != 0 {
        return unsafe { complete_save(context, plan, true, status_out) };
    }
    let mut numbering = 0u32;
    let mut storage = 0u32;
    unsafe { develop_numbering(1, &mut numbering); develop_numbering(2, &mut storage); }
    if numbering & 1 != 0 || storage == 7 || unsafe { xm1_develop_loadtest_setting() } == 0xa8 {
        return unsafe { complete_save(context, plan, true, status_out) };
    }
    let saved = if unsafe { context.frame_word(0x27c8) } == 0 {
        status = 2;
        0
    } else {
        unsafe { xm1_save_facade(context.frame().cast::<u32>(), kind.word() as i32, &mut status) }
    };
    let trace = kind == SaveRequest::Combined
        || (kind == SaveRequest::Raw && unsafe { context.frame_word(0x4a8) } == 0)
        || (kind != SaveRequest::Raw && unsafe { context.frame_word(0x4a8) } != 0)
        || unsafe { context.frame_word(0x578) } != 2;
    if trace { unsafe { xm1_pipeline_image_trace_end(0x06000000); develop_trace_event(1, 0x61); } }
    if status != 0 {
        unsafe { framework::xm1_develop_store(context.0, read_volatile((plan as *const u32).add(4))); }
        return if status == 1 { 1 } else { 2 };
    }
    unsafe { complete_save(context, plan, saved == 1, status_out) }
}

/// # Safety
/// `context` and `status_out` are live aligned image-5 ABI records.
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_save_step(context: *mut u8, kind: u32, status_out: *mut i32) -> i32 {
    unsafe { save_step(Context(context), SaveRequest::parse(kind), status_out) }
}

/// # Safety
/// `context` points to the live image-5 ABI record.
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_initialize(context: *mut u8) {
    let context = Context(context);
    unsafe {
        context.set(0x40, 0);
        context.set(0x34, 0x69);
        context.set(0x3c, 0);
        context.set(0, 0);
        context.set(0x44, 3);
        context.set(0x48, 2);
        context.set(0x4c, 0);
    }
}

/// # Safety
/// `params` contains 13 aligned words; `context` is the live image-5 ABI record.
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_load(context: *mut u8, params: *const u32) {
    let context = Context(context);
    unsafe {
        for i in 0..4 { context.set(4 + i * 4, read_volatile(params.add(i))); }
        let pair = [read_volatile(params.add(4)), read_volatile(params.add(5))];
        context.set(0x14, pair[0]); context.set(0x18, pair[1]);
        let ids = [read_volatile(params.add(6)), read_volatile(params.add(7)), read_volatile(params.add(8))];
        for (i, value) in ids.iter().enumerate() { context.set(0x34 + i * 4, *value); }
        let arguments = [read_volatile(params.add(9)), read_volatile(params.add(10)), read_volatile(params.add(11)), read_volatile(params.add(12))];
        for (i, value) in arguments.iter().enumerate() { context.set(0x1c + i * 4, *value); }
    }
}

/// # Safety
/// `context` and the registered continuation/plane are live image-5 records.
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_post(context: *mut u8, status: u32) {
    let context = Context(context);
    let plane = unsafe { global(0x4580) } as *mut u32;
    let mut result = StepResult(status);
    if status == 0 && unsafe { global(0x4568) } != 0 {
        unsafe {
            write_volatile(plane, context.frame_word(0x188));
            write_volatile(plane.add(2), context.frame_word(0x190));
            write_volatile(plane.add(3), context.frame_word(0x194));
            write_volatile(plane.add(4), context.frame_word(0x198));
            write_volatile(plane.add(1), context.frame_word(0x18c));
        }
    } else {
        let error = unsafe { develop_post_service() };
        if error == 0 { unsafe { set_global(0x41fc, 0xff); set_global(0x4200, 1); } }
        else { unsafe { develop_post_error(error); } result = StepResult(3); }
        unsafe { xm1_develop_initialize(context.0); xm1_pipeline_image_cache_maintenance(); }
    }
    let continuation: unsafe extern "C" fn(u32) = unsafe { core::mem::transmute(global(0x4570) as usize) };
    unsafe { continuation(result.0); }
}

#[repr(C)]
struct Job { context: u32, parameters: [u32; 13], reserved: [u32; 2] }
#[repr(C)]
struct ReceiveFrame { header: [u32; 2], payload: *mut Job }

/// # Safety
/// `channel` is the registered develop-task receive channel.
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_entry(channel: u32) -> ! {
    let mut job = Job { context: 0, parameters: [0; 13], reserved: [0; 2] };
    let mut message = ReceiveFrame { header: [0; 2], payload: &mut job };
    loop {
        message.payload = &mut job;
        unsafe { xm1_pipeline_image_receive_message(channel, &mut message, -1); }
        let context = Context(unsafe { read_volatile(core::ptr::addr_of!(job.context)) } as *mut u8);
        unsafe { xm1_develop_load(context.0, job.parameters.as_ptr()); develop_diagnostic(context.0, 0); }
        if unsafe { context.word(0xc) } == 5 { unsafe { xm1_develop_dispatch(context.0); } }
        else { unsafe { framework::xm1_develop_finish(context.0); } }
    }
}

unsafe extern "C" {
    fn xm1_pipeline_image_signal_current_zero() -> i32;
    fn xm1_develop_enter_critical() -> u32;
    fn xm1_develop_leave_critical(token: u32);
    fn xm1_pipeline_image_wait_all(mask: u32, output: *mut u32, timeout: i32) -> i32;
    fn xm1_pipeline_image_receive_message(channel: u32, message: *mut ReceiveFrame, timeout: i32) -> i32;
    fn xm1_develop_mark_task_active(frame: *mut u8);
    fn develop_diagnostic(context: *mut u8, phase: u32);
    fn develop_finish_trace(kind: u32, phase: u32);
    fn develop_numbering(selector: u32, output: *mut u32);
    fn xm1_develop_loadtest_setting() -> u32;
    fn xm1_pipeline_image_trace_end(mask: u32);
    fn develop_trace_event(enabled: u32, event: u32);
    fn xm1_save_facade(frame: *mut u32, kind: i32, status: *mut i32) -> i32;
    fn develop_post_service() -> u32;
    fn develop_post_error(error: u32);
    fn xm1_pipeline_image_cache_maintenance();
}

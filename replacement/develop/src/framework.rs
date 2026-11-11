use super::capture::StepPlan;
use super::pipeline::debug_storage::{append, copy, xm1_pipeline_image_storage_decimal};
use super::pipeline::shared::Address32;
use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy)]
struct Context(*mut u8);
#[derive(Clone, Copy)]
struct Operation(u32);
#[derive(Clone, Copy)]
struct State(u32);
#[derive(Clone, Copy)]
struct StageTask(u32);
#[repr(u32)]
enum TaskMarker { Active = 1 }
#[repr(u32)]
#[derive(Clone, Copy)]
enum ObjectPhase { Prepare = 0, Open = 1, Complete = 2, Close = 3 }

#[derive(Clone, Copy)]
struct BitCount(u32);
#[derive(Clone, Copy)]
struct BitIndex(u32);

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_bitmap_union(bitmap: *mut u8, mask: *const u32, bits: u32) {
    let bits = BitCount(bits);
    if bitmap.is_null() || mask.is_null() { loop {} }
    if bits.0 == 0 { return; }
    let mut offset = 0u32;
    loop {
        let destination = (bitmap as u32).wrapping_add(offset) as *mut u32;
        let source = (mask as u32).wrapping_add(offset) as *const u32;
        let previous = unsafe { read_volatile(destination) };
        let incoming = unsafe { read_volatile(source) };
        unsafe { write_volatile(destination, previous | incoming) };
        offset = offset.wrapping_add(4);
        if bits.0 <= offset.wrapping_shl(3) { break; }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_bitmap_set(bitmap: *mut u8, bit: u32) {
    if bitmap.is_null() { loop {} }
    let bit = BitIndex(bit);
    let word = (bitmap as u32).wrapping_add((bit.0 >> 5).wrapping_mul(4)) as *mut u32;
    let previous = unsafe { read_volatile(word) };
    unsafe { write_volatile(word, previous | (1u32 << (bit.0 & 31))) };
}

unsafe fn word(base: *const u8, offset: usize) -> u32 {
    unsafe { read_volatile(base.add(offset).cast::<u32>()) }
}
unsafe fn plan_word(plan: *const StepPlan, index: usize) -> u32 {
    unsafe { word(plan.cast(), index * 4) }
}
impl Context {
    unsafe fn state(self) -> *mut u8 { unsafe { word(self.0, 0) as *mut u8 } }
    unsafe fn operation_slot(self) -> *mut u32 {
        let state = unsafe { self.state() };
        let offset = if unsafe { word(self.0, 4) } == 7 { 0x38 }
            else if unsafe { word(state, 0x4b4) } == 0x1d { 0 }
            else { unsafe { word(self.0, 0x34) }.wrapping_sub(11).wrapping_mul(4) as usize };
        unsafe { state.add(offset).cast() }
    }
    unsafe fn store(self, state: State) { unsafe { write_volatile(self.operation_slot(), state.0) } }
    unsafe fn current(self) -> State { State(unsafe { read_volatile(self.operation_slot()) }) }
    unsafe fn objects(self, operation: Operation, phase: ObjectPhase, argument: u32) {
        unsafe { xm1_develop_dispatch_objects(self.0, operation.0, phase as u32, argument) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_plan_lookup(a: u32, b: u32) -> *const StepPlan {
    let mut plan = 0x00ea6738usize as *const StepPlan;
    loop {
        let operation = unsafe { plan_word(plan, 0) };
        let kind = unsafe { plan_word(plan, 7) };
        if operation == 0x80 && kind == 13 { return core::ptr::null(); }
        if (a == 0x80 && b == kind) || (b == 13 && a == operation) { return plan; }
        plan = unsafe { plan.add(1) };
    }
}

unsafe fn request_in_table(table: *const u8, operation: Operation, stride: usize) -> bool {
    if table.is_null() { return false; }
    let mut index = 0u8;
    loop {
        let request = unsafe { word(table, index as usize * stride) };
        if request == 0x80 { return false; }
        if request == operation.0 { return true; }
        index = index.wrapping_add(1);
    }
}
unsafe fn followup(context: Context) {
    let mode = unsafe { word(context.0, 0x44) };
    let wait = unsafe { word(context.0, 0x48) };
    if (mode > 2 && wait > 1) || wait <= 1 { return; }
    let current = unsafe { context.current() }.0;
    if current > 0x100 { return; }
    let table = unsafe { word(0x00ea2548usize as *const u8, mode as usize * 8 + 4) } as *const u8;
    let mut index = 0u8;
    loop {
        let offset = index as usize * 12;
        let from = unsafe { word(table, offset + 4) };
        if from == 0x101 { return; }
        if from == current {
            let next = unsafe { word(table, offset + 8) };
            if next <= 0x100 { unsafe { context.store(State(next)) }; }
            return;
        }
        index = index.wrapping_add(1);
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_transition(context: *mut u8, operation: u32) -> i32 {
    let context = Context(context);
    let operation = Operation(operation);
    if unsafe { xm1_develop_transition_supported(context.0, operation.0) } == 5 {
        unsafe { followup(context) }; return 5;
    }
    if unsafe { xm1_develop_simple_request(context.0, operation.0) } == 6 {
        unsafe { followup(context) }; return 6;
    }
    0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_transition_supported(context: *mut u8, operation: u32) -> i32 {
    let mode = unsafe { word(context, 0x44) };
    if mode <= 2 {
        let table = unsafe { word(0x00ea2548usize as *const u8, mode as usize * 8 + 4) } as *const u8;
        if unsafe { request_in_table(table, Operation(operation), 12) } { return 5; }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_current(context: *mut u8) -> u32 { unsafe { Context(context).current().0 } }
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_store(context: *mut u8, state: u32) { unsafe { Context(context).store(State(state)) } }

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_simple_request(context: *mut u8, operation: u32) -> i32 {
    let wait = unsafe { word(context, 0x48) };
    if wait <= 1 {
        let table = unsafe { word(0x00ea1edcusize as *const u8, wait as usize * 8 + 4) } as *const u8;
        if unsafe { request_in_table(table, Operation(operation), 4) } { return 6; }
    }
    0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_followup(context: *mut u8) {
    unsafe { followup(Context(context)) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_dispatch_objects(context: *mut u8, operation: u32, phase: u32, argument: u32) {
    if phase == 0 || phase == 2 {
        unsafe { super::pipeline::operation_objects::xm1_develop_set_operation_objects(context, operation, phase, argument) };
    } else {
        unsafe { super::pipeline::operation_objects::xm1_develop_operation_object_rule(context, operation, phase, argument) };
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_lookup_object(context: *mut u8, group: u32, kind: i32) -> u32 {
    unsafe { xm1_develop_object_entry(Context(context).state().add(0x7c) as u32, group, kind as u32) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_callback(context: *mut u8, operation: u32, arg: u32, _flags: u32) {
    let callback = unsafe { word(context, 8) };
    if callback != 0 {
        unsafe { xm1_develop_invoke_callback(context, operation, arg, callback) };
    } else {
        unsafe { xm1_develop_report_error(0x00ea1fa0, 0, 0x00e86c1c, 0) };
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_begin(context: *mut u8, plan: *const StepPlan) -> i32 {
    let handle = Context(context);
    unsafe { xm1_develop_append_stream_text(xm1_pipeline_image_current_task() & 255, plan_word(plan, 10));
        xm1_develop_stock_trace(plan_word(plan, 11)); }
    let status = unsafe { xm1_develop_transition(context, plan_word(plan, 0)) };
    if status != 0 { return status; }
    unsafe { handle.objects(Operation(plan_word(plan, 0)), ObjectPhase::Prepare, plan_word(plan, 5)) };
    let status = unsafe { xm1_develop_transition(context, plan_word(plan, 0)) };
    if status != 0 { return status; }
    unsafe { handle.objects(Operation(plan_word(plan, 0)), ObjectPhase::Open, plan_word(plan, 5)) };
    let status = unsafe { xm1_develop_transition(context, plan_word(plan, 0)) };
    if status != 0 {
        unsafe { handle.objects(Operation(plan_word(plan, 0)), ObjectPhase::Close, plan_word(plan, 5)) };
        return status;
    }
    unsafe { xm1_develop_callback(context, plan_word(plan, 1), 0, 0);
        handle.store(State(plan_word(plan, 3)));
        xm1_pipeline_image_trace_begin(plan_word(plan, 6)); }
    0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_complete(context: *mut u8, plan: *const StepPlan, status: i32) -> i32 {
    let handle = Context(context);
    unsafe { xm1_pipeline_image_trace_end(plan_word(plan, 6));
        handle.objects(Operation(plan_word(plan, 0)), ObjectPhase::Close, plan_word(plan, 5)); }
    if status == 1 {
        unsafe { handle.store(State(plan_word(plan, 4)));
            handle.objects(Operation(plan_word(plan, 0)), ObjectPhase::Complete, plan_word(plan, 5));
            xm1_develop_callback(context, plan_word(plan, 2), 0, 0); }
    }
    let result = unsafe { xm1_develop_transition(context, plan_word(plan, 0)) };
    unsafe { xm1_pipeline_image_signal_current_zero(); xm1_develop_mark_task_active(handle.state());
        xm1_develop_stock_trace(plan_word(plan, 12)); }
    result
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_mark_task_active(frame: *mut u8) {
    let task = StageTask(unsafe { xm1_pipeline_image_current_task() });
    let offset = task.0.wrapping_add(5).wrapping_mul(4) as usize;
    let marker = (frame as usize).wrapping_add(offset) as *mut TaskMarker;
    unsafe { write_volatile(marker, TaskMarker::Active) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_precondition(context: *mut u8, kind: u32) -> i32 {
    let state = unsafe { Context(context).state() };
    let mode = unsafe { word(state, 0x4b4) };
    if mode != 0x19 && mode != 0x1a && mode != 0x20 && unsafe { word(state, 0x710) } & 0x400 == 0 {
        if mode == 0x1d {
            if unsafe { word(state, 0x3218) } == 0 { return 0; }
            if kind == 0 {
                if unsafe { word(state, 0x5cc) } != 4 { return 0; }
                let options = unsafe { word(state, 0x3228) } as *const u8;
                if unsafe { word(options, 0x110) } == 1 || unsafe { word(options, 0x114) } == 1 || unsafe { word(options, 0x10c) } == 1 { return 0; }
            } else if kind == 1 && unsafe { word(state, 0x5cc) } == 4 {
                let options = unsafe { word(state, 0x3228) } as *const u8;
                if unsafe { word(options, 0x110) } == 0 && unsafe { word(options, 0x114) } == 0 && unsafe { word(options, 0x10c) } == 0 && unsafe { word(state, 0x27bc) } == 0 { return 0; }
            }
        } else {
            if kind == 0 && unsafe { word(state, 0x275c) } == 0 { return 0; }
            if kind == 1 && unsafe { word(state, 0x2754) } == 0 && unsafe { word(state, 0x275c) } == 1 { return 0; }
        }
    }
    let option = unsafe { word(state, 0x578) };
    if option == 0 && kind == 2 { return 0; }
    if kind <= 1 && option == 1 { return 0; }
    ((mode != 0x1e && kind != 4) || (mode == 0x1e && kind == 4)) as i32
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_transform_required(context: *mut u8, kind: u32) -> i32 {
    let state = unsafe { Context(context).state() };
    ((unsafe { word(state, 0x614) } != 0 || unsafe { word(state, 0x61c) } != 0) && (kind != 0 || unsafe { word(state, 0x275c) } != 1)) as i32
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_finish(context: *mut u8) {
    let operand = unsafe { word(context, 0x10) } as *mut u8;
    match unsafe { word(context, 0xc) } {
        0 => unsafe { xm1_develop_stock_finish0(operand) },
        1 => unsafe { xm1_develop_stock_finish1(operand) },
        3 => unsafe { xm1_develop_stock_finish3(operand) },
        4 => unsafe { xm1_develop_stock_finish4(operand) },
        _ => (),
    }
    unsafe { xm1_develop_callback(context, 0xfe, 0, 0) };
}

fn notification_operation(state: State) -> bool {
    (0xe2..=0xf8).contains(&state.0) && state.0 & 1 == 0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_cancel_all(context: *mut u8) {
    if notification_operation(unsafe { Context(context).current() }) {
        unsafe { xm1_pipeline_image_send_notification(word(context, 0x34), u32::MAX) };
    }
    let mask = u32::MAX;
    unsafe { xm1_develop_bitmap_union(context.add(0x4c), &mask, 1) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_cancel(context: *mut u8, bit: u32) {
    let current = unsafe { Context(context).current() };
    let mask = match bit {
        0 => 2, 1 => 4, 2 => 8, 3 => 0x80, 4 => 0x100, 5 => 0x400,
        6 => 0x8000, 7 | 8 => 0x20000, 9 => 0x40000, 10 => 0x80000000,
        11 => 0x2000, _ => 0,
    };
    if notification_operation(current) {
        unsafe { xm1_pipeline_image_send_notification(word(context, 0x34), mask) };
    }
    unsafe { xm1_develop_bitmap_set(context.add(0x4c), bit) };
}

struct DiagnosticLabel {
    address: usize,
    length: usize,
    append_frame: bool,
}
impl DiagnosticLabel {
    fn new(address: usize, length: usize, append_frame: bool) -> Self {
        Self { address, length, append_frame }
    }
}
unsafe fn diagnostic_label(context: Context, phase: u32) -> DiagnosticLabel {
    let (address, length, append_frame) = match phase {
        0 => (0xea20cc, 13, true),
        1 => (0xea20dc, 16, true),
        2 => (0xea20f0, 15, true),
        3 => (0xea2100, 14, true),
        4 => (0xea2110, 14, true),
        5 => match unsafe { context.current() }.0 {
            0xe2 => (0xea2120, 14, true),
            0xe4 => (0xea2130, 21, true),
            0xe6 => (0xea2148, 25, false),
            0xe8 => (0xea2164, 16, true),
            0xea => (0xea2178, 23, false),
            0xec => (0xea2190, 16, true),
            0xee => (0xea21a4, 20, true),
            0xf0 => (0xea21bc, 21, true),
            0xf6 => (0xea21d4, 17, true),
            0xf8 => (0xea21e8, 17, true),
            _ => (0xea21fc, 16, true),
        },
        6 => match unsafe { context.current() }.0 {
            0xe2 => (0xea2210, 16, true),
            0xe4 => (0xea2224, 23, false),
            0xe6 => (0xea223c, 27, false),
            0xe8 => (0xea2258, 18, true),
            0xea => (0xea226c, 23, false),
            0xec => (0xea2284, 18, true),
            0xee => (0xea2298, 22, true),
            0xf0 => (0xea22b0, 23, false),
            0xf6 => (0xea22c8, 17, true),
            0xf8 => (0xea22dc, 17, true),
            _ => (0xea22f0, 18, true),
        },
        _ => (0, 0, true),
    };
    DiagnosticLabel::new(address, length, append_frame)
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_diagnostic(context: *mut u8, phase: u32) {
    let label = unsafe { diagnostic_label(Context(context), phase) };
    let mut text = [0u8; 32];
    if label.length != 0 {
        unsafe { copy(Address32(text.as_mut_ptr() as u32), Address32(label.address as u32), label.length as u32) };
    }
    if label.append_frame {
        let state = unsafe { Context(context).state() };
        if state.is_null() {
            unsafe { copy(Address32(text.as_mut_ptr().add(label.length) as u32), Address32(0xea2304), 8) };
        } else {
            let mut number = core::mem::MaybeUninit::<[u8; 32]>::uninit();
            let number = number.as_mut_ptr().cast::<u8>();
            unsafe {
                xm1_pipeline_image_storage_decimal(Address32(number as u32), word(state, 0x2f9c), 4, 0);
                append(Address32(text.as_mut_ptr() as u32), Address32(number as u32), Some(4));
                append(Address32(text.as_mut_ptr() as u32), Address32(0xea22d8), Some(1));
                let frame = read_volatile(state.add(0x1680).cast::<u16>());
                xm1_pipeline_image_storage_decimal(Address32(number as u32), frame as u32, 4, 0);
                append(Address32(text.as_mut_ptr() as u32), Address32(number as u32), Some(4));
            }
        }
    }
    unsafe { xm1_develop_append_stream_text(u32::from(read_volatile(context.add(0x34))), text.as_ptr() as u32) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_finish_trace(kind: u32, phase: u32) {
    let mut entry = 0xea5678usize as *const u8;
    loop {
        let operation = unsafe { word(entry, 0) };
        if operation == 0x33 { return; }
        if operation == kind {
            if phase == 0 {
                unsafe {
                    xm1_develop_stock_trace(word(entry, 4));
                    xm1_develop_append_stream_text(xm1_pipeline_image_current_task() & 255, word(entry, 12));
                }
            } else {
                unsafe { xm1_develop_stock_trace(word(entry, 8)) };
            }
            return;
        }
        entry = unsafe { entry.add(16) };
    }
}

unsafe extern "C" {
    fn xm1_develop_object_entry(objects: u32, group: u32, kind: u32) -> u32;
    fn xm1_pipeline_image_send_notification(task: u32, mask: u32) -> i32;
    fn xm1_develop_invoke_callback(context: *mut u8, operation: u32, argument: u32, callback: u32);
    fn xm1_develop_report_error(file: u32, line: u32, text: u32, flags: u32);
    fn xm1_develop_append_stream_text(task: u32, text: u32);
    fn xm1_develop_stock_trace(value: u32);
    fn xm1_pipeline_image_trace_begin(value: u32);
    fn xm1_pipeline_image_trace_end(value: u32);
    fn xm1_pipeline_image_signal_current_zero() -> i32;
    fn xm1_pipeline_image_current_task() -> u32;
    fn xm1_develop_stock_finish0(operand: *mut u8);
    fn xm1_develop_stock_finish1(operand: *mut u8);
    fn xm1_develop_stock_finish3(operand: *mut u8);
    fn xm1_develop_stock_finish4(operand: *mut u8);
}

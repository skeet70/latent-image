use core::ptr::{read_volatile, write_volatile};
use super::buffers::{xm1_develop_enter_critical, xm1_develop_leave_critical};
use super::shared::{Address32, ByteOffset, OperationCode};

#[derive(Clone, Copy)]
#[repr(C)]
pub struct CalendarFields {
    year: u8,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct ClockHours(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct ClockMinutes(u32);
#[derive(Clone, Copy)]
enum ClockDirection { Add, Subtract }
struct ClockAdjustment {
    direction: ClockDirection,
    hours: ClockHours,
    minutes: ClockMinutes,
}

unsafe fn clock_adjustment() -> ClockAdjustment {
    let direction = if unsafe { read_volatile(0x409c2877 as *const u8) } >> 7 == 0 { ClockDirection::Add } else { ClockDirection::Subtract };
    let hours = unsafe { read_volatile(0x409c2844 as *const u8) } & 31;
    let minutes = (unsafe { read_volatile(0x409c283c as *const u8) } >> 6) * 15;
    ClockAdjustment { direction, hours: ClockHours(if hours > 23 { 0 } else { u32::from(hours) }), minutes: ClockMinutes(u32::from(minutes)) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_clock_enabled() -> u32 {
    u32::from(unsafe { read_volatile(0x409c2878 as *const u8) } >> 7)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_clock_fields(direction: *mut u32, hours: *mut u32, minutes: *mut u32) {
    let adjustment = unsafe { clock_adjustment() };
    unsafe {
        write_volatile(direction, match adjustment.direction { ClockDirection::Add => 0, ClockDirection::Subtract => 1 });
        write_volatile(hours, adjustment.hours.0);
        write_volatile(minutes, adjustment.minutes.0);
    }
}

unsafe fn current_calendar() -> CalendarFields {
    let token = unsafe { xm1_develop_enter_critical() };
    let fields = unsafe { read_volatile(0x409c34b8 as *const CalendarFields) };
    unsafe { xm1_develop_leave_critical(token) };
    fields
}

unsafe fn month_days(month: u8, year: u8) -> u8 {
    if month == 2 && year & 3 == 0 { 29 } else {
        unsafe { read_volatile(0x007cb848u32.wrapping_add(u32::from(month)).wrapping_sub(1) as *const u8) }
    }
}

unsafe fn adjust_calendar(mut fields: CalendarFields, adjustment: ClockAdjustment) -> CalendarFields {
    match adjustment.direction {
        ClockDirection::Add => {
            fields.minute = fields.minute.wrapping_add(adjustment.minutes.0 as u8);
            if fields.minute > 59 {
                fields.minute = fields.minute.wrapping_sub(60);
                fields.hour = fields.hour.wrapping_add(1);
            }
            fields.hour = fields.hour.wrapping_add(adjustment.hours.0 as u8);
            if fields.hour > 23 {
                fields.hour = fields.hour.wrapping_sub(24);
                fields.day = fields.day.wrapping_add(1);
            }
            if fields.month == 2 && fields.year & 3 == 0 {
                if fields.day > 29 { fields.day = 1; fields.month = 3; }
            } else {
                if fields.day > unsafe { month_days(fields.month, fields.year) } {
                    fields.day = 1;
                    fields.month = fields.month.wrapping_add(1);
                }
                if fields.month > 12 {
                    fields.month = 1;
                    fields.year = fields.year.wrapping_add(1);
                }
            }
            if fields.year > 50 { fields.year = 0; }
        }
        ClockDirection::Subtract => {
            let borrow = u32::from(u32::from(fields.minute) < adjustment.minutes.0);
            fields.minute = (u32::from(fields.minute).wrapping_add(borrow * 60).wrapping_sub(adjustment.minutes.0)) as u8;
            let hours = adjustment.hours.0.wrapping_add(borrow);
            if u32::from(fields.hour) < hours {
                fields.hour = (u32::from(fields.hour).wrapping_add(24).wrapping_sub(hours)) as u8;
                fields.day = fields.day.wrapping_sub(1);
                if fields.day == 0 {
                    fields.month = fields.month.wrapping_sub(1);
                    if fields.month == 0 {
                        fields.month = 12;
                        fields.year = if fields.year == 0 { 50 } else { fields.year - 1 };
                    }
                    fields.day = unsafe { month_days(fields.month, fields.year) };
                }
            } else {
                fields.hour = (u32::from(fields.hour) - hours) as u8;
            }
        }
    }
    fields
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_calendar_current(output: *mut u8) -> i32 {
    unsafe { write_volatile(output as *mut CalendarFields, current_calendar()) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_calendar_adjusted(output: *mut u8) -> i32 {
    let fields = unsafe { current_calendar() };
    let result = unsafe { adjust_calendar(fields, clock_adjustment()) };
    unsafe { write_volatile(output as *mut CalendarFields, result) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_calendar_selected(output: *mut u8) -> i32 {
    if unsafe { xm1_pipeline_image_clock_enabled() } == 1 {
        unsafe { xm1_pipeline_image_calendar_adjusted(output) }
    } else {
        unsafe { xm1_pipeline_image_calendar_current(output) }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_calendar_read(output: *mut u8) {
    unsafe { xm1_pipeline_image_calendar_selected(output) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_stream_marker(length: u32, marker: u32) {
    let control = Address32(0x40a5b5e0);
    let buffer = Address32(unsafe { control.offset(ByteOffset(12)).word() });
    if buffer.0 == 0 { return; }
    let mut offset = unsafe { control.offset(ByteOffset(4)).word() };
    let padding = length & 3;
    if padding != 0 {
        for index in 0..4-padding {
            unsafe { buffer.offset(ByteOffset(offset.wrapping_add(index))).write_byte(0) };
        }
        offset = offset.wrapping_add(4-padding);
        unsafe { control.offset(ByteOffset(4)).write_word(offset) };
    }
    unsafe { buffer.offset(ByteOffset(offset)).write_word(marker) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_append_stream_text(category: u32, source: u32) {
    let control = Address32(0x40a5b5e0);
    if unsafe { control.offset(ByteOffset(12)).word() } == 0 { return; }
    let mut text = [0u8; 32];
    for (index, byte) in text.iter_mut().enumerate() {
        let value = unsafe { read_volatile(source.wrapping_add(index as u32) as *const u8) };
        if value == 0 { break; }
        *byte = value;
    }
    let token = unsafe { xm1_develop_enter_critical() };
    let offset = unsafe { control.offset(ByteOffset(4)).word() };
    if (offset.wrapping_add(43) & !3) > 0x1ffc {
        unsafe { xm1_pipeline_image_stream_marker(0, 0xaaaaaaaa); control.offset(ByteOffset(4)).write_word(8) };
    }
    let buffer = Address32(unsafe { control.offset(ByteOffset(12)).word() });
    let offset = unsafe { control.offset(ByteOffset(4)).word() };
    let category = category as u8;
    let header = [0xae, 0xde, category, category, 32, 0, 0, 0];
    for (index, byte) in header.iter().enumerate() {
        unsafe { buffer.offset(ByteOffset(offset.wrapping_add(index as u32))).write_byte(*byte) };
    }
    unsafe { control.offset(ByteOffset(4)).write_word(offset.wrapping_add(8)) };
    let offset = unsafe { control.offset(ByteOffset(4)).word() };
    for (index, byte) in text.iter().enumerate() {
        unsafe { buffer.offset(ByteOffset(offset.wrapping_add(index as u32))).write_byte(*byte) };
    }
    unsafe {
        control.offset(ByteOffset(4)).write_word(offset.wrapping_add(32));
        xm1_pipeline_image_calendar_read(buffer.0 as *mut u8);
        xm1_pipeline_image_stream_marker(32, 0x55555555);
        xm1_develop_leave_critical(token);
    }
}

unsafe fn trace_operation(operation: OperationCode) {
    if unsafe { read_volatile(0x4089ebec as *const u32) } != 1 { return; }
    let trace = Address32(unsafe { read_volatile(0x40a5c8b0 as *const u32) });
    let count = unsafe { trace.offset(ByteOffset(32)).word() };
    if count >= 0x1540 { return; }
    unsafe { trace.offset(ByteOffset(32)).write_word(count + 1) };
    let timer = unsafe { read_volatile(0xfff91000 as *const u32) };
    let entry = trace.offset(ByteOffset(36 + count * 12));
    unsafe { entry.offset(ByteOffset(4)).write_word(30000u32.wrapping_sub(timer)) };
    let ticks = unsafe { read_volatile(0x400c0800 as *const u32) };
    unsafe { entry.offset(ByteOffset(8)).write_word(ticks); entry.write_word(operation.0) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_trace_operation(operation: u32) {
    unsafe { trace_operation(OperationCode(operation)) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_trace_begin(operation: u32) {
    unsafe { trace_operation(OperationCode(operation)) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_trace_end(operation: u32) {
    unsafe { trace_operation(OperationCode(operation | 0x10000000)) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_trace_operation_end(operation: u32) {
    unsafe { trace_operation(OperationCode(operation | 0x20000000)) };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct CalendarFields(pub [u8; 6]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OffsetDirection { Add, Subtract }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalendarOffset { direction: OffsetDirection, hours: u8, minutes: u8 }
impl CalendarOffset {
    pub fn from_profile(direction: u8, hours: u8, minutes: u8) -> Self {
        Self { direction: if direction == 0 { OffsetDirection::Add } else { OffsetDirection::Subtract }, hours: if hours <= 23 { hours } else { 0 }, minutes: if minutes <= 59 { minutes } else { 0 } }
    }
}
const MONTH_DAYS: [u8; 12] = [31,28,31,30,31,30,31,31,30,31,30,31];
fn month_days(year: u8, month: u8) -> u8 {
    if month == 2 && year & 3 == 0 { 29 } else { MONTH_DAYS[usize::from(month) - 1] }
}
pub fn adjust(CalendarFields(mut fields): CalendarFields, offset: CalendarOffset) -> CalendarFields {
    match offset.direction {
        OffsetDirection::Add => {
            fields[4] = fields[4].wrapping_add(offset.minutes);
            if fields[4] > 59 { fields[4] = fields[4].wrapping_sub(60); fields[3] = fields[3].wrapping_add(1); }
            fields[3] = fields[3].wrapping_add(offset.hours);
            if fields[3] > 23 { fields[3] = fields[3].wrapping_sub(24); fields[2] = fields[2].wrapping_add(1); }
            if fields[2] > month_days(fields[0], fields[1]) {
                fields[2] = 1;
                fields[1] = fields[1].wrapping_add(1);
            }
            if fields[1] > 12 { fields[1] = 1; fields[0] = fields[0].wrapping_add(1); }
            if fields[0] > 50 { fields[0] = 0; }
        }
        OffsetDirection::Subtract => {
            let borrowed = fields[4] < offset.minutes;
            fields[4] = if borrowed { fields[4].wrapping_add(60).wrapping_sub(offset.minutes) } else { fields[4] - offset.minutes };
            let hours = offset.hours + u8::from(borrowed);
            if fields[3] >= hours { fields[3] -= hours; }
            else {
                fields[3] = fields[3].wrapping_add(24).wrapping_sub(hours);
                fields[2] = fields[2].wrapping_sub(1);
                if fields[2] == 0 {
                    fields[1] = fields[1].wrapping_sub(1);
                    if fields[1] == 0 { fields[1] = 12; fields[0] = if fields[0] == 0 { 50 } else { fields[0] - 1 }; }
                    fields[2] = month_days(fields[0], fields[1]);
                }
            }
        }
    }
    CalendarFields(fields)
}

#[cfg(target_arch = "arm")]
mod native {
    use super::*;
    use core::ptr::{read_volatile, write_volatile};
    extern "C" {
        fn xm1_power_interrupt_lock() -> u32;
        fn xm1_power_interrupt_unlock(saved: u32);
    }
    unsafe fn byte(at: u32) -> u8 { read_volatile(at as *const u8) }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_diagnostic_calendar_header(out: *mut u8) {
        let adjusted = byte(0x409c281c + 0x5c) >> 7 == 1;
        let saved = xm1_power_interrupt_lock();
        let fields = CalendarFields(core::array::from_fn(|index| byte(0x409c34b8 + index as u32)));
        xm1_power_interrupt_unlock(saved);
        let fields = if adjusted {
            let direction = byte(0x409c281c + 0x5b) >> 7;
            let hours = byte(0x409c281c + 0x28) & 0x1f;
            let minutes = (byte(0x409c281c + 0x20) >> 6) * 15;
            adjust(fields, CalendarOffset::from_profile(direction, hours, minutes))
        } else { fields };
        for (index, value) in fields.0.into_iter().enumerate() { write_volatile(out.add(index), value); }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalendarMonth(u8);
impl CalendarMonth {
    pub const fn parse(value: u8) -> Option<Self> { if value >= 1 && value <= 12 { Some(Self(value)) } else { None } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferTimestamp { bytes: [u8; 6], month: CalendarMonth }
impl TransferTimestamp {
    pub const fn parse(bytes: [u8; 6]) -> Option<Self> {
        match CalendarMonth::parse(bytes[1]) { Some(month) => Some(Self { bytes, month }), None => None }
    }
    pub const fn bytes(&self) -> &[u8; 6] { &self.bytes }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowMinutes(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowDisposition { BeforeStart, Within, Expired }

pub fn classify_window(start: TransferTimestamp, now: TransferTimestamp, duration: WindowMinutes) -> WindowDisposition {
    if start.bytes[..5] > now.bytes[..5] { return WindowDisposition::BeforeStart; }
    let mut end = start.bytes;
    let minutes = duration.0.wrapping_add(u32::from(end[4]));
    end[4] = (minutes % 60) as u8;
    let hours = (minutes / 60).wrapping_add(u32::from(end[3])) as u8;
    end[3] = hours % 24;
    end[2] = end[2].wrapping_add(hours / 24);
    const MONTH_DAYS: [u8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if end[2] > MONTH_DAYS[usize::from(start.month.0 - 1)] {
        if start.month.0 == 2 {
            if end[0] & 3 != 0 || end[2] > 29 { end[2] = 1; end[1] = 3; }
        } else { end[2] = 1; end[1] = end[1].wrapping_add(1); }
    }
    if end[1] > 12 { end[1] = 1; end[0] = end[0].wrapping_add(1); }
    if now.bytes[..5] > end[..5] { WindowDisposition::Expired } else { WindowDisposition::Within }
}

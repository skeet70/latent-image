// X-M1 1.50, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.
// Selector returns and transition cells are confirmed against image 5 bytes.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Backend { Settings, Auxiliary, Motor, Slot }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordingOffset(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GuestAddress(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation { Reset, Apply, Poll }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingGroup { First, Second }

pub trait CaptureBackendIo {
    fn read(&self, offset: RecordingOffset) -> u32;
    fn write(&mut self, offset: RecordingOffset, value: u32);
    fn read_guest(&self, address: GuestAddress) -> u32;
    fn write_guest_byte(&mut self, address: GuestAddress, value: u8);
    fn state(&self, backend: Backend) -> u16;
    fn set_state(&mut self, backend: Backend, state: u16);
    fn set_row(&mut self, backend: Backend, row: u16);
    fn message(&self) -> u16;
    fn repeat_current(&mut self);
    fn defer(&mut self);
    fn missing_state(&mut self);
    fn post_message(&mut self, message: u16);
    fn notify(&mut self);
    fn submit_motor(&mut self, mode: u32);
    fn submit_slot(&mut self, mode: u32);
    fn group(&mut self, operation: Operation, group: SettingGroup, flags: u8) -> i32;
    fn motor_setting(&mut self, argument: GuestAddress) -> i32;
    fn selected_settings(&mut self, kind: u32, settings: GuestAddress, selected: GuestAddress) -> i32;
    fn output_settings(&mut self, kind: u32, settings: GuestAddress) -> i32;
    fn configuration_678(&self) -> u32;
}
fn read(io: &impl CaptureBackendIo, offset: u32) -> u32 { io.read(RecordingOffset(offset)) }
fn write(io: &mut impl CaptureBackendIo, offset: u32, value: u32) { io.write(RecordingOffset(offset), value); }
fn test(io: &impl CaptureBackendIo, bit: u32) -> bool { read(io, 0x93e0 + bit / 32 * 4) & (1 << (bit % 32)) != 0 }
fn clear(io: &mut impl CaptureBackendIo, bit: u32) {
    let offset = 0x93e0 + bit / 32 * 4;
    let value = read(io, offset) & !(1 << (bit % 32));
    write(io, offset, value);
}
fn clear_group(io: &mut impl CaptureBackendIo, bits: &[u32]) { for &bit in bits { clear(io, bit); } }
fn clear_bitmaps(io: &mut impl CaptureBackendIo) { for offset in [0x93e0, 0x93e4, 0x93e8, 0x93ec] { write(io, offset, 0); } }
fn copy_bitmaps(io: &mut impl CaptureBackendIo) {
    clear_bitmaps(io);
    let requested = read(io, 0x93d8);
    if requested != 0 { for offset in [0, 4, 8] { let word = io.read_guest(GuestAddress(requested + offset)); write(io, 0x93e0 + offset, word); } }
    let selected = read(io, 0x93dc);
    if selected != 0 { let word = io.read_guest(GuestAddress(selected)); write(io, 0x93ec, word); }
}
const FIRST: [u32; 5] = [0x16, 0x17, 0x18, 0x19, 0x1a];
const SECOND: [u32; 5] = [0x1d, 0x20, 0x1f, 0x1e, 0x21];
fn flags(io: &impl CaptureBackendIo, bits: &[u32; 5]) -> u8 {
    bits.iter().enumerate().fold(0, |mask, (index, &bit)| mask | (u8::from(test(io, bit)) << index))
}
fn special_settings(io: &impl CaptureBackendIo, settings: GuestAddress) -> bool {
    io.read_guest(settings) == 0x2c && io.read_guest(GuestAddress(settings.0 + 0x54)).wrapping_sub(7) < 3
}
fn settings_apply(io: &mut impl CaptureBackendIo) -> i32 {
    let settings = GuestAddress(read(io, 0x93d4));
    if test(io, 0x15) {
        if read(io, 0x4cd4) != 1 && read(io, 0x9620) != 1 { let result = io.motor_setting(GuestAddress(0)); if result != 1 { return result; } }
        clear(io, 0x15);
    }
    let mut first = flags(io, &FIRST);
    if first != 0 {
        if special_settings(io, settings) { first = 0x3f; } else if first != 0x10 { first |= 0x20; }
        let result = io.group(Operation::Apply, SettingGroup::First, first);
        if result != 1 { return result; }
        clear_group(io, &FIRST);
    }
    for bit in [0x1b, 0x1c] {
        if test(io, bit) {
            let result = if bit == 0x1b { io.selected_settings(0x13, settings, GuestAddress(0x01526a78 + 0x93ec)) } else { io.output_settings(2, settings) };
            if result != 1 { return result; }
            clear(io, bit);
        }
    }
    let mut second = flags(io, &SECOND);
    if second != 0 {
        if !special_settings(io, settings) {
            if io.configuration_678() == 1 && second != 0x10 { second |= 0x20; }
            let result = io.group(Operation::Apply, SettingGroup::Second, second);
            if result != 1 { return result; }
        }
        clear_group(io, &SECOND);
    }
    if test(io, 0x22) {
        let result = io.selected_settings(2, settings, GuestAddress(0x01526a78 + 0x93ec));
        if result != 1 { return result; }
        clear(io, 0x22);
    }
    if test(io, 0x23) {
        if read(io, 0x4cd4) != 1 && read(io, 0x9620) != 1 { let result = io.motor_setting(settings); if result != 1 { return result; } }
        clear(io, 0x23);
    }
    1
}
fn settings_resume(io: &mut impl CaptureBackendIo) -> i32 {
    let bit = [0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x20, 0x1f, 0x1e, 0x21, 0x22, 0x23].into_iter().find(|&bit| test(io, bit));
    let settings = GuestAddress(read(io, 0x93d4));
    let (result, group) = match bit {
        Some(0x15 | 0x23) => (io.motor_setting(GuestAddress(3)), &[][..]),
        Some(0x16..=0x1a) => (io.group(Operation::Poll, SettingGroup::First, 0), &FIRST[..]),
        Some(0x1b | 0x22) => (io.selected_settings(0x19, settings, GuestAddress(0x01526a78 + 0x93ec)), &[][..]),
        Some(0x1c) => (io.output_settings(0x19, settings), &[][..]),
        Some(0x1d..=0x21) => (io.group(Operation::Poll, SettingGroup::Second, 0), &SECOND[..]),
        _ => { io.repeat_current(); return 0; }
    };
    if result != 1 { return result; }
    if group.is_empty() { clear(io, bit.unwrap()); } else { clear_group(io, group); }
    let result = settings_apply(io);
    if result == 1 { io.set_state(Backend::Settings, 0); clear_bitmaps(io); }
    result
}

pub fn settings_request(io: &mut impl CaptureBackendIo, kind: u32, settings: GuestAddress, requested: GuestAddress, selected: GuestAddress) -> i32 {
    write(io, 0x93d0, kind); write(io, 0x93d4, settings.0); write(io, 0x93d8, requested.0); write(io, 0x93dc, selected.0);
    let row = match kind { 0 => 0, 2 => 1, 0x19 => 2, _ => 3 };
    io.set_row(Backend::Settings, row);
    match usize::from(row) * 2 + usize::from(io.state(Backend::Settings)) {
        0 | 4 => 1,
        1 => 0,
        2 => {
            copy_bitmaps(io);
            let result = settings_apply(io);
            io.set_state(Backend::Settings, u16::from(result != 1));
            if result == 1 { clear_bitmaps(io); }
            result
        }
        5 => settings_resume(io),
        3 | 6 | 7 => { io.defer(); 0 }
        _ => { io.missing_state(); 0 }
    }
}

pub fn auxiliary_request(io: &mut impl CaptureBackendIo, kind: u32, argument: u32) -> i32 {
    write(io, 0x943c, kind); write(io, 0x9440, argument);
    let row = match kind { 2 => 0, 0 | 1 => 1, 3 if io.message() == 0x13bb => 2, 3 => 3, _ => 4 };
    io.set_row(Backend::Auxiliary, row);
    match usize::from(row) * 2 + usize::from(io.state(Backend::Auxiliary)) {
        0 | 6 => 1,
        1 | 7 => 0,
        2 => {
            let kind = read(io, 0x943c);
            let argument = read(io, 0x9440);
            write(io, 0x9444, kind); write(io, 0x9448, argument);
            io.post_message(0x13bb); io.set_state(Backend::Auxiliary, 1); 0
        }
        5 => { io.set_state(Backend::Auxiliary, 0); 1 }
        3 | 4 | 8 | 9 => { io.defer(); 0 }
        _ => { io.missing_state(); 0 }
    }
}
pub fn motor_request(io: &mut impl CaptureBackendIo, kind: u32, argument: u32, output: Option<GuestAddress>) -> i32 {
    if kind == 1 { write(io, 0x9450, argument); }
    write(io, 0x9454, kind);
    let row = match kind { 0 => 0, 1 => 1, 2 if io.message() == 0x13bc => 2, 2 => 3, _ => 4 };
    io.set_row(Backend::Motor, row);
    let result = match usize::from(row) * 2 + usize::from(io.state(Backend::Motor)) {
        0 | 6 => 1,
        1 | 7 => 0,
        2 => { let mode = read(io, 0x9450); write(io, 0x9458, mode); io.submit_motor(if mode <= 3 { mode } else { 2 }); io.set_state(Backend::Motor, 1); 0 }
        5 => { io.set_state(Backend::Motor, 0); 1 }
        3 | 4 | 8 | 9 => { io.defer(); 0 }
        _ => { io.missing_state(); 0 }
    };
    if let Some(output) = output { io.write_guest_byte(output, read(io, 0x945c) as u8); }
    result
}
pub fn slot_request(io: &mut impl CaptureBackendIo, request: Option<[u32; 2]>) -> u32 {
    if let Some([kind, argument]) = request {
        write(io, 0xc390, kind); write(io, 0xc394, argument);
        let row = match kind { 0 => 0, 1 => 1, 2 if io.message() == 0x13e4 => 2, 2 => 3, _ => 4 };
        io.set_row(Backend::Slot, row);
        match usize::from(row) * 2 + usize::from(io.state(Backend::Slot)) {
            0 | 4 | 6 => write(io, 0xc398, 0),
            1 | 7 => write(io, 0xc398, 1),
            2 => { write(io, 0xc39c, 0); let argument = read(io, 0xc394); write(io, 0xc39c, argument); io.submit_slot(argument); write(io, 0xc398, 1); io.set_state(Backend::Slot, 1); }
            5 => { let settings = read(io, 0x3540); io.write_guest_byte(GuestAddress(settings + 0x9f0), read(io, 0xc39c) as u8); io.notify(); write(io, 0xc398, 0); io.set_state(Backend::Slot, 0); }
            3 | 8 | 9 => io.defer(),
            _ => io.missing_state(),
        }
    } else { io.missing_state(); }
    read(io, 0xc398)
}

pub fn parameter_class() -> i32 { 1 }

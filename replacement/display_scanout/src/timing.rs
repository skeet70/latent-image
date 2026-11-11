use crate::{Bank, Engine, Hardware, Register};
use calibration_liveview::{Calibration, CalibrationByte, CalibrationOffset, Width};
use crate::timing_keys as keys;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PendingUpdate(pub u32);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ForceUpdate(pub u32);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DefaultSelection(pub u8);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TimingRegisters {
    pub control: u32,
    pub dimensions: u32,
    pub mode_pulse: u32,
    pub mode_interval: u32,
    pub route_pulse: u32,
    pub route_interval: u32,
    pub mode_delay: u32,
    pub output_pulse: u32,
    pub output_interval: u32,
    pub secondary_output_pulse: u32,
    pub secondary_output_interval: u32,
    pub threshold: u32,
    pub defaults: u32,
    pub route_adjustment: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimingState {
    pub selected_engine: Engine,
    pub original: TimingConfiguration,
    pub pending: PendingUpdate,
    pub force: ForceUpdate,
    pub defaults: DefaultSelection,
    pub registers: TimingRegisters,
}
impl TimingState {
    pub fn zero() -> Self {
        Self {
            selected_engine: Engine::Zero,
            original: TimingConfiguration { mode: TimingMode(0), route: TimingRoute::Zero, rate: TimingRate(0) },
            pending: PendingUpdate(0),
            force: ForceUpdate(0),
            defaults: DefaultSelection(0),
            registers: TimingRegisters::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FormatterRegisters {
    pub control: u32,
    pub input_control: u32,
    pub dimensions: u32,
    pub output_control: u32,
    pub mode: u32,
    pub parameters_38: [u32; 5],
    pub parameters_64: [u32; 5],
    pub channel_control: u32,
    pub channel_parameters: [[u32; 6]; 3],
    pub final_control: u32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FormatterState {
    pub pending: PendingUpdate,
    pub registers: FormatterRegisters,
}
impl FormatterState {
    pub fn zero() -> Self { Self::default() }
}

pub trait CalibrationRead {
    fn read_byte(&mut self, offset: CalibrationOffset) -> CalibrationByte;
}
impl CalibrationRead for Calibration<'_> {
    fn read_byte(&mut self, offset: CalibrationOffset) -> CalibrationByte {
        CalibrationByte(self.read(offset, Width::Byte).unwrap().0 as u8)
    }
}
fn byte(calibration: &mut impl CalibrationRead, offset: CalibrationOffset) -> u32 {
    u32::from(calibration.read_byte(offset).0)
}
fn pair(calibration: &mut impl CalibrationRead, offsets: keys::CalibrationPair) -> u32 {
    let high = byte(calibration, offsets.high);
    byte(calibration, offsets.low) | (high << 8)
}
fn reg(engine: Engine, bank: Bank, offset: u16) -> Register { Register { engine, bank, offset } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimingMode(u8);
impl TimingMode {
    pub fn parse(value: u32) -> Option<Self> { (value < 10).then_some(Self(value as u8)) }
    pub fn raw(self) -> u32 { u32::from(self.0) }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimingRoute { Zero, One, Two, Direct }
impl TimingRoute {
    pub fn parse(value: u32) -> Option<Self> {
        match value { 0 => Some(Self::Zero), 1 => Some(Self::One), 2 => Some(Self::Two), 3 => Some(Self::Direct), _ => None }
    }
    pub fn raw(self) -> u32 { match self { Self::Zero => 0, Self::One => 1, Self::Two => 2, Self::Direct => 3 } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimingRate(u8);
impl TimingRate {
    pub fn parse(value: u32) -> Option<Self> { (value < 6).then_some(Self(value as u8)) }
    pub fn raw(self) -> u32 { u32::from(self.0) }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimingConfiguration { pub mode: TimingMode, pub route: TimingRoute, pub rate: TimingRate }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImmediatePacket { pub interval: u16, pub selector: u8, pub flag: u8, pub selection: DefaultSelection }

pub fn acquire_defaults(selection: DefaultSelection, word: &mut u32, calibration: &mut impl CalibrationRead) {
    if selection.0 & 2 == 0 { *word = (*word & !7) | (byte(calibration, keys::DEFAULT_SELECTOR) & 7); }
    if selection.0 & 1 == 0 { *word = (*word & !0x0fff0000) | ((pair(calibration, keys::DEFAULT_INTERVAL) & 0xfff) << 16); }
    if selection.0 & 4 == 0 { *word = (*word & !(1 << 28)) | ((byte(calibration, keys::DEFAULT_FLAG) & 1) << 28); }
}

pub fn immediate(engine: Engine, state: &mut TimingState, packet: ImmediatePacket, calibration: &mut impl CalibrationRead, hardware: &mut impl Hardware) -> u32 {
    state.defaults = packet.selection;
    let mut word = state.registers.defaults;
    acquire_defaults(packet.selection, &mut word, calibration);
    if packet.selection.0 & 1 != 0 { word = (word & !0x0fff0000) | ((u32::from(packet.interval) & 0xfff) << 16); }
    if packet.selection.0 & 2 != 0 { word = (word & !7) | u32::from(packet.selector & 7); }
    if packet.selection.0 & 4 != 0 { word = (word & !(1 << 28)) | (u32::from(packet.flag & 1) << 28); }
    state.registers.defaults = word;
    hardware.write(reg(engine, Bank::Timing, 0x34), word);
    0
}

pub fn flush(engine: Engine, state: &mut TimingState, hardware: &mut impl Hardware) -> u32 {
    if state.pending.0 == 0 { return 0; }
    state.pending = PendingUpdate(0);
    let registers = &state.registers;
    for (offset, value) in [
        (8, registers.mode_pulse), (12, registers.mode_interval),
        (16, registers.route_pulse), (20, registers.route_interval),
        (24, registers.mode_delay), (28, registers.output_pulse),
        (32, registers.output_interval),
    ] { hardware.write(reg(engine, Bank::Timing, offset), value); }
    if engine == Engine::One {
        hardware.write(reg(engine, Bank::Timing, 0x24), registers.secondary_output_pulse);
        hardware.write(reg(engine, Bank::Timing, 0x28), registers.secondary_output_interval);
    }
    for (offset, value) in [(0x2c, registers.threshold), (0x34, registers.defaults), (0x38, registers.route_adjustment)] {
        hardware.write(reg(engine, Bank::Timing, offset), value);
    }
    let control = registers.control;
    let current = hardware.read(reg(engine, Bank::Timing, 0));
    if (current & !0x100) | (control & 0x100) == control
        && hardware.read(reg(engine, Bank::Timing, 4)) == registers.dimensions
        && state.force.0 != 1 { return 0; }
    state.force = ForceUpdate(0);
    hardware.write(reg(engine, Bank::Timing, 0), control);
    if registers.dimensions != 0 { hardware.write(reg(engine, Bank::Timing, 4), registers.dimensions); }
    1
}

#[inline(never)]
pub fn reset(engine: Engine, hardware: &mut impl Hardware) {
    let register = reg(engine, Bank::Scanout, 0);
    let saved = hardware.read(register);
    hardware.write(register, 0x10000);
    let value = hardware.read(register);
    hardware.write(register, value & !0x10000);
    while hardware.read(register) & 0x10000 == 0 {}
    hardware.write(register, saved | 0x10000);
}

pub fn wait_input_ready(engine: Engine, hardware: &mut impl Hardware) {
    let register = reg(engine, Bank::Timing, 0);
    let control = hardware.read(register);
    hardware.write(register, control & !0x10000);
    while hardware.read(reg(engine, Bank::Timing, 0x30)) & 2 == 0 { hardware.yield_zero(); }
}

pub fn formatter_flush(engine: Engine, state: &mut FormatterState, hardware: &mut impl Hardware) {
    if state.pending.0 == 0 { return; }
    state.pending = PendingUpdate(0);
    let registers = &state.registers;
    for (offset, value) in [
        (0, registers.control), (4, registers.input_control),
        (0x20, registers.dimensions), (0xc8, registers.output_control), (0x34, registers.mode),
    ] { hardware.write(reg(engine, Bank::Formatter, offset), value); }
    for (start, words) in [(0x38, &registers.parameters_38), (0x64, &registers.parameters_64)] {
        for (n, &value) in words.iter().enumerate() {
            hardware.write(reg(engine, Bank::Formatter, start + n as u16 * 4), value);
        }
    }
    hardware.write(reg(engine, Bank::Formatter, 0x78), registers.channel_control);
    for (channel, words) in registers.channel_parameters.iter().enumerate() {
        for (n, &value) in words.iter().enumerate() {
            hardware.write(reg(engine, Bank::Formatter, 0x7c + channel as u16 * 24 + n as u16 * 4), value);
        }
    }
    hardware.write(reg(engine, Bank::Formatter, 0xc4), registers.final_control);
}

pub fn formatter_strobe(engine: Engine, hardware: &mut impl Hardware) {
    let register = reg(engine, Bank::Formatter, 0);
    let token = hardware.enter_critical();
    let control = hardware.read(register);
    hardware.write(register, control | (1 << 28));
    hardware.leave_critical(token);
    while hardware.read(register) & (1 << 28) != 0 {}
}

fn field(word: &mut u32, shift: u32, mask: u32, value: u32) {
    *word = (*word & !(mask << shift)) | ((value & mask) << shift);
}

pub fn stage(engine: Engine, states: &mut [TimingState; 2], packet: &mut TimingConfiguration, calibration: &mut impl CalibrationRead) {
    let index = engine.index();
    states[0].selected_engine = engine;
    states[1].selected_engine = engine;
    let state = &mut states[index];
    state.pending = PendingUpdate(0);
    state.original = *packet;
    if packet.rate.0 == 0 || packet.rate.0 == 3 { packet.rate = TimingRate(2); }
    let mode = keys::mode(packet.mode);
    let route = keys::route(packet.route, packet.rate);
    let direct = packet.route == TimingRoute::Direct;
    let registers = &mut state.registers;
    registers.control = 0x100;
    field(&mut registers.control, 16, 1, byte(calibration, keys::CONTROL_ENABLE));
    if direct {
        registers.control = (registers.control & !0xff) | (byte(calibration, mode.control.flags) & 0xff);
        field(&mut registers.control, 8, 1, byte(calibration, mode.control.clock));
    } else {
        for (key, shift) in route.control.into_iter().zip([0,1,4]) { field(&mut registers.control, shift, 1, byte(calibration, key)); }
        registers.control &= !8;
    }
    let dimensions = if direct { mode.dimensions } else { route.dimensions };
    registers.dimensions = pair(calibration, dimensions.first) & 0xfff;
    field(&mut registers.dimensions, 16, 0xfff, pair(calibration, dimensions.second));
    registers.mode_pulse = 0;
    field(&mut registers.mode_pulse, 28, 1, byte(calibration, mode.pulse.first_flag));
    field(&mut registers.mode_pulse, 16, 0xfff, pair(calibration, mode.pulse.first));
    field(&mut registers.mode_pulse, 12, 1, byte(calibration, mode.pulse.second_flag));
    field(&mut registers.mode_pulse, 0, 0xfff, pair(calibration, mode.pulse.second));
    registers.mode_interval = (pair(calibration, mode.interval.first) & 0xfff) << 16;
    field(&mut registers.mode_interval, 0, 0xfff, pair(calibration, mode.interval.second));
    registers.route_pulse = 0;
    registers.route_interval = 0;
    if !direct {
        field(&mut registers.route_pulse, 28, 1, byte(calibration, route.pulse.first_flag));
        field(&mut registers.route_pulse, 12, 1, byte(calibration, route.pulse.second_flag));
        field(&mut registers.route_pulse, 16, 0xfff, pair(calibration, route.pulse.first));
        field(&mut registers.route_pulse, 0, 0xfff, pair(calibration, route.pulse.second));
        registers.route_interval = (pair(calibration, route.interval.first) & 0xfff) << 16;
        field(&mut registers.route_interval, 0, 0xfff, pair(calibration, route.interval.second));
    }
    registers.mode_delay = match mode.delay { Some(key) => byte(calibration, key), None => 0 };
    let output = if direct { mode.output } else { route.output };
    registers.output_pulse = pair(calibration, output.interval) & 0xfff;
    field(&mut registers.output_pulse, 12, 1, byte(calibration, output.enabled));
    let output_interval = if direct { mode.output_interval } else { route.output_interval };
    registers.output_interval = pair(calibration, output_interval.first) & 0xfff;
    field(&mut registers.output_interval, 16, 0xfff, pair(calibration, output_interval.second));
    registers.secondary_output_pulse = 0;
    registers.secondary_output_interval = 0;
    if !direct {
        registers.secondary_output_pulse = pair(calibration, route.secondary_output.interval) & 0xfff;
        field(&mut registers.secondary_output_pulse, 12, 1, byte(calibration, route.secondary_output.enabled));
        registers.secondary_output_interval = pair(calibration, route.secondary_output_interval.first) & 0xfff;
        field(&mut registers.secondary_output_interval, 16, 0xfff, pair(calibration, route.secondary_output_interval.second));
    }
    let half = (registers.dimensions & 0xfff) >> 1;
    let margin = if matches!(packet.mode.raw(), 1 | 4 | 6) { 7 } else { 9 };
    let adjusted = half.wrapping_sub(margin) & 0x1fff;
    field(&mut registers.threshold, 0, 0x1fff, adjusted - adjusted / 30);
    acquire_defaults(state.defaults, &mut registers.defaults, calibration);
    registers.route_adjustment = 0;
    if !direct {
        field(&mut registers.route_adjustment, 16, 1, byte(calibration, route.adjustment.first_flag));
        field(&mut registers.route_adjustment, 17, 0x1ff, pair(calibration, route.adjustment.phase));
        field(&mut registers.route_adjustment, 0, 0xfff, pair(calibration, route.adjustment.interval));
        field(&mut registers.route_adjustment, 26, 1, byte(calibration, route.adjustment.second_flag));
    }
    state.pending = PendingUpdate(1);
}

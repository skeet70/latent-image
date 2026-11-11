use crate::timing::{self, CalibrationRead, FormatterState, PendingUpdate};
use crate::{Bank, Engine, Error, Hardware, Register};
use calibration_liveview::CalibrationOffset;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FormatterMode(pub u32);
impl FormatterMode {
    pub const fn raw(self) -> u32 { self.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferMode(u8);
impl BufferMode {
    pub const fn parse(value: u32) -> Option<Self> {
        if value <= 10 { Some(Self(value as u8)) } else { None }
    }
    pub const fn raw(self) -> u32 { self.0 as u32 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceMode { Zero, One, Three }
impl SourceMode {
    pub const fn parse(value: u32) -> Option<Self> {
        match value { 0 => Some(Self::Zero), 1 => Some(Self::One), 3 => Some(Self::Three), _ => None }
    }
    pub const fn raw(self) -> u32 { match self { Self::Zero => 0, Self::One => 1, Self::Three => 3 } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceSelector { Zero, One, Two, Three }
impl SourceSelector {
    pub const fn parse(value: u32) -> Option<Self> {
        match value { 0 => Some(Self::Zero), 1 => Some(Self::One), 2 => Some(Self::Two), 3 => Some(Self::Three), _ => None }
    }
    pub const fn raw(self) -> u32 { match self { Self::Zero => 0, Self::One => 1, Self::Two => 2, Self::Three => 3 } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flip { Normal, Flipped }
impl Flip {
    pub const fn parse(value: u32) -> Option<Self> {
        match value { 0 => Some(Self::Normal), 1 => Some(Self::Flipped), _ => None }
    }
    pub const fn raw(self) -> u32 { match self { Self::Normal => 0, Self::Flipped => 1 } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferSlot { Zero, One, Two, Three }
impl BufferSlot {
    pub const fn parse(value: u32) -> Option<Self> {
        match value { 0 => Some(Self::Zero), 1 => Some(Self::One), 2 => Some(Self::Two), 3 => Some(Self::Three), _ => None }
    }
    pub const fn raw(self) -> u32 { self.index() as u32 }
    pub const fn index(self) -> usize { match self { Self::Zero => 0, Self::One => 1, Self::Two => 2, Self::Three => 3 } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceCategory { Disabled, One, Two, Three, Four }
impl SourceCategory {
    pub const fn parse(value: u32) -> Option<Self> {
        match value { 0 => Some(Self::Disabled), 1 => Some(Self::One), 2 => Some(Self::Two), 3 => Some(Self::Three), 4 => Some(Self::Four), _ => None }
    }
    pub const fn raw(self) -> u32 { match self { Self::Disabled => 0, Self::One => 1, Self::Two => 2, Self::Three => 3, Self::Four => 4 } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceConfiguration {
    pub mode: SourceMode,
    pub selector: SourceSelector,
    pub flip: Flip,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SourceGeometry {
    pub extent: u32,
    pub origin: u32,
    pub limit: u32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChannelTransform {
    pub coefficients: u32,
    pub offset: u32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AddressRegisters {
    pub geometry: SourceGeometry,
    pub channels: [ChannelTransform; 3],
    pub alternate_channels: [ChannelTransform; 3],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrimaryConfigurationKey(pub u32);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SecondaryConfigurationKey(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatterConfiguration {
    pub mode: FormatterMode,
    pub primary_key: PrimaryConfigurationKey,
    pub source: BufferSlot,
    pub secondary_key: SecondaryConfigurationKey,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlaneRegisters {
    pub control: u32,
    pub dimensions: u32,
    pub start: u32,
    pub end: u32,
    pub offset: u32,
    pub limit: u32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EnabledState(pub u32);
impl EnabledState {
    pub const fn is_enabled(self) -> bool { self.0 != 0 }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecordedSourceMode(pub u32);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecordedSelector(pub u32);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecordedFlip(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelFormatterState {
    pub mode: FormatterMode,
    pub primary_key: PrimaryConfigurationKey,
    pub secondary_key: SecondaryConfigurationKey,
    pub active_slot: BufferSlot,
    pub enabled: EnabledState,
    pub source_mode: RecordedSourceMode,
    pub previous_source_mode: RecordedSourceMode,
    pub selector: RecordedSelector,
    pub flip: RecordedFlip,
    pub slot_modes: [BufferMode; 4],
    pub registers: AddressRegisters,
    pub formatter: FormatterState,
    pub pipeline_control: u32,
    pub plane: PlaneRegisters,
}
impl PanelFormatterState {
    pub fn zero() -> Self {
        Self {
            mode: FormatterMode(0), active_slot: BufferSlot::Zero, enabled: EnabledState(0),
            primary_key: PrimaryConfigurationKey(0), secondary_key: SecondaryConfigurationKey(0),
            source_mode: RecordedSourceMode(0), previous_source_mode: RecordedSourceMode(0),
            selector: RecordedSelector(0), flip: RecordedFlip(0),
            slot_modes: [BufferMode(0); 4], registers: AddressRegisters::default(),
            formatter: FormatterState::zero(), pipeline_control: 0, plane: PlaneRegisters::default(),
        }
    }
    pub fn configuration(&self) -> FormatterConfiguration {
        FormatterConfiguration { mode: self.mode, primary_key: self.primary_key, source: self.active_slot, secondary_key: self.secondary_key }
    }
}

pub fn source_category(state: &PanelFormatterState) -> SourceCategory {
    if !state.enabled.is_enabled() { return SourceCategory::Disabled; }
    if matches!(state.mode.raw(), 1 | 4 | 5 | 7 | 8 | 9) {
        if state.source_mode.0 == SourceMode::One.raw() && state.selector.0 != SourceSelector::Three.raw() {
            SourceCategory::Three
        } else if state.source_mode.0 == SourceMode::Three.raw() {
            SourceCategory::Four
        } else if state.selector.0 == SourceSelector::Three.raw() || state.flip.0 == Flip::Flipped.raw() {
            SourceCategory::One
        } else {
            SourceCategory::Two
        }
    } else if state.selector.0 == SourceSelector::Three.raw() {
        SourceCategory::One
    } else {
        SourceCategory::Two
    }
}

#[derive(Clone, Copy)]
enum GeometryCalibration { ModeDependent, Fixed, Alternate, Flipped }
fn geometry_calibration(state: &PanelFormatterState, category: SourceCategory) -> GeometryCalibration {
    match category {
        SourceCategory::Disabled => GeometryCalibration::Fixed,
        SourceCategory::Four => GeometryCalibration::Alternate,
        _ if state.selector.0 == SourceSelector::Three.raw() => GeometryCalibration::Flipped,
        SourceCategory::Three if state.previous_source_mode.0 == SourceMode::Three.raw() => GeometryCalibration::Alternate,
        _ if state.mode.raw() == 3 => GeometryCalibration::ModeDependent,
        _ if state.flip.0 == Flip::Flipped.raw() => GeometryCalibration::Flipped,
        _ => GeometryCalibration::ModeDependent,
    }
}
fn pair(calibration: &mut impl CalibrationRead, high: CalibrationOffset) -> u32 {
    let upper = calibration.read_byte(high).0;
    let lower = calibration.read_byte(CalibrationOffset::parse(high.byte_offset() + 1).unwrap()).0;
    (u32::from(upper) << 8) | u32::from(lower)
}
fn key(offset: usize) -> CalibrationOffset { CalibrationOffset::parse(offset).unwrap() }
fn prepare_geometry(state: &mut PanelFormatterState, mode: BufferMode, category: SourceCategory, calibration: &mut impl CalibrationRead) {
    if state.active_slot != BufferSlot::One {
        state.registers.geometry = SourceGeometry::default();
        return;
    }
    let class = geometry_calibration(state, category);
    let (base, mode_offset) = match class {
        GeometryCalibration::ModeDependent => (0x4c4, usize::from(mode.0.saturating_sub(5)) * 2),
        GeometryCalibration::Fixed => (0x4c4, 0),
        GeometryCalibration::Alternate => (0x53c, usize::from(mode.0.saturating_sub(5)) * 2),
        GeometryCalibration::Flipped => (0x500, usize::from(mode.0.saturating_sub(5)) * 2),
    };
    let extent = pair(calibration, key(base + mode_offset)).min(0xfff);
    let origin_high = pair(calibration, key(base + 0xc + mode_offset)).min(0x1fff);
    let origin_low = pair(calibration, key(base + 0x18 + mode_offset)).min(0x1fff);
    let limit_high = pair(calibration, key(base + 0x24 + mode_offset)).min(0x1fff);
    let limit_low = pair(calibration, key(base + 0x30 + mode_offset)).min(0x1fff);
    state.registers.geometry = SourceGeometry { extent, origin: origin_low | (origin_high << 16), limit: limit_low | (limit_high << 16) };
}
fn replace(word: &mut u32, shift: u32, value: u32) {
    *word = (*word & !(0x1fff << shift)) | ((value & 0x1fff) << shift);
}
fn prepare_channels(engine: Engine, state: &mut PanelFormatterState, mode: BufferMode, category: SourceCategory, calibration: &mut impl CalibrationRead) {
    for channel in 0..3 {
        let alternate = &mut state.registers.alternate_channels[channel];
        replace(&mut alternate.coefficients, 16, if channel == 0 { 0x400 } else { 0 });
        replace(&mut alternate.coefficients, 0, if channel == 1 { 0x400 } else { 0 });
        replace(&mut alternate.offset, 0, if channel == 2 { 0x400 } else { 0 });
        if engine == Engine::One && state.active_slot == BufferSlot::One {
            let (base, mode_offset, field_stride) = match category {
                SourceCategory::Disabled => (0x8f1 + channel * 6, 0, 2),
                SourceCategory::Three => (0x903 + channel * 6, 0, 2),
                SourceCategory::One => (0x765 + channel * 0x42, usize::from(mode.0) * 2, 0x16),
                SourceCategory::Four => (0x82b + channel * 0x42, usize::from(mode.0) * 2, 0x16),
                SourceCategory::Two => (0x69f + channel * 0x42, usize::from(mode.0) * 2, 0x16),
            };
            let transform = &mut state.registers.channels[channel];
            replace(&mut transform.coefficients, 16, pair(calibration, key(base + mode_offset)));
            replace(&mut transform.coefficients, 0, pair(calibration, key(base + field_stride + mode_offset)));
            replace(&mut transform.offset, 0, pair(calibration, key(base + field_stride * 2 + mode_offset)));
        }
        if state.mode.raw() == 10 {
            let identity = match channel {
                0 => ChannelTransform { coefficients: 0x04000000, offset: 0 },
                1 => ChannelTransform { coefficients: 0x400, offset: 0 },
                _ => ChannelTransform { coefficients: 0, offset: 0x400 },
            };
            state.registers.channels[channel] = identity;
            state.registers.alternate_channels[channel] = identity;
        }
    }
}

pub fn publish_address_registers(engine: Engine, registers: &AddressRegisters, hardware: &mut impl Hardware) {
    for (offset, value) in [(0x24, registers.geometry.extent), (0x28, registers.geometry.origin), (0x2c, registers.geometry.limit)] {
        hardware.write(Register { engine, bank: Bank::Formatter, offset }, value);
    }
    for (start, channels) in [(0x4c, &registers.channels), (0x100, &registers.alternate_channels)] {
        for (channel, transform) in channels.iter().enumerate() {
            let offset = start + channel as u16 * 8;
            hardware.write(Register { engine, bank: Bank::Formatter, offset }, transform.coefficients);
            hardware.write(Register { engine, bank: Bank::Formatter, offset: offset + 4 }, transform.offset);
        }
    }
}

pub fn set_slot_mode(engine: Engine, state: &mut PanelFormatterState, slot: BufferSlot, mode: BufferMode, category: SourceCategory, calibration: &mut impl CalibrationRead, hardware: &mut impl Hardware) -> Result<(), Error> {
    if matches!(slot, BufferSlot::One | BufferSlot::Two) && slot == state.active_slot {
        prepare_geometry(state, mode, category, calibration);
        prepare_channels(engine, state, mode, category, calibration);
        publish_address_registers(engine, &state.registers, hardware);
    }
    state.slot_modes[slot.index()] = mode;
    Ok(())
}

pub fn update_source_configuration(states: &mut [PanelFormatterState; 2], configuration: SourceConfiguration) {
    for state in states {
        state.previous_source_mode = state.source_mode;
        state.source_mode = RecordedSourceMode(configuration.mode.raw());
        state.selector = RecordedSelector(configuration.selector.raw());
        state.flip = RecordedFlip(configuration.flip.raw());
    }
}
pub fn initial_live_configuration(live: bool, states: &mut [PanelFormatterState; 2], configuration: SourceConfiguration, calibration: &mut impl CalibrationRead, hardware: &mut impl Hardware) -> Result<(), Error> {
    update_source_configuration(states, configuration);
    if live {
        let state = &mut states[Engine::One.index()];
        let category = source_category(state);
        let slot = state.active_slot;
        let mode = state.slot_modes[slot.index()];
        set_slot_mode(Engine::One, state, slot, mode, category, calibration, hardware)?;
    }
    Ok(())
}

fn byte(calibration: &mut impl CalibrationRead, offset: CalibrationOffset) -> u32 {
    u32::from(calibration.read_byte(offset).0)
}
fn field(word: &mut u32, shift: u32, mask: u32, value: u32) {
    *word = (*word & !(mask << shift)) | ((value & mask) << shift);
}
fn calibration_word(calibration: &mut impl CalibrationRead, first: CalibrationOffset) -> u32 {
    let base = first.byte_offset();
    let low = byte(calibration, key(base + 3));
    let middle_low = byte(calibration, key(base + 2));
    let middle_high = byte(calibration, key(base + 1));
    let high = byte(calibration, first);
    low | (middle_low << 8) | (middle_high << 16) | (high << 24)
}

pub fn stage_configuration(engine: Engine, state: &mut PanelFormatterState, configuration: FormatterConfiguration, calibration: &mut impl CalibrationRead) {
    let source = match engine { Engine::Zero => BufferSlot::Two, Engine::One => BufferSlot::One };
    if configuration.source != source { loop { core::hint::spin_loop(); } }
    state.mode = configuration.mode;
    state.primary_key = configuration.primary_key;
    state.active_slot = configuration.source;
    state.secondary_key = configuration.secondary_key;
    state.formatter.pending = PendingUpdate(1);
    let live = engine == Engine::One && source == BufferSlot::One;
    let mode_three = state.mode.raw() == 3;
    let registers = &mut state.formatter.registers;
    registers.control = 0;
    if source == BufferSlot::One {
        for (offset, shift, mask) in [
            (0x494,0,7), (0x493,4,7), (0x492,7,1), (0x491,8,3),
            (0x490,10,1), (0x48f,12,15), (0x48e,16,15), (0x48d,20,7),
            (0x48c,23,1), (0x48b,24,1), (0x48a,25,1), (0x489,26,1),
            (0x488,27,1), (0x487,29,7),
        ] { field(&mut registers.control, shift, mask, byte(calibration, key(offset))); }
    }
    registers.input_control = 0;
    if source == BufferSlot::One { registers.input_control = pair(calibration, key(0x4b8)) & 0x1fff; }
    registers.dimensions = 0;
    if source == BufferSlot::One {
        registers.dimensions = pair(calibration, key(0x4be)) & 0xfff;
        field(&mut registers.dimensions, 16, 0x7fff, pair(calibration, key(0x4bc)));
    }
    state.pipeline_control = 0;
    if live {
        let increment = usize::from(mode_three);
        field(&mut state.pipeline_control, 24, 1, byte(calibration, key(0x62d + increment)));
        field(&mut state.pipeline_control, 16, 0x3f, byte(calibration, key(0x62f + increment)));
        field(&mut state.pipeline_control, 8, 0xff, byte(calibration, key(0x631 + increment)));
        field(&mut state.pipeline_control, 0, 0xff, byte(calibration, key(0x633 + increment)));
        registers.mode = calibration_word(calibration, key(if mode_three { 0x66e } else { 0x63e }));
    }
    if live {
        let base = if mode_three { 0x672 } else { 0x642 };
        for (index, word) in registers.parameters_38.iter_mut().enumerate() {
            field(word, 0, 0x7ff, pair(calibration, key(base + index * 4 + 2)));
            field(word, 16, 0x3ff, pair(calibration, key(base + index * 4)));
        }
    }
    let parameter_base = if live && !state.enabled.is_enabled() { 0xd08 } else if live && !mode_three { 0xb8c } else { 0xc4a };
    for (index, word) in registers.parameters_64.iter_mut().enumerate() {
        *word = calibration_word(calibration, key(parameter_base + index * 4));
    }
    field(&mut registers.channel_control, 0, 0xff, byte(calibration, key(parameter_base + 22)));
    field(&mut registers.channel_control, 8, 0xff, byte(calibration, key(parameter_base + 21)));
    field(&mut registers.channel_control, 16, 0xff, byte(calibration, key(parameter_base + 20)));
    let channel_base = if live && !state.enabled.is_enabled() { 0xd1f } else if live && !mode_three { 0xba3 } else { 0xc61 };
    for (channel, words) in registers.channel_parameters.iter_mut().enumerate() {
        if state.mode.raw() == 10 {
            *words = [0x40404040; 6];
        } else {
            for (index, word) in words.iter_mut().enumerate() {
                *word = calibration_word(calibration, key(channel_base + channel * 24 + index * 4));
            }
        }
    }
    registers.final_control = 0;
    if source == BufferSlot::One {
        let base = if mode_three { 0xdce } else { 0xdc7 };
        field(&mut registers.final_control, 0, 0x1fff, pair(calibration, key(base + 5)));
        field(&mut registers.final_control, 14, 1, byte(calibration, key(base + 4)));
        field(&mut registers.final_control, 15, 1, byte(calibration, key(base + 3)));
        field(&mut registers.final_control, 16, 0x1ff, pair(calibration, key(base + 1)));
        field(&mut registers.final_control, 28, 15, byte(calibration, key(base)));
    }
    registers.output_control = 0;
    if engine == Engine::One {
        state.plane.control = 0;
        if source == BufferSlot::One {
            for (offset, shift, mask) in [(0x4a6,0,1),(0x4a5,4,3),(0x4a4,8,1),(0x4a3,12,1)] {
                field(&mut state.plane.control, shift, mask, byte(calibration, key(offset)));
            }
        }
        if state.plane.control & 1 != 0 {
            state.plane.dimensions = 0;
            if source == BufferSlot::One {
                field(&mut state.plane.dimensions, 0, 0x1fff, pair(calibration, key(0x4a9)));
                field(&mut state.plane.dimensions, 16, 0x1fff, pair(calibration, key(0x4a7)));
            }
            state.plane.start = 0;
            state.plane.end = 0;
            if source == BufferSlot::One {
                let high = pair(calibration, key(0x4ab)) & 0x3fff;
                state.plane.start = (high << 16) | pair(calibration, key(0x4ad));
                let high = pair(calibration, key(0x4af)) & 0x3fff;
                state.plane.end = (high << 16) | pair(calibration, key(0x4b1));
            }
            state.plane.offset = 0;
            state.plane.limit = 0;
            if source == BufferSlot::One {
                field(&mut state.plane.offset, 0, 0x1fff, pair(calibration, key(0x4b3)));
                field(&mut state.plane.limit, 0, 0x1fff, pair(calibration, key(0x4b5)));
            }
        }
    }
}

fn apply_current_slot(engine: Engine, state: &mut PanelFormatterState, calibration: &mut impl CalibrationRead, hardware: &mut impl Hardware) {
    let category = source_category(state);
    let slot = state.active_slot;
    let mode = state.slot_modes[slot.index()];
    let _ = set_slot_mode(engine, state, slot, mode, category, calibration, hardware);
}
pub fn configure(engine: Engine, state: &mut PanelFormatterState, configuration: FormatterConfiguration, calibration: &mut impl CalibrationRead, hardware: &mut impl Hardware) -> Result<(), Error> {
    if state.configuration() == configuration { return Ok(()); }
    stage_configuration(engine, state, configuration, calibration);
    timing::formatter_flush(engine, &mut state.formatter, hardware);
    apply_current_slot(engine, state, calibration, hardware);
    Ok(())
}
pub fn set_enabled(live: bool, states: &mut [PanelFormatterState; 2], enabled: bool, calibration: &mut impl CalibrationRead, hardware: &mut impl Hardware) -> Result<(), Error> {
    let enabled = EnabledState(u32::from(enabled));
    if states[0].enabled == enabled && states[1].enabled == enabled { return Ok(()); }
    states[0].enabled = enabled;
    states[1].enabled = enabled;
    if live {
        let state = &mut states[1];
        let configuration = state.configuration();
        stage_configuration(Engine::One, state, configuration, calibration);
        timing::formatter_flush(Engine::One, &mut state.formatter, hardware);
        apply_current_slot(Engine::One, state, calibration, hardware);
    }
    Ok(())
}

#[cfg(all(target_arch = "arm", feature = "panel"))]
pub(crate) mod abi {
    use super::*;
    use core::ptr::{read_volatile, write_volatile};

    extern "C" {
        fn xm1_power_interrupt_lock() -> u32;
        fn xm1_power_interrupt_unlock(token: u32);
        fn xm1_power_task_delay(ticks: u32) -> i32;
    }
    struct NativeCalibration;
    impl CalibrationRead for NativeCalibration {
        fn read_byte(&mut self, offset: CalibrationOffset) -> calibration_liveview::CalibrationByte {
            unsafe { crate::panel::configuration_byte(offset) }
        }
    }
    struct Device;
    impl Hardware for Device {
        fn read(&mut self, register: Register) -> u32 { unsafe { word(register_address(register)) } }
        fn write(&mut self, register: Register, value: u32) { unsafe { put(register_address(register), value); } }
        fn enter_critical(&mut self) -> u32 { unsafe { xm1_power_interrupt_lock() } }
        fn leave_critical(&mut self, token: u32) { unsafe { xm1_power_interrupt_unlock(token); } }
        fn yield_zero(&mut self) { unsafe { let _ = xm1_power_task_delay(0); } }
    }
    fn register_address(register: Register) -> usize {
        let bank = match register.bank { Bank::Scanout => 0xfff6b000, Bank::Timing => 0xfff6a000, Bank::Formatter => 0xfff69000 };
        bank + register.engine.index() * 0x3000 + usize::from(register.offset)
    }
    fn base(engine: Engine) -> usize { 0x917c10 + engine.index() * 0x128 }
    unsafe fn word(address: usize) -> u32 { read_volatile(address as *const u32) }
    unsafe fn put(address: usize, value: u32) { write_volatile(address as *mut u32, value); }
    unsafe fn put_byte(address: usize, value: u8) { write_volatile(address as *mut u8, value); }
    unsafe fn masked_half(address: usize, mask: u16, value: u16) {
        let pointer = address as *mut u16;
        write_volatile(pointer, (read_volatile(pointer) & !mask) | (value & mask));
    }
    unsafe fn packet(address: usize, source: BufferSlot) -> FormatterConfiguration {
        FormatterConfiguration {
            mode: FormatterMode(word(address)),
            primary_key: PrimaryConfigurationKey(word(address + 4)),
            source,
            secondary_key: SecondaryConfigurationKey(word(address + 12)),
        }
    }
    unsafe fn metadata(engine: Engine, source: BufferSlot) -> PanelFormatterState {
        let address = base(engine);
        let mut state = PanelFormatterState::zero();
        state.mode = FormatterMode(word(address));
        state.active_slot = source;
        state.enabled = EnabledState(word(address + 0x10));
        state.source_mode = RecordedSourceMode(word(address + 0x14));
        state.previous_source_mode = RecordedSourceMode(word(address + 0x18));
        state.selector = RecordedSelector(word(address + 0x1c));
        state.flip = RecordedFlip(word(address + 0x20));
        state
    }
    unsafe fn load_address_registers(engine: Engine) -> AddressRegisters {
        let address = base(engine);
        let mut registers = AddressRegisters {
            geometry: SourceGeometry { extent: word(address + 0x5c), origin: word(address + 0x60), limit: word(address + 0x64) },
            ..AddressRegisters::default()
        };
        for channel in 0..3 {
            registers.channels[channel] = ChannelTransform { coefficients: word(address + 0x84 + channel * 8), offset: word(address + 0x88 + channel * 8) };
            registers.alternate_channels[channel] = ChannelTransform { coefficients: word(address + 0x104 + channel * 8), offset: word(address + 0x108 + channel * 8) };
        }
        registers
    }
    unsafe fn load_prepared_inputs(engine: Engine, source: BufferSlot) -> AddressRegisters {
        let address = base(engine);
        let mut registers = AddressRegisters::default();
        for channel in 0..3 {
            registers.alternate_channels[channel] = ChannelTransform { coefficients: word(address + 0x104 + channel * 8), offset: word(address + 0x108 + channel * 8) };
            if engine == Engine::One && source == BufferSlot::One {
                registers.channels[channel] = ChannelTransform { coefficients: word(address + 0x84 + channel * 8), offset: word(address + 0x88 + channel * 8) };
            }
        }
        registers
    }
    unsafe fn store_prepared_address(engine: Engine, state: &PanelFormatterState) {
        let address = base(engine);
        let registers = &state.registers;
        put(address + 0x5c, registers.geometry.extent);
        put(address + 0x60, registers.geometry.origin);
        put(address + 0x64, registers.geometry.limit);
        for channel in 0..3 {
            let alternate = registers.alternate_channels[channel];
            let at = address + 0x104 + channel * 8;
            masked_half(at + 2, 0x1fff, (alternate.coefficients >> 16) as u16);
            masked_half(at, 0x1fff, alternate.coefficients as u16);
            masked_half(at + 4, 0x1fff, alternate.offset as u16);
            if engine == Engine::One && state.active_slot == BufferSlot::One {
                let transform = registers.channels[channel];
                let at = address + 0x84 + channel * 8;
                masked_half(at + 2, 0x1fff, (transform.coefficients >> 16) as u16);
                masked_half(at, 0x1fff, transform.coefficients as u16);
                masked_half(at + 4, 0x1fff, transform.offset as u16);
            }
            if state.mode.raw() == 10 {
                let transform = registers.channels[channel];
                let alternate = registers.alternate_channels[channel];
                put(address + 0x84 + channel * 8, transform.coefficients);
                put(address + 0x88 + channel * 8, transform.offset);
                put(address + 0x104 + channel * 8, alternate.coefficients);
                put(address + 0x108 + channel * 8, alternate.offset);
            }
        }
    }
    unsafe fn apply_slot(engine: Engine, slot: BufferSlot, mode: BufferMode, category: SourceCategory) {
        let address = base(engine);
        if matches!(slot, BufferSlot::One | BufferSlot::Two) && word(address + 8) == slot.raw() {
            let mut state = metadata(engine, slot);
            state.registers = load_prepared_inputs(engine, slot);
            prepare_geometry(&mut state, mode, category, &mut NativeCalibration);
            prepare_channels(engine, &mut state, mode, category, &mut NativeCalibration);
            store_prepared_address(engine, &state);
            publish_address_registers(engine, &load_address_registers(engine), &mut Device);
        }
        put(address + 0x24 + slot.index() * 4, mode.raw());
    }
    unsafe fn apply_current(engine: Engine) -> i32 {
        let address = base(engine);
        let state = metadata(engine, BufferSlot::Zero);
        let category = source_category(&state);
        let raw_slot = word(address + 8);
        let raw_mode = word(address.wrapping_add(0x24).wrapping_add((raw_slot as usize).wrapping_mul(4)));
        let Some(slot) = BufferSlot::parse(raw_slot) else { return -30; };
        let Some(mode) = BufferMode::parse(raw_mode) else { return -30; };
        apply_slot(engine, slot, mode, category);
        0
    }
    unsafe fn load_stage(engine: Engine) -> PanelFormatterState {
        let address = base(engine);
        let mut state = PanelFormatterState::zero();
        state.enabled = EnabledState(word(address + 0x10));
        if engine == Engine::One {
            for index in 0..5 { state.formatter.registers.parameters_38[index] = word(address + 0x70 + index * 4); }
        }
        state.formatter.registers.channel_control = word(address + 0xb0);
        state
    }
    unsafe fn store_stage(engine: Engine, state: &PanelFormatterState) {
        let address = base(engine);
        put(address + 0x34, 1);
        put(address, state.mode.raw());
        put(address + 4, state.primary_key.0);
        put(address + 8, state.active_slot.raw());
        put(address + 12, state.secondary_key.0);
        let registers = &state.formatter.registers;
        put(address + 0x38, registers.control);
        put(address + 0x3c, registers.input_control);
        put(address + 0x58, registers.dimensions);
        put(address + 0x68, state.pipeline_control);
        if engine == Engine::One {
            put(address + 0x6c, registers.mode);
            for (index, &value) in registers.parameters_38.iter().enumerate() {
                masked_half(address + 0x70 + index * 4, 0x7ff, value as u16);
                masked_half(address + 0x72 + index * 4, 0x3ff, (value >> 16) as u16);
            }
        }
        for (index, &value) in registers.parameters_64.iter().enumerate() { put(address + 0x9c + index * 4, value); }
        put_byte(address + 0xb0, registers.channel_control as u8);
        put_byte(address + 0xb1, (registers.channel_control >> 8) as u8);
        put_byte(address + 0xb2, (registers.channel_control >> 16) as u8);
        for (channel, words) in registers.channel_parameters.iter().enumerate() {
            for (index, &value) in words.iter().enumerate() { put(address + 0xb4 + channel * 24 + index * 4, value); }
        }
        put(address + 0xfc, registers.final_control);
        put(address + 0x100, registers.output_control);
        if engine == Engine::One {
            put(address + 0x40, state.plane.control);
            if state.plane.control & 1 != 0 {
                for (offset, value) in [(0x44,state.plane.dimensions),(0x48,state.plane.start),(0x4c,state.plane.end),(0x50,state.plane.offset),(0x54,state.plane.limit)] { put(address + offset, value); }
            }
        }
    }
    unsafe fn flush(engine: Engine) {
        let address = base(engine);
        if word(address + 0x34) == 0 { return; }
        put(address + 0x34, 0);
        let mut state = FormatterState::zero();
        state.pending = PendingUpdate(1);
        let registers = &mut state.registers;
        registers.control = word(address + 0x38);
        registers.input_control = word(address + 0x3c);
        registers.dimensions = word(address + 0x58);
        registers.output_control = word(address + 0x100);
        registers.mode = word(address + 0x6c);
        for index in 0..5 {
            registers.parameters_38[index] = word(address + 0x70 + index * 4);
            registers.parameters_64[index] = word(address + 0x9c + index * 4);
        }
        registers.channel_control = word(address + 0xb0);
        for (channel, words) in registers.channel_parameters.iter_mut().enumerate() {
            for (index, value) in words.iter_mut().enumerate() { *value = word(address + 0xb4 + channel * 24 + index * 4); }
        }
        registers.final_control = word(address + 0xfc);
        timing::formatter_flush(engine, &mut state, &mut Device);
    }
    unsafe fn stage_and_publish(engine: Engine, configuration: FormatterConfiguration) {
        let mut state = load_stage(engine);
        stage_configuration(engine, &mut state, configuration, &mut NativeCalibration);
        store_stage(engine, &state);
        flush(engine);
        let _ = apply_current(engine);
    }
    /// Pinned 0x6ce1dc.
    pub(crate) unsafe fn enable_engine(engine: Engine, value: u32) -> i32 {
        if value > 1 { return -30; }
        let address = base(Engine::Zero);
        if word(address + 0x138) == value && word(address + 0x10) == value { return 0; }
        put(address + 0x138, value);
        put(address + 0x10, value);
        if engine == Engine::One {
            let configuration = FormatterConfiguration {
                mode: FormatterMode(word(address + 0x128)),
                primary_key: PrimaryConfigurationKey(word(address + 0x12c)),
                source: BufferSlot::parse(word(address + 0x130)).unwrap_or(BufferSlot::Zero),
                secondary_key: SecondaryConfigurationKey(word(address + 0x134)),
            };
            let mut state = load_stage(engine);
            state.enabled = EnabledState(value);
            state.source_mode = RecordedSourceMode(word(address + 0x13c));
            state.selector = RecordedSelector(word(address + 0x144));
            state.flip = RecordedFlip(word(address + 0x148));
            stage_configuration(engine, &mut state, configuration, &mut NativeCalibration);
            store_stage(engine, &state);
            flush(engine);
            let category = source_category(&state);
            if let Some(mode) = BufferMode::parse(word(address + 0x14c + configuration.source.index() * 4)) {
                apply_slot(engine, configuration.source, mode, category);
            }
        }
        0
    }
    pub(crate) unsafe fn initialize(engine: Engine, configuration: *const u32) -> i32 {
        let address = configuration as usize;
        let source = match engine { Engine::Zero => BufferSlot::Two, Engine::One => BufferSlot::One };
        if word(address + 8) != source.raw() { loop { core::hint::spin_loop(); } }
        stage_and_publish(engine, packet(address, source));
        0
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_initial_live_configuration(live: u32, mode: u32, selector: u32, flip: u32) -> i32 {
        let Some(mode) = SourceMode::parse(mode) else { return -30; };
        let Some(selector) = SourceSelector::parse(selector) else { return -30; };
        let Some(flip) = Flip::parse(flip) else { return -30; };
        for engine in [Engine::One, Engine::Zero] {
            let address = base(engine);
            put(address + 0x18, word(address + 0x14));
            put(address + 0x14, mode.raw());
            put(address + 0x1c, selector.raw());
            put(address + 0x20, flip.raw());
        }
        if live == 1 { apply_current(Engine::One) } else { 0 }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_formatter_configure(engine: u32, configuration: *const u32) -> i32 {
        let Some(engine) = Engine::parse(engine) else { return -30; };
        let address = base(engine);
        let packet_address = configuration as usize;
        if word(address) == word(packet_address)
            && word(address + 4) == word(packet_address + 4)
            && word(address + 8) == word(packet_address + 8)
            && word(address + 12) == word(packet_address + 12) { return 0; }
        let source = match engine { Engine::Zero => BufferSlot::Two, Engine::One => BufferSlot::One };
        if word(packet_address + 8) != source.raw() { loop { core::hint::spin_loop(); } }
        stage_and_publish(engine, packet(packet_address, source));
        0
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_formatter_set_enabled(live: u32, enabled: u32) -> i32 {
        if enabled > 1 { return -30; }
        if word(base(Engine::One) + 0x10) == enabled && word(base(Engine::Zero) + 0x10) == enabled { return 0; }
        put(base(Engine::One) + 0x10, enabled);
        put(base(Engine::Zero) + 0x10, enabled);
        if live == 1 {
            let address = base(Engine::One);
            if word(address + 8) != 1 { loop { core::hint::spin_loop(); } }
            stage_and_publish(Engine::One, packet(address, BufferSlot::One));
        }
        0
    }
}
#[cfg(all(target_arch = "arm", feature = "panel"))]
pub use abi::{xm1_scanout_panel_formatter_configure, xm1_scanout_panel_formatter_set_enabled, xm1_scanout_panel_initial_live_configuration};

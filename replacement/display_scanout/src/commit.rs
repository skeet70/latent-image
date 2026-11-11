use crate::{Bank, Engine, Hardware, Register};

macro_rules! word_type {
    ($($name:ident),+ $(,)?) => { $(
        #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
        pub struct $name(pub u32);
    )+ };
}
word_type!(RegisterWord, CallbackId, ConfigurationSelector, CaptureMode, ConfigurationAuxiliary,
    CommitPending, RegistrationId, FloatBits, ColorWord, CoordinateOffset);
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Opacity(pub u8);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RuntimeState {
    pub selector: ConfigurationSelector,
    pub mode: CaptureMode,
    pub auxiliary: ConfigurationAuxiliary,
    pub pending: CommitPending,
    pub registration: RegistrationId,
    pub completion_callbacks: [Option<CallbackId>; 6],
    pub secondary_callbacks: [Option<CallbackId>; 6],
    pub capture_count: u8,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlaneRegisters {
    pub origin: RegisterWord,
    pub layout: RegisterWord,
    pub stride: RegisterWord,
    pub address: RegisterWord,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RectangleRegisters {
    pub horizontal: RegisterWord,
    pub vertical: RegisterWord,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SavedRegisters {
    pub control: RegisterWord,
    pub geometry_control: RegisterWord,
    pub format: RegisterWord,
    pub timing: RegisterWord,
    pub extent: RegisterWord,
    pub sync: RegisterWord,
    pub engine_one_control: RegisterWord,
    pub window_start: RegisterWord,
    pub window_end: RegisterWord,
    pub output_start: RegisterWord,
    pub output_end: RegisterWord,
    pub horizontal_scale: RegisterWord,
    pub vertical_scale: RegisterWord,
    pub origin: RegisterWord,
    pub crop_start: RegisterWord,
    pub crop_end: RegisterWord,
    pub crop_step: RegisterWord,
    pub auxiliary_start: RegisterWord,
    pub auxiliary_end: RegisterWord,
    pub auxiliary_step: RegisterWord,
    pub output_control: RegisterWord,
    pub plane_control: RegisterWord,
    pub auxiliary_control: RegisterWord,
    pub primary_planes: [PlaneRegisters; 4],
    pub primary_control: RegisterWord,
    pub secondary_control: RegisterWord,
    pub overlay_control: RegisterWord,
    pub secondary_planes: [PlaneRegisters; 4],
    pub rectangle_mask: RegisterWord,
    pub two_rectangles: [RectangleRegisters; 2],
    pub ten_rectangles: [RectangleRegisters; 10],
    pub default_levels: [RegisterWord; 2],
    pub level_selectors: RegisterWord,
    pub default_colors: [RegisterWord; 4],
    pub color_selectors: RegisterWord,
    pub default_opacities: RegisterWord,
    pub opacity_selectors: RegisterWord,
    pub two_extents: [RegisterWord; 2],
    pub overlay_start: RegisterWord,
    pub overlay_end: RegisterWord,
    pub overlay_step: RegisterWord,
    pub overlay_format: RegisterWord,
    pub overlay_address: RegisterWord,
    pub overlay_stride: RegisterWord,
    pub overlay_enable: RegisterWord,
    pub interrupt_control: RegisterWord,
    pub interrupt_start: RegisterWord,
    pub interrupt_end: RegisterWord,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RectangleExtent { pub width: i16, pub height: i16 }
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RectangleInput {
    pub coordinates: [FloatBits; 4],
    pub extent: RectangleExtent,
    pub level: FloatBits,
    pub color: ColorWord,
    pub opacity: Opacity,
    pub reverse: u8,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PreparationInput {
    pub origin: RegisterWord,
    pub primary_origins: [RegisterWord; 4],
    pub secondary_origins: [RegisterWord; 4],
    pub vertical_offset: CoordinateOffset,
    pub horizontal_offset: CoordinateOffset,
    pub geometry_control: RegisterWord,
    pub rectangle_mask: RegisterWord,
    pub two_rectangles: [RectangleRegisters; 2],
    pub two_reverse: [u8; 2],
    pub ten_rectangles: [RectangleRegisters; 10],
    pub ten_reverse: [u8; 10],
    pub two_records: [RectangleInput; 2],
    pub ten_records: [RectangleInput; 10],
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EngineState {
    pub runtime: RuntimeState,
    pub saved: SavedRegisters,
    pub input: PreparationInput,
}
impl EngineState { pub fn zero() -> Self { Self::default() } }
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CommitOptions { pub alternate_masks: bool, pub reverse_vertical: bool }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionList { First, Second }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletionSlot { pub list: CompletionList, index: usize }
impl CompletionSlot {
    pub fn new(list: CompletionList, index: usize) -> Option<Self> {
        if index < 6 { Some(Self { list, index }) } else { None }
    }
    pub const fn index(self) -> usize { self.index }
}
pub trait CompletionCallbacks {
    fn prepared(&mut self, _engine: Engine, _state: &EngineState) {}
    fn call(&mut self, callback: CallbackId, slot: CompletionSlot, engine: Engine, state: &mut EngineState);
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureConfiguration {
    pub mode: CaptureMode,
    pub selector: ConfigurationSelector,
    pub auxiliary: ConfigurationAuxiliary,
}
impl RuntimeState { pub fn zero() -> Self { Self::default() } }
impl SavedRegisters { pub fn zero() -> Self { Self::default() } }
impl PreparationInput { pub fn zero() -> Self { Self::default() } }
pub trait Registration {
    /// Engine zero uses persistent scheduler order 1, phase 0; engine one registers a direct callback address on IRQ 14.
    fn register_commit(&mut self, engine: Engine, state: &mut EngineState) -> i32;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RectangleGroup { Ten, Two }
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DefaultsCollection {
    pub levels: [FloatBits; 4],
    pub colors: [ColorWord; 4],
    pub opacities: [Opacity; 2],
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackedDefaults {
    pub levels: [RegisterWord; 4],
    pub colors: [ColorWord; 4],
    pub opacities: [Opacity; 2],
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DefaultSelector(pub u8);
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RectanglePacket {
    pub coordinates: [u16; 4],
    pub extent: RectangleExtent,
    pub level: DefaultSelector,
    pub color: DefaultSelector,
    pub opacity: DefaultSelector,
    pub reverse: u8,
}

pub fn float_to_signed(value: FloatBits) -> i32 {
    let bits = value.0;
    let exponent = (bits >> 23) & 255;
    if exponent < 127 { return 0; }
    if exponent >= 158 {
        if exponent == 255 && bits & 0x7fffff != 0 { return 0; }
        return if bits >> 31 != 0 { i32::MIN } else { i32::MAX };
    }
    let magnitude = ((bits << 8) | 0x80000000) >> (158 - exponent);
    if bits >> 31 != 0 { (magnitude as i32).wrapping_neg() } else { magnitude as i32 }
}
pub fn float_to_unsigned(value: FloatBits) -> u32 {
    let bits = value.0;
    let exponent = (bits >> 23) & 255;
    if bits >> 31 != 0 || exponent < 127 { return 0; }
    if exponent > 158 { return if exponent == 255 && bits & 0x7fffff != 0 { 0 } else { u32::MAX }; }
    ((bits << 8) | 0x80000000) >> (158 - exponent)
}
fn float_equal(a: FloatBits, b: FloatBits) -> bool {
    let a_abs = a.0 & 0x7fffffff;
    let b_abs = b.0 & 0x7fffffff;
    a_abs <= 0x7f800000 && b_abs <= 0x7f800000 && (a == b || (a_abs == 0 && b_abs == 0))
}
/// Returns -30 on capacity exhaustion, preserving values collected before the failure.
pub fn collect_defaults(state: &EngineState, alternate_masks: bool, packet: &mut DefaultsCollection) -> i32 {
    let mask = if alternate_masks { state.input.rectangle_mask.0 } else { state.saved.rectangle_mask.0 };
    let (mut levels, mut colors, mut opacities) = (0, 0, 0);
    for (records, shift) in [(&state.input.ten_records[..], 0), (&state.input.two_records[..], 16)] {
        for (item, record) in records.iter().enumerate() {
            if mask & (1 << (shift + item)) == 0 { continue; }
            if !packet.levels[..levels].iter().any(|&value| float_equal(value, record.level)) {
                if levels == packet.levels.len() { return -30; }
                packet.levels[levels] = record.level;
                levels += 1;
            }
            if !packet.colors[..colors].contains(&record.color) {
                if colors == packet.colors.len() { return -30; }
                packet.colors[colors] = record.color;
                colors += 1;
            }
            if !packet.opacities[..opacities].contains(&record.opacity) {
                if opacities == packet.opacities.len() { return -30; }
                packet.opacities[opacities] = record.opacity;
                opacities += 1;
            }
        }
    }
    0
}
fn replace_half(word: &mut RegisterWord, high: bool, mask: u16, value: u16) {
    let shift = if high { 16 } else { 0 };
    let mask = (mask as u32) << shift;
    word.0 = (word.0 & !mask) | (((value as u32) << shift) & mask);
}
fn half(word: RegisterWord, high: bool) -> u16 { (word.0 >> if high { 16 } else { 0 }) as u16 }
pub fn pack_defaults(state: &mut EngineState, packet: &PackedDefaults) {
    state.saved.default_opacities.0 = (state.saved.default_opacities.0 & 0xffff0000)
        | packet.opacities[0].0.min(128) as u32 | ((packet.opacities[1].0.min(128) as u32) << 8);
    for item in 0..4 {
        replace_half(&mut state.saved.default_levels[item / 2], item % 2 != 0, 0x7ff, packet.levels[item].0 as u16);
        state.saved.default_colors[item] = RegisterWord(packet.colors[item].0);
    }
}
pub fn pack_rectangle(state: &mut EngineState, group: RectangleGroup, item: usize, packet: &RectanglePacket) -> i32 {
    let (rectangle, reverse, bit, selector) = match group {
        RectangleGroup::Ten if item < 10 => (&mut state.input.ten_rectangles[item], &mut state.input.ten_reverse[item], item, item * 2),
        RectangleGroup::Two if item < 2 => (&mut state.input.two_rectangles[item], &mut state.input.two_reverse[item], 16 + item, 24 + item * 2),
        _ => return -30,
    };
    replace_half(&mut rectangle.horizontal, false, 0xfff, packet.coordinates[1]);
    replace_half(&mut rectangle.horizontal, true, 0xfff, packet.coordinates[0]);
    replace_half(&mut rectangle.vertical, false, 0xfff, packet.coordinates[3]);
    replace_half(&mut rectangle.vertical, true, 0xfff, packet.coordinates[2]);
    *reverse = packet.reverse;
    state.saved.opacity_selectors.0 = (state.saved.opacity_selectors.0 & !(1 << bit)) | ((packet.opacity.0 as u32) << bit);
    state.saved.color_selectors.0 = (state.saved.color_selectors.0 & !(3 << selector)) | ((packet.color.0 as u32) << selector);
    state.saved.level_selectors.0 = (state.saved.level_selectors.0 & !(3 << selector)) | ((packet.level.0 as u32) << selector);
    if group == RectangleGroup::Two {
        replace_half(&mut state.saved.two_extents[item], true, 0x7ff, packet.extent.width as u16);
        replace_half(&mut state.saved.two_extents[item], false, 0x7ff, packet.extent.height as u16);
    }
    0
}
fn prepare(state: &mut EngineState, options: CommitOptions) {
    let mut defaults = DefaultsCollection::default();
    let _ = collect_defaults(state, options.alternate_masks, &mut defaults);
    pack_defaults(state, &PackedDefaults {
        levels: defaults.levels.map(|value| RegisterWord(float_to_unsigned(value))),
        colors: defaults.colors,
        opacities: defaults.opacities,
    });
    let scale = if state.runtime.mode == CaptureMode(2) && state.runtime.auxiliary != ConfigurationAuxiliary(0) { 1000 } else { 10 };
    for group in [RectangleGroup::Ten, RectangleGroup::Two] {
        let count = if group == RectangleGroup::Ten { 10 } else { 2 };
        for item in 0..count {
            let record = if group == RectangleGroup::Ten { state.input.ten_records[item] } else { state.input.two_records[item] };
            let packet = RectanglePacket {
                coordinates: record.coordinates.map(|value| float_to_signed(value) as u16),
                extent: RectangleExtent {
                    width: ((record.extent.width as i32 * scale) / 10) as i16,
                    height: ((record.extent.height as i32 * scale) / 10) as i16,
                },
                level: DefaultSelector(defaults.levels.iter().position(|&value| float_equal(record.level, value)).unwrap_or(4) as u8),
                color: DefaultSelector(defaults.colors.iter().position(|&value| value == record.color).unwrap_or(4) as u8),
                opacity: DefaultSelector(defaults.opacities.iter().position(|&value| value == record.opacity).unwrap_or(2) as u8),
                reverse: record.reverse,
            };
            let _ = pack_rectangle(state, group, item, &packet);
        }
    }
    state.saved.geometry_control = state.input.geometry_control;
    let limit = (half(state.saved.extent, true) & 0xfff) + 1;
    for (inputs, saved, reverses) in [
        (&state.input.ten_rectangles[..], &mut state.saved.ten_rectangles[..], &state.input.ten_reverse[..]),
        (&state.input.two_rectangles[..], &mut state.saved.two_rectangles[..], &state.input.two_reverse[..]),
    ] {
        for ((input, saved), &reverse) in inputs.iter().zip(saved).zip(reverses) {
            replace_half(&mut saved.horizontal, false, 0xfff, half(input.horizontal, false));
            replace_half(&mut saved.horizontal, true, 0xfff, half(input.horizontal, true));
            replace_half(&mut saved.vertical, false, 0xfff, half(input.vertical, false));
            replace_half(&mut saved.vertical, true, 0xfff, half(input.vertical, true));
            if options.reverse_vertical && reverse != 0 {
                replace_half(&mut saved.horizontal, true, 0xfff, limit.wrapping_sub(half(input.vertical, true) & 0xfff));
                replace_half(&mut saved.vertical, true, 0xfff, limit.wrapping_sub(half(input.horizontal, true) & 0xfff));
            }
        }
    }
    if options.reverse_vertical { state.saved.geometry_control.0 ^= 1; }
    state.saved.origin = state.input.origin;
    let flags = state.saved.geometry_control.0 as u8;
    if flags & 1 != 0 {
        let value = half(state.saved.origin, true).wrapping_add(state.input.vertical_offset.0 as u16);
        replace_half(&mut state.saved.origin, true, 0x3fff, value);
    }
    if flags & 2 != 0 {
        let value = half(state.saved.origin, false).wrapping_add(state.input.horizontal_offset.0 as u16);
        replace_half(&mut state.saved.origin, false, 0x3fff, value);
    }
    for (group, planes, origins) in [
        (0, &mut state.saved.primary_planes, &state.input.primary_origins),
        (1, &mut state.saved.secondary_planes, &state.input.secondary_origins),
    ] {
        let shift = if group == 0 { (state.saved.format.0 >> 4) & 3 } else { (state.saved.format.0 >> 8) & 3 };
        for (plane, &origin) in planes.iter_mut().zip(origins) {
            plane.origin = origin;
            if state.input.vertical_offset.0 != 0 || state.input.horizontal_offset.0 != 0 {
                if flags & (1 << (2 + group * 2)) != 0 {
                    let value = half(plane.origin, true).wrapping_add(state.input.vertical_offset.0 as u16);
                    replace_half(&mut plane.origin, true, 0x3fff, value);
                }
                if flags & (1 << (3 + group * 2)) != 0 {
                    let value = half(plane.origin, false).wrapping_add((state.input.horizontal_offset.0 >> shift) as u16);
                    replace_half(&mut plane.origin, false, 0x3fff, value);
                }
            }
        }
    }
}
fn register(engine: Engine, offset: u16) -> Register { Register { engine, bank: Bank::Scanout, offset } }
fn visit_registers(engine: Engine, capture: bool, saved: &mut SavedRegisters, mut visit: impl FnMut(&mut RegisterWord, u16)) {
    macro_rules! fields { ($($field:ident => $offset:expr),+ $(,)?) => { $(visit(&mut saved.$field, $offset);)+ }; }
    fields!(control=>0, geometry_control=>4, format=>8, timing=>12, extent=>16, sync=>20);
    if engine == Engine::One { visit(&mut saved.engine_one_control, 0x18); }
    fields!(window_start=>0x1c, window_end=>0x20, output_start=>0x38, output_end=>0x3c,
        horizontal_scale=>0x88, vertical_scale=>0x8c, origin=>0x24, crop_start=>0x28,
        crop_end=>0x2c, crop_step=>0x30, auxiliary_start=>0xd4, auxiliary_end=>0xd8,
        auxiliary_step=>0xdc, output_control=>0x34, plane_control=>0x40, auxiliary_control=>0xe0,
        primary_control=>0x84, secondary_control=>0x90, overlay_control=>0xe4);
    for item in 0..4 {
        for (plane, base) in [(&mut saved.primary_planes[item], 0x44), (&mut saved.secondary_planes[item], 0x94)] {
            let offset = base + item as u16 * 4;
            visit(&mut plane.origin, offset);
            visit(&mut plane.layout, offset + 0x10);
            visit(&mut plane.stride, offset + 0x20);
            visit(&mut plane.address, offset + 0x30);
        }
    }
    for item in 0..2 {
        visit(&mut saved.two_rectangles[item].horizontal, 0x10c + item as u16 * 8);
        visit(&mut saved.two_rectangles[item].vertical, 0x110 + item as u16 * 8);
        visit(&mut saved.two_extents[item], 0x194 + item as u16 * 4);
    }
    for item in 0..10 {
        visit(&mut saved.ten_rectangles[item].horizontal, 0x11c + item as u16 * 8);
        visit(&mut saved.ten_rectangles[item].vertical, 0x120 + item as u16 * 8);
    }
    visit(&mut saved.default_levels[0], 0x16c);
    visit(&mut saved.default_levels[1], 0x170);
    visit(&mut saved.level_selectors, 0x174);
    for (item, word) in saved.default_colors.iter_mut().enumerate() { visit(word, 0x178 + item as u16 * 4); }
    fields!(color_selectors=>0x188, default_opacities=>0x18c, opacity_selectors=>0x190);
    if !capture { visit(&mut saved.rectangle_mask, 0x108); }
    fields!(overlay_start=>0xe8, overlay_end=>0xec, overlay_step=>0xf0, overlay_format=>0xf4,
        overlay_address=>0xf8, overlay_stride=>0xfc, overlay_enable=>0x100,
        interrupt_control=>0x1bc, interrupt_start=>0x9d0, interrupt_end=>0x9d4);
}
pub fn commit(engine: Engine, state: &mut EngineState, options: CommitOptions, hardware: &mut impl Hardware, callbacks: &mut impl CompletionCallbacks) {
    if state.runtime.pending == CommitPending(1) {
        state.runtime.pending = CommitPending(0);
        prepare(state, options);
        callbacks.prepared(engine, state);
        visit_registers(engine, false, &mut state.saved, |word, offset| hardware.write(register(engine, offset), word.0));
    }
    for secondary in [false, true] {
        for item in 0..6 {
            let slot = if secondary { &mut state.runtime.secondary_callbacks[item] } else { &mut state.runtime.completion_callbacks[item] };
            if let Some(callback) = slot.take() {
                let list = if secondary { CompletionList::Second } else { CompletionList::First };
                callbacks.call(callback, CompletionSlot { list, index: item }, engine, state);
            }
        }
    }
}
pub fn commit_zero(state: &mut EngineState, options: CommitOptions, hardware: &mut impl Hardware, callbacks: &mut impl CompletionCallbacks) { commit(Engine::Zero, state, options, hardware, callbacks); }
pub fn commit_one(state: &mut EngineState, options: CommitOptions, hardware: &mut impl Hardware, callbacks: &mut impl CompletionCallbacks) { commit(Engine::One, state, options, hardware, callbacks); }
pub fn capture(engine: Engine, state: &mut EngineState, configuration: CaptureConfiguration, hardware: &mut impl Hardware, registration: &mut impl Registration) -> i32 {
    let first = state.runtime.capture_count == 0;
    if !first && state.runtime.mode == configuration.mode && state.runtime.selector == configuration.selector && state.runtime.auxiliary == configuration.auxiliary { return 0; }
    if first { state.runtime = RuntimeState::default(); }
    state.runtime.mode = if first && configuration.mode == CaptureMode(0) { CaptureMode(if engine == Engine::One { 1 } else { 3 }) } else { configuration.mode };
    state.runtime.selector = configuration.selector;
    state.runtime.auxiliary = configuration.auxiliary;
    let mut result = 0;
    if first {
        crate::timing::reset(engine, hardware);
        visit_registers(engine, true, &mut state.saved, |word, offset| word.0 = hardware.read(register(engine, offset)));
        state.saved.control.0 |= 6;
        if state.runtime.registration == RegistrationId(0) { result = registration.register_commit(engine, state); }
    }
    state.runtime.capture_count = state.runtime.capture_count.wrapping_add(1);
    result
}

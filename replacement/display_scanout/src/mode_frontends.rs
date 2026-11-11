use crate::{Engine, Error};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeWord(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawGroup(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Window { First, Second, Third }
impl Window {
    pub fn parse(value: u32) -> Option<Self> { match value { 0 => Some(Self::First), 1 => Some(Self::Second), 2 => Some(Self::Third), _ => None } }
    #[cfg(all(target_arch = "arm", feature = "panel"))]
    fn index(self) -> usize { match self { Self::First => 0, Self::Second => 1, Self::Third => 2 } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    ForcedEngine, SelectedMode, Initialized(Engine), Alternate, WindowOrigin(Engine, Window),
    WindowSize(Engine, Window), Backing(Engine), Primary(Engine), AlternatePrimary(Engine),
    Range(Engine, RawGroup, bool), Colour(Engine, usize), ScaleHistory(usize),
    ScaleDimensions(Engine), ScaleRatio(Engine), ScalePhase(Engine), ScaleOrigin(Engine),
    ScalePadding(Engine), AlternateOrigin(Engine), AlternateFormat(Engine), ScaleOpacity(Engine),
    ColourPacketNibbles(Engine), ColourPacketThird(Engine), ColourPacketFlag(Engine), ColourPacketValue(Engine),
}
pub trait Storage {
    fn read(&self, field: Field) -> ModeWord;
    fn write(&mut self, field: Field, value: ModeWord);
}
pub fn selected_engine(state: &impl Storage) -> Engine {
    if state.read(Field::ForcedEngine).0 == 0 && state.read(Field::SelectedMode).0 == 1 { Engine::One } else { Engine::Zero }
}
fn replace(state: &mut impl Storage, field: Field, mask: u32, value: u32) {
    let old = state.read(field).0;
    state.write(field, ModeWord((old & !mask) | (value & mask)));
}
pub fn window(state: &mut impl Storage, selector: u32, packet: [u16; 4]) -> Result<(), Error> {
    let engine = selected_engine(state);
    let window = Window::parse(selector).ok_or(Error::InvalidArgument)?;
    replace(state, Field::WindowOrigin(engine, window), 0x0fff_0000, u32::from(packet[0]) << 16);
    replace(state, Field::WindowOrigin(engine, window), 0x0fff, u32::from(packet[1]));
    replace(state, Field::WindowSize(engine, window), 0x0fff_0000, u32::from(packet[2].wrapping_sub(1)) << 16);
    replace(state, Field::WindowSize(engine, window), 0x0fff, u32::from(packet[3].wrapping_sub(1)));
    Ok(())
}
pub fn backing(state: &mut impl Storage, value: ModeWord) {
    let engine = selected_engine(state);
    replace(state, Field::Backing(engine), 0x3fff_ffff, value.0);
}
pub fn enabled(state: &mut impl Storage, value: ModeWord) -> Result<(), Error> {
    let engine = selected_engine(state);
    if state.read(Field::Initialized(engine)).0 == 0 { return Err(Error::Uninitialized); }
    if state.read(Field::Alternate).0 != 0 {
        replace(state, Field::Primary(engine), 1, 0);
        replace(state, Field::AlternatePrimary(engine), 1, u32::from(value.0 == 1));
    } else { replace(state, Field::Primary(engine), 1, u32::from(value.0 == 1)); }
    Ok(())
}
pub fn range(state: &mut impl Storage, group: RawGroup, item: u32, value: ModeWord) -> Result<(), Error> {
    let engine = selected_engine(state);
    if state.read(Field::Initialized(engine)).0 == 0 { return Err(Error::Uninitialized); }
    let item = crate::shadow::ItemSelection::parse(item).ok_or(Error::InvalidArgument)?;
    let mask = match item { crate::shadow::ItemSelection::All => 0x1111, crate::shadow::ItemSelection::One(item) => 1 << (item.index() * 4) };
    let alternate = state.read(Field::Alternate).0 != 0;
    replace(state, Field::Range(engine, group, alternate), mask, if value.0 == 1 { mask } else { 0 });
    Ok(())
}
pub fn colour(state: &mut impl Storage, value: ModeWord) {
    let engine = selected_engine(state);
    for (index, shift) in [16, 8, 0].into_iter().enumerate() { state.write(Field::Colour(engine, index), ModeWord((value.0 >> shift) & 255)); }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColourPacket {
    pub first: u8,
    pub second: u8,
    pub third: u8,
    pub flag: bool,
    pub value: ModeWord,
}
/// Pinned 0x6cbb24.
pub fn colour_packet(state: &mut impl Storage, packet: ColourPacket) {
    let engine = selected_engine(state);
    replace(state, Field::ColourPacketNibbles(engine), 0x0f, u32::from(packet.first));
    replace(state, Field::ColourPacketNibbles(engine), 0xf0, u32::from(packet.second) << 4);
    state.write(Field::ColourPacketThird(engine), ModeWord(u32::from(packet.third)));
    replace(state, Field::ColourPacketFlag(engine), 1, u32::from(packet.flag));
    state.write(Field::ColourPacketValue(engine), packet.value);
}

/// Pinned 0x6c994c.
pub const fn window_size() -> i32 { 0 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectError { Invalid }
impl SelectError { pub const fn stock(self) -> i32 { -30 } }
pub trait SelectRuntime {
    fn forced(&self) -> bool;
    fn selected(&self) -> Engine;
    fn panel_apply(&mut self, value: u32) -> Result<(), SelectError>;
    fn formatter_enable(&mut self, engine: Engine, value: u32) -> Result<(), SelectError>;
    fn formatter_strobe(&mut self, engine: Engine);
}
/// Pinned 0x6d1520.
pub fn select(runtime: &mut impl SelectRuntime, value: u32) -> i32 {
    let panel = runtime.panel_apply(value).err().map_or(0, SelectError::stock);
    let forced = runtime.forced();
    let engine = if forced { Engine::Zero } else { runtime.selected() };
    let formatter = runtime.formatter_enable(engine, value).err().map_or(0, SelectError::stock);
    if !forced { runtime.formatter_strobe(engine); }
    formatter | panel
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScalePacket {
    pub origin: [u16; 2], pub input: [u16; 2], pub output: [u16; 2],
    pub padding: u16, pub format: ModeWord, pub opacity: u8,
}
fn ratio(input: u16, output: u16) -> (u16, u16) {
    let numerator = if output > input { u32::from(input).wrapping_sub(1) } else { u32::from(input) };
    let ratio = (numerator.wrapping_shl(12) / u32::from(output)) as u16;
    let phase = if ratio == 4096 { 0 } else { ((u32::from(input).wrapping_sub(1).wrapping_shl(12).wrapping_sub(u32::from(output - 1).wrapping_mul(u32::from(ratio))) >> 5) & 255) as u16 };
    (ratio, phase)
}
pub fn scale(state: &mut impl Storage, packet: ScalePacket) -> Result<(), Error> {
    if packet.output.contains(&0) { return Err(Error::InvalidArgument); }
    let horizontal = ratio(packet.input[0], packet.output[0]);
    let vertical = ratio(packet.input[1], packet.output[1]);
    for (index, value) in [packet.origin[0], packet.origin[1], packet.input[0], packet.input[1], packet.output[0], packet.output[1], packet.padding].into_iter().enumerate() { state.write(Field::ScaleHistory(index), ModeWord(u32::from(value))); }
    state.write(Field::ScaleHistory(7), packet.format);
    state.write(Field::ScaleHistory(8), ModeWord(u32::from(packet.opacity)));
    let engine = selected_engine(state);
    replace(state, Field::ScaleDimensions(engine), 0x1fff_0000, u32::from(packet.input[0].wrapping_sub(1)) << 16);
    replace(state, Field::ScaleDimensions(engine), 0x1fff, u32::from(packet.input[1].wrapping_sub(1)));
    state.write(Field::ScaleRatio(engine), ModeWord((u32::from(horizontal.0) << 16) | u32::from(vertical.0)));
    replace(state, Field::ScalePhase(engine), 0x0fff_0000, u32::from(horizontal.1) << 16);
    replace(state, Field::ScalePhase(engine), 0x0fff, u32::from(vertical.1));
    replace(state, Field::ScaleOrigin(engine), 0x03ff_0000, if horizontal.0 == 4096 { 0 } else { u32::from(packet.origin[0]) << 16 });
    replace(state, Field::ScaleOrigin(engine), 0x03ff, if vertical.0 == 4096 { 0 } else { u32::from(packet.origin[1]) });
    replace(state, Field::ScalePadding(engine), 0x0fff, u32::from(packet.padding.saturating_sub(packet.input[0])));
    replace(state, Field::AlternateOrigin(engine), 0x3fff_0000, if horizontal.0 == 4096 { u32::from(packet.origin[0]) << 16 } else { 0 });
    replace(state, Field::AlternateOrigin(engine), 0x3fff, if vertical.0 == 4096 { u32::from(packet.origin[1]) } else { 0 });
    replace(state, Field::AlternateFormat(engine), 3, packet.format.0);
    state.write(Field::ScaleOpacity(engine), ModeWord(u32::from(packet.opacity.min(128))));
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControllerArgument(pub u16);
// Pinned leaf entries: 0x6d4068 and 0x6d4050.
pub fn auxiliary_frontend(_: ControllerArgument, _: ControllerArgument) -> i32 { 0 }
pub fn byte_frontend(_: ControllerArgument, _: ControllerArgument) -> i32 { 0 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresetSelector(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresetSource(u8);
impl PresetSource {
    pub fn parse(value: u32) -> Option<Self> { if value <= 11 { Some(Self(value as u8)) } else { None } }
    pub fn raw(self) -> u32 { u32::from(self.0) }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresetTarget(pub u32);
pub trait PresetRuntime {
    fn source(&self, selector: PresetSelector) -> ModeWord;
    fn current_source(&self) -> ModeWord;
    fn current_target(&self) -> PresetTarget;
    fn set_timing(&mut self, target: PresetTarget, source: PresetSource) -> i32;
}
pub fn preset(runtime: &mut impl PresetRuntime, selector: PresetSelector) -> i32 {
    let Some(source) = PresetSource::parse(runtime.source(selector).0) else { return Error::InvalidArgument.stock(); };
    if runtime.current_source().0 == source.raw() { return 0; }
    let target = runtime.current_target();
    runtime.set_timing(target, source)
}

#[cfg(all(target_arch = "arm", feature = "panel"))]
mod native {
    use super::*;
    use core::ptr::{read_volatile, write_volatile};
    unsafe extern "C" {
        fn xm1_scanout_completion_event(engine: u32, event: u32, callback: u32) -> i32;
        fn xm1_scanout_panel_device_initialize_program();
        fn xm1_scanout_panel_device_transfer(register: i16, value: u16) -> i32;
        fn xm1_scanout_panel_device_set_timing(device: u32, timing: u32) -> i32;
    }
    struct Live;
    fn saved(engine: Engine) -> usize { 0x96b9bc + engine.index() * 420 }
    fn alternate(engine: Engine) -> usize { 0x9554b8 + engine.index() * 572 }
    fn address(field: Field) -> (usize, usize) {
        match field {
            Field::ForcedEngine => (0x955b20, 4), Field::SelectedMode => (0x955b08, 4),
            Field::Initialized(e) => (0x9553cc + e.index() * 88 + 0x54, 1), Field::Alternate => (0x916610, 1),
            Field::WindowOrigin(e, w) => (saved(e) + 0x1c + w.index() * 8, 4),
            Field::WindowSize(e, w) => (saved(e) + 0x20 + w.index() * 8, 4),
            Field::Backing(e) => (saved(e) + 0x3c, 4), Field::Primary(e) => (saved(e), 1), Field::AlternatePrimary(e) => (alternate(e), 4),
            Field::Range(e, g, alt) => (if alt { alternate(e).wrapping_add((g.0 as usize).wrapping_mul(4)).wrapping_add(4) } else { saved(e).wrapping_add((g.0 as usize).wrapping_mul(76)).wrapping_add(0x54) }, 2),
            Field::Colour(e, i) => (saved(e) + 0x180 + i, 1),
            Field::ScaleHistory(i) => match i { 0..=6 => (0x955b34 + i * 2, 2), 7 => (0x955b44, 4), _ => (0x955b48, 1) },
            Field::ScaleDimensions(e) => (saved(e) + 0x38, 4), Field::ScaleRatio(e) => (saved(e) + 0x44, 4),
            Field::ScalePhase(e) => (saved(e) + 0x48, 4), Field::ScaleOrigin(e) => (saved(e) + 0x4c, 4), Field::ScalePadding(e) => (saved(e) + 0x40, 2),
            Field::AlternateOrigin(e) => (alternate(e) + 12, 4), Field::AlternateFormat(e) => (alternate(e) + 0x3c, 1), Field::ScaleOpacity(e) => (saved(e) + 0x17c, 1),
            Field::ColourPacketNibbles(e) => (saved(e) + 0x19c, 1), Field::ColourPacketThird(e) => (saved(e) + 0x19e, 1),
            Field::ColourPacketFlag(e) => (saved(e) + 0x19f, 1), Field::ColourPacketValue(e) => (saved(e) + 0x1a0, 4),
        }
    }
    impl Storage for Live {
        fn read(&self, field: Field) -> ModeWord { let (at, size) = address(field); ModeWord(unsafe { match size { 1 => u32::from(read_volatile(at as *const u8)), 2 => u32::from(read_volatile(at as *const u16)), _ => read_volatile(at as *const u32) } }) }
        fn write(&mut self, field: Field, value: ModeWord) { let (at, size) = address(field); unsafe { match size { 1 => write_volatile(at as *mut u8, value.0 as u8), 2 => write_volatile(at as *mut u16, value.0 as u16), _ => write_volatile(at as *mut u32, value.0) } } }
    }
    fn status(result: Result<(), Error>) -> i32 { result.map_or_else(Error::stock, |()| 0) }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn xm1_scanout_mode_completion(callback: u32) -> i32 { xm1_scanout_completion_event(selected_engine(&Live).index() as u32, 2, callback) }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn xm1_scanout_mode_window(selector: u32, packet: *const u16) -> i32 { let values = [read_volatile(packet), read_volatile(packet.add(1)), read_volatile(packet.add(2)), read_volatile(packet.add(3))]; status(window(&mut Live, selector, values)) }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn xm1_scanout_mode_backing(value: u32) -> i32 { backing(&mut Live, ModeWord(value)); 0 }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn xm1_scanout_mode_enabled(value: u32) -> i32 { status(enabled(&mut Live, ModeWord(value))) }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn xm1_scanout_mode_range(group: u32, item: u32, value: u32) -> i32 { status(range(&mut Live, RawGroup(group), item, ModeWord(value))) }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn xm1_scanout_mode_colour(value: u32) -> i32 { colour(&mut Live, ModeWord(value)); 0 }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn xm1_scanout_mode_scale(packet: *const u8) -> i32 {
        let half = |offset| read_volatile(packet.add(offset) as *const u16);
        if half(8) == 0 || half(10) == 0 { return -30; }
        status(scale(&mut Live, ScalePacket { origin: [half(0), half(2)], input: [half(4), half(6)], output: [half(8), half(10)], padding: half(12), format: ModeWord(read_volatile(packet.add(16) as *const u32)), opacity: read_volatile(packet.add(20)) }))
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn xm1_scanout_panel_program_frontend() { xm1_scanout_panel_device_initialize_program(); }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn xm1_scanout_panel_transfer_frontend(register: u32, value: u32) -> i32 { xm1_scanout_panel_device_transfer(register as i16, value as u16) }
    #[unsafe(no_mangle)]
    pub extern "C" fn xm1_scanout_panel_auxiliary_frontend(first: u16, second: u16) -> i32 { auxiliary_frontend(ControllerArgument(first), ControllerArgument(second)) }
    #[unsafe(no_mangle)]
    pub extern "C" fn xm1_scanout_panel_byte_frontend(first: u16, second: u16) -> i32 { byte_frontend(ControllerArgument(first), ControllerArgument(second)) }
    struct LivePreset;
    impl PresetRuntime for LivePreset {
        fn source(&self, selector: PresetSelector) -> ModeWord {
            let address = 0x7cee28_u32.wrapping_add(selector.0.wrapping_mul(12)).wrapping_add(4);
            ModeWord(unsafe { read_volatile(address as *const u32) })
        }
        fn current_source(&self) -> ModeWord { ModeWord(unsafe { read_volatile(0x96bd78 as *const u32) }) }
        fn current_target(&self) -> PresetTarget { PresetTarget(unsafe { read_volatile(0x96bd74 as *const u32) }) }
        fn set_timing(&mut self, target: PresetTarget, source: PresetSource) -> i32 { unsafe { xm1_scanout_panel_device_set_timing(target.0, source.raw()) } }
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn xm1_scanout_mode_preset(selector: u32) -> i32 { preset(&mut LivePreset, PresetSelector(selector)) }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn xm1_scanout_mode_window_size(_selector: u32, packet: *const u32) -> i32 {
        let _engine = selected_engine(&Live);
        let _packet = read_volatile(packet.add(1));
        window_size()
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn xm1_scanout_mode_colour_packet(packet: *const u8) -> i32 {
        colour_packet(&mut Live, ColourPacket {
            first: read_volatile(packet.add(4)),
            second: read_volatile(packet.add(5)),
            third: read_volatile(packet.add(6)),
            flag: read_volatile(packet) & 1 != 0,
            value: ModeWord(read_volatile(packet.add(8) as *const u32)),
        });
        0
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn xm1_scanout_mode_commit(callback: u32) -> i32 { unsafe { xm1_scanout_completion_event(selected_engine(&Live).index() as u32, 0, callback) } }
    struct LiveSelect;
    impl SelectRuntime for LiveSelect {
        fn forced(&self) -> bool { unsafe { read_volatile(0x955b20 as *const u32) != 0 } }
        fn selected(&self) -> Engine { selected_engine(&Live) }
        fn panel_apply(&mut self, value: u32) -> Result<(), SelectError> {
            if unsafe { crate::panel_device::native::apply_mode(value) } == 0 { Ok(()) } else { Err(SelectError::Invalid) }
        }
        fn formatter_enable(&mut self, engine: Engine, value: u32) -> Result<(), SelectError> {
            if unsafe { crate::panel_formatter::abi::enable_engine(engine, value) } == 0 { Ok(()) } else { Err(SelectError::Invalid) }
        }
        fn formatter_strobe(&mut self, engine: Engine) { unsafe { let _ = crate::abi::xm1_scanout_formatter_strobe(engine.index() as u32); } }
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn xm1_scanout_mode_select(value: u32) -> i32 { select(&mut LiveSelect, value) }
}

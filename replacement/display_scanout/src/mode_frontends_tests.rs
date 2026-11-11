use crate::mode_frontends::{self as mode, Field, ModeWord, RawGroup, ScalePacket, Storage, Window};
use crate::{Engine, Error};
use std::vec::Vec;

#[derive(Default)]
struct Memory { fields: Vec<(Field, ModeWord)>, switch_on_history: bool }
impl Memory {
    fn put(&mut self, field: Field, value: u32) { self.write(field, ModeWord(value)); }
    fn get(&self, field: Field) -> u32 { self.read(field).0 }
}
impl Storage for Memory {
    fn read(&self, field: Field) -> ModeWord { self.fields.iter().find(|entry| entry.0 == field).map_or(ModeWord(0), |entry| entry.1) }
    fn write(&mut self, field: Field, value: ModeWord) {
        if let Some(entry) = self.fields.iter_mut().find(|entry| entry.0 == field) { entry.1 = value; } else { self.fields.push((field, value)); }
        if self.switch_on_history && field == Field::ScaleHistory(8) { self.put(Field::ForcedEngine, 7); }
    }
}
fn live() -> Memory { let mut state = Memory::default(); state.put(Field::SelectedMode, 1); state }
fn packet() -> ScalePacket { ScalePacket { origin: [37, 59], input: [640, 480], output: [320, 480], padding: 704, format: ModeWord(0xdead_beef), opacity: 201 } }

#[test]
fn mode_selection_requires_exact_live_value_and_no_override() {
    let mut state = live();
    assert_eq!(mode::selected_engine(&state), Engine::One);
    state.put(Field::ForcedEngine, 2);
    assert_eq!(mode::selected_engine(&state), Engine::Zero);
    state.put(Field::ForcedEngine, 0);
    state.put(Field::SelectedMode, 2);
    assert_eq!(mode::selected_engine(&state), Engine::Zero);
}
#[test]
fn windows_preserve_reserved_bits_and_wrap_zero_dimensions() {
    let mut state = live();
    state.put(Field::WindowOrigin(Engine::One, Window::Third), 0xa123_b456);
    state.put(Field::WindowSize(Engine::One, Window::Third), 0xc111_d222);
    assert_eq!(mode::window(&mut state, 2, [0x1345, 0x2678, 0, 4097]), Ok(()));
    assert_eq!(state.get(Field::WindowOrigin(Engine::One, Window::Third)), 0xa345_b678);
    assert_eq!(state.get(Field::WindowSize(Engine::One, Window::Third)), 0xcfff_d000);
    let before = state.fields.clone();
    assert_eq!(mode::window(&mut state, 3, [1, 2, 3, 4]), Err(Error::InvalidArgument));
    assert_eq!(state.fields, before);
}
#[test]
fn backing_and_colour_accept_all_metadata_bits_without_initialization() {
    let mut state = live();
    state.put(Field::Backing(Engine::One), 0x8000_0000);
    mode::backing(&mut state, ModeWord(0xffff_4321));
    mode::colour(&mut state, ModeWord(0xab12_3456));
    assert_eq!(state.get(Field::Backing(Engine::One)), 0xbfff_4321);
    assert_eq!([state.get(Field::Colour(Engine::One, 0)), state.get(Field::Colour(Engine::One, 1)), state.get(Field::Colour(Engine::One, 2))], [0x12, 0x34, 0x56]);
}
#[test]
fn enable_uses_exact_one_and_clears_saved_enable_when_alternate() {
    let mut state = live();
    assert_eq!(mode::enabled(&mut state, ModeWord(1)), Err(Error::Uninitialized));
    state.put(Field::Initialized(Engine::One), 3);
    state.put(Field::Alternate, 8);
    state.put(Field::Primary(Engine::One), 0xf7);
    state.put(Field::AlternatePrimary(Engine::One), 0x8765_4320);
    assert_eq!(mode::enabled(&mut state, ModeWord(1)), Ok(()));
    assert_eq!(state.get(Field::Primary(Engine::One)), 0xf6);
    assert_eq!(state.get(Field::AlternatePrimary(Engine::One)), 0x8765_4321);
    assert_eq!(mode::enabled(&mut state, ModeWord(2)), Ok(()));
    assert_eq!(state.get(Field::AlternatePrimary(Engine::One)), 0x8765_4320);
}
#[test]
fn range_checks_capture_before_item_and_retains_raw_group() {
    let mut state = live();
    assert_eq!(mode::range(&mut state, RawGroup(17), 8, ModeWord(1)), Err(Error::Uninitialized));
    state.put(Field::Initialized(Engine::One), 1);
    assert_eq!(mode::range(&mut state, RawGroup(17), 8, ModeWord(1)), Err(Error::InvalidArgument));
    state.put(Field::Range(Engine::One, RawGroup(17), false), 0xa2a2);
    assert_eq!(mode::range(&mut state, RawGroup(17), 255, ModeWord(1)), Ok(()));
    assert_eq!(state.get(Field::Range(Engine::One, RawGroup(17), false)), 0xb3b3);
    assert_eq!(mode::range(&mut state, RawGroup(17), 2, ModeWord(9)), Ok(()));
    assert_eq!(state.get(Field::Range(Engine::One, RawGroup(17), false)), 0xb2b3);
}
#[test]
fn scale_records_raw_history_and_independent_axis_geometry() {
    let mut state = live();
    state.put(Field::ScaleDimensions(Engine::One), 0xe000_a000);
    state.put(Field::AlternateOrigin(Engine::One), 0xc000_c000);
    state.put(Field::ScaleOrigin(Engine::One), 0xfc00_fc00);
    state.put(Field::AlternateFormat(Engine::One), 0xa4);
    assert_eq!(mode::scale(&mut state, packet()), Ok(()));
    assert_eq!(state.get(Field::ScaleDimensions(Engine::One)), 0xe27f_a1df);
    assert_eq!(state.get(Field::ScaleRatio(Engine::One)), 0x2000_1000);
    assert_eq!(state.get(Field::ScalePhase(Engine::One)), 0x0080_0000);
    assert_eq!(state.get(Field::ScaleOrigin(Engine::One)), 0xfc25_fc00);
    assert_eq!(state.get(Field::AlternateOrigin(Engine::One)), 0xc000_c03b);
    assert_eq!(state.get(Field::ScalePadding(Engine::One)), 64);
    assert_eq!(state.get(Field::AlternateFormat(Engine::One)), 0xa7);
    assert_eq!(state.get(Field::ScaleOpacity(Engine::One)), 128);
    assert_eq!(state.get(Field::ScaleHistory(7)), 0xdead_beef);
    assert_eq!(state.get(Field::ScaleHistory(8)), 201);
    let before = state.fields.clone();
    assert_eq!(mode::scale(&mut state, packet()), Ok(()));
    assert_eq!(state.fields, before);
}
#[test]
fn invalid_scale_preserves_all_history_and_registers() {
    for axis in 0..2 {
        let mut state = live();
        state.put(Field::ScaleHistory(0), 0x8888);
        let before = state.fields.clone();
        let mut request = packet(); request.output[axis] = 0;
        assert_eq!(mode::scale(&mut state, request), Err(Error::InvalidArgument));
        assert_eq!(state.fields, before);
    }
}
#[test]
fn scale_selects_current_engine_after_history_publication() {
    let mut state = live(); state.switch_on_history = true;
    assert_eq!(mode::scale(&mut state, packet()), Ok(()));
    assert_eq!(state.get(Field::ScaleRatio(Engine::Zero)), 0x2000_1000);
    assert_eq!(state.get(Field::ScaleRatio(Engine::One)), 0);
}

#[test]
fn auxiliary_controller_frontends_accept_distinct_full_width_arguments() {
    for (first, second) in [(0x1234, 0xabcd), (0xffff, 0x8001), (0x4321, 0x9876)] {
        assert_eq!(mode::auxiliary_frontend(mode::ControllerArgument(first), mode::ControllerArgument(second)), 0);
        assert_eq!(mode::byte_frontend(mode::ControllerArgument(first), mode::ControllerArgument(second)), 0);
    }
}

struct Presets {
    current: u32,
    target: u32,
    timing: [u32; 4],
    result: i32,
}
impl mode::PresetRuntime for Presets {
    fn source(&self, selector: mode::PresetSelector) -> ModeWord { ModeWord([2, 7, 15, 4][selector.0 as usize]) }
    fn current_source(&self) -> ModeWord { ModeWord(self.current) }
    fn current_target(&self) -> mode::PresetTarget { mode::PresetTarget(self.target) }
    fn set_timing(&mut self, target: mode::PresetTarget, source: mode::PresetSource) -> i32 {
        self.timing[target.0 as usize] = source.raw();
        self.current = source.raw();
        self.result
    }
}
#[test]
fn preset_applies_selected_row_to_live_target_and_preserves_setter_result() {
    let mut state = Presets { current: 3, target: 2, timing: [9, 8, 6, 5], result: -83 };
    assert_eq!(mode::preset(&mut state, mode::PresetSelector(1)), -83);
    assert_eq!(state.current, 7);
    assert_eq!(state.timing, [9, 8, 7, 5]);
    state.target = 3;
    state.result = 0;
    assert_eq!(mode::preset(&mut state, mode::PresetSelector(3)), 0);
    assert_eq!(state.current, 4);
    assert_eq!(state.timing, [9, 8, 7, 4]);
}
#[test]
fn unchanged_preset_does_not_reprogram_target_or_propagate_unused_error() {
    let mut state = Presets { current: 2, target: 99, timing: [9, 8, 6, 5], result: -83 };
    assert_eq!(mode::preset(&mut state, mode::PresetSelector(0)), 0);
    assert_eq!(state.current, 2);
    assert_eq!(state.timing, [9, 8, 6, 5]);
}
#[test]
fn invalid_preset_source_preserves_target_and_current_timing() {
    let mut state = Presets { current: 3, target: 2, timing: [9, 8, 6, 5], result: 0 };
    assert_eq!(mode::preset(&mut state, mode::PresetSelector(2)), -30);
    assert_eq!(state.current, 3);
    assert_eq!(state.timing, [9, 8, 6, 5]);
}

#[test]
fn colour_packet_splits_nibbles_and_replaces_only_packet_fields() {
    let mut state = live();
    state.put(Field::ColourPacketNibbles(Engine::One), 0xa5);
    state.put(Field::ColourPacketThird(Engine::One), 0xff);
    state.put(Field::ColourPacketFlag(Engine::One), 0xfe);
    state.put(Field::ColourPacketValue(Engine::One), 0x1111_1111);
    mode::colour_packet(&mut state, mode::ColourPacket { first: 3, second: 12, third: 0x7e, flag: true, value: ModeWord(0x89ab_cdef) });
    assert_eq!(state.get(Field::ColourPacketNibbles(Engine::One)), 0xc3);
    assert_eq!(state.get(Field::ColourPacketThird(Engine::One)), 0x7e);
    assert_eq!(state.get(Field::ColourPacketFlag(Engine::One)), 0xff);
    assert_eq!(state.get(Field::ColourPacketValue(Engine::One)), 0x89ab_cdef);
}
#[test]
fn colour_packet_follows_forced_engine_selection() {
    let mut state = live();
    state.put(Field::ForcedEngine, 2);
    mode::colour_packet(&mut state, mode::ColourPacket { first: 1, second: 2, third: 3, flag: false, value: ModeWord(4) });
    assert_eq!(state.get(Field::ColourPacketValue(Engine::Zero)), 4);
    assert_eq!(state.get(Field::ColourPacketValue(Engine::One)), 0);
}
#[test]
fn window_size_leaf_has_no_effects() {
    assert_eq!(mode::window_size(), 0);
}

struct Selection { forced: bool, engine: Engine, panel: Result<(), mode::SelectError>, enable: Result<(), mode::SelectError>, log: Vec<&'static str>, enabled: Option<Engine> }
impl mode::SelectRuntime for Selection {
    fn forced(&self) -> bool { self.forced }
    fn selected(&self) -> Engine { self.engine }
    fn panel_apply(&mut self, _: u32) -> Result<(), mode::SelectError> { self.log.push("panel"); self.panel }
    fn formatter_enable(&mut self, engine: Engine, _: u32) -> Result<(), mode::SelectError> { self.log.push("enable"); self.enabled = Some(engine); self.enable }
    fn formatter_strobe(&mut self, _: Engine) { self.log.push("strobe"); }
}
fn selection(engine: Engine) -> Selection { Selection { forced: false, engine, panel: Ok(()), enable: Ok(()), log: Vec::new(), enabled: None } }

#[test]
fn select_applies_panel_then_enables_and_strobes_the_selected_engine() {
    let mut state = selection(Engine::One);
    assert_eq!(mode::select(&mut state, 1), 0);
    assert_eq!(state.log, ["panel", "enable", "strobe"]);
    assert_eq!(state.enabled, Some(Engine::One));
}
#[test]
fn select_forces_engine_zero_without_stroking() {
    let mut state = selection(Engine::One);
    state.forced = true;
    assert_eq!(mode::select(&mut state, 0), 0);
    assert_eq!(state.log, ["panel", "enable"]);
    assert_eq!(state.enabled, Some(Engine::Zero));
}
#[test]
fn select_keeps_strobing_after_a_panel_failure_and_combines_codes() {
    let mut state = selection(Engine::Zero);
    state.panel = Err(mode::SelectError::Invalid);
    assert_eq!(mode::select(&mut state, 1), -30);
    assert_eq!(state.log, ["panel", "enable", "strobe"]);
    state.enable = Err(mode::SelectError::Invalid);
    assert_eq!(mode::select(&mut state, 2), -30);
}


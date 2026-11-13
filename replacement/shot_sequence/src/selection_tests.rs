use crate::calculation_work::CalculationProfile;
use crate::grid_boundary::{BlendQuery, ClassLimits, ClassQuery, DefaultQuery, GridImaging, GridLibrary, GridProfiles, GridSetting, GridSettings, LevelQuery, LimitInputs, ReferenceQuery};
use crate::selection::{choose_classes, fix_classes, record_list, ListSource};
use crate::state::{ShotState, StateMemory};
use crate::state_tests::Memory;
use crate::stock::state as layout;
use std::collections::BTreeMap;
use xm1_saclac_interface::calculation::{AreaAt, AreaIndex, AreaResult, AreaWord, GridCell, ResultPass};
use xm1_saclac_interface::selection::{CellCode, SelectionClass, SelectionList, SelectionSlot, SubjectQuery};
use xm1_saclac_interface::RecordHandle;

/// Class codes as the image layer maps them: classes 2..5 to codes 1..4, everything else to 0.
#[derive(Default)]
struct Layers {
    profile: BTreeMap<u8, u32>,
    limits: ClassLimits,
    total_limits: ClassLimits,
    code: i16,
    default_class: u32,
}

fn key(value: CalculationProfile) -> u8 { value as u8 }

impl GridProfiles for Layers {
    fn profile(&mut self, _record: RecordHandle, value: CalculationProfile) -> u32 { self.profile.get(&key(value)).copied().unwrap_or(0) }
}

impl GridLibrary for Layers {
    fn selection_mode(&mut self, _record: RecordHandle, _refinement: u32) -> u32 { 0 }
    fn selection_flags(&mut self, _record: RecordHandle, _refinement: u32) -> u16 { 0 }
    fn selection_code(&mut self, _record: RecordHandle) -> i16 { self.code }
    fn class_limits(&mut self, _inputs: &LimitInputs) -> ClassLimits { self.limits }
    fn fixed_class_limits(&mut self) -> ClassLimits { ClassLimits { upper: 15, lower: 0 } }
    fn total_class_limits(&mut self, _total: i16) -> ClassLimits { self.total_limits }
    fn layout_areas(&mut self, _layout: u32) -> u8 { 3 }
    fn target_reference(&mut self, _query: &ReferenceQuery) -> u32 { 0 }
    fn target_level(&mut self, _query: &LevelQuery) -> i16 { 0 }
    fn subject_eligible(&mut self, _query: SubjectQuery) -> bool { false }
    fn focus_blend(&mut self, _query: &BlendQuery) -> u32 { 0 }
    fn settle_level(&mut self, level: i16) -> (i16, i16) { (level, 0) }
    fn floor_level(&mut self, level: i16) -> i16 { level }
}

impl GridImaging for Layers {
    fn class_code(&mut self, class: SelectionClass) -> u8 { if (2..=5).contains(&class.0) { class.0 as u8 - 1 } else { 0 } }
    fn code_class(&mut self, code: u8) -> SelectionClass { SelectionClass(if (1..=4).contains(&code) { u32::from(code) + 1 } else { 1 }) }
    fn class_cell(&mut self, class: SelectionClass) -> CellCode { CellCode { column: 0, row: self.class_code(class) } }
    fn cell_code(&mut self, class: SelectionClass) -> u8 { self.class_code(class) }
    fn cell_class(&mut self, _cell: CellCode) -> u32 { 0 }
    fn cell_kind(&mut self, _cell: CellCode) -> u32 { 0 }
    fn cell_level(&mut self, _cell: CellCode) -> i16 { 0 }
    fn cell_unset(&mut self, cell: CellCode) -> bool { cell.column == 0 }
    fn class_status(&mut self, _index: u32) -> u8 { 0 }
    fn level_scale(&mut self, _level: i16) -> u32 { 0 }
    fn level_class(&mut self, level: i32, value: i32) -> SelectionClass { SelectionClass((level + value) as u32 + 4) }
    fn exposure_value(&mut self) -> i32 { 0 }
    fn first_class_index(&mut self) -> u32 { 1 }
    fn class_level(&mut self, _index: u32) -> u16 { 0 }
    fn class_changed(&mut self, _query: &ClassQuery) -> bool { false }
    fn record_class(&mut self, _query: &ClassQuery) -> u32 { 0 }
    fn record_mode(&mut self, _record: RecordHandle) -> u32 { 0 }
    fn record_flags(&mut self, _record: RecordHandle) -> u16 { 0 }
    fn default_class(&mut self, _query: &DefaultQuery) -> SelectionClass { SelectionClass(self.default_class) }
}

impl GridSettings for Layers {
    fn setting(&mut self, _setting: GridSetting) -> i32 { 0 }
    fn lens_threshold(&mut self, _step: u8) -> i16 { 0 }
    fn lens_band(&mut self, _level: i16) -> u32 { 0 }
    fn reference_level(&mut self, _reference: u32, _lens_reference: u32) -> i16 { 0 }
    fn reference_offset(&mut self, _reference: u32) -> i16 { 0 }
}

fn record() -> RecordHandle { xm1_saclac_interface::stock::record_handle_from_a2(0x77) }

fn index(index: u8) -> AreaIndex { AreaIndex::new(index).unwrap() }

fn grid(pass: u32) -> ResultPass { ResultPass::Grid(xm1_saclac_interface::calculation::GridPass::new(pass).unwrap()) }

fn state() -> ShotState<Memory> { ShotState::new(Memory::default()) }

#[test]
fn a_live_list_holds_one_fixed_area_in_every_pass() {
    let mut layers = Layers::default();
    layers.profile.insert(key(CalculationProfile::SelectionKind), 1);
    layers.profile.insert(key(CalculationProfile::RowLimit), 9);
    let list = record_list(record(), ListSource::Live, &mut state(), &mut layers);
    assert!(list.fixed());
    assert_eq!(list.area_count(), 1);
    assert_eq!(list.class(SelectionSlot::plain(index(0))), SelectionClass(9));
    for pass in 0..3 { assert_eq!(list.class(SelectionSlot { kind: None, pass: grid(pass), index: index(0) }), SelectionClass(9)); }
    assert_eq!(list.weight(index(0)), 0);
}

#[test]
fn a_numbered_layout_numbers_the_record_defaults_and_weighs_each_area() {
    let mut layers = Layers { limits: ClassLimits { upper: 4, lower: 1 }, ..Layers::default() };
    layers.profile.insert(key(CalculationProfile::At0032), 13);
    layers.profile.insert(key(CalculationProfile::At003c), 0x40);
    let list = record_list(record(), ListSource::Record, &mut state(), &mut layers);
    assert!(list.fixed());
    assert_eq!(list.area_count(), 3);
    assert_eq!(list.upper_code(), 4);
    for area in 0..3 {
        assert_eq!(list.class(SelectionSlot::plain(index(area))), SelectionClass(u32::from(area) + 1));
        assert_eq!(list.class(SelectionSlot { kind: None, pass: grid(2), index: index(area) }), SelectionClass(u32::from(area) + 1));
        assert_eq!(list.weight(index(area)), 0x40);
    }
}

#[test]
fn a_client_list_takes_the_default_class_and_drops_weights_for_layout_12() {
    let mut layers = Layers { default_class: 3, ..Layers::default() };
    layers.profile.insert(key(CalculationProfile::At0032), 12);
    layers.profile.insert(key(CalculationProfile::At003c), 0x40);
    let list = record_list(record(), ListSource::Client(&[5, 6]), &mut state(), &mut layers);
    assert!(list.fixed());
    assert_eq!(list.area_count(), 2);
    assert_eq!(list.class(SelectionSlot::plain(index(1))), SelectionClass(3));
    assert_eq!(list.weight(index(1)), 0);
    assert_eq!(list.class(SelectionSlot::plain(index(2))), SelectionClass(0));
}

fn with_list(state: &mut ShotState<Memory>, fixed: bool, classes: &[u32]) {
    let mut list = SelectionList::default();
    list.set_fixed(fixed);
    for (area, class) in classes.iter().enumerate() { list.set_class(SelectionSlot::plain(index(area as u8)), SelectionClass(*class)); }
    state.set_selection(&list);
    state.memory().write(layout::AREA_COUNT, &(classes.len() as u32).to_le_bytes());
}

#[test]
fn fixed_classes_keep_their_class_and_take_its_cell() {
    let mut state = state();
    with_list(&mut state, true, &[4, 2]);
    choose_classes(record(), &mut state, &mut Layers::default());
    assert_eq!(state.selection_class(SelectionSlot::plain(index(0))), SelectionClass(4));
    assert_eq!(state.selection_cell(SelectionSlot::plain(index(0))), CellCode { column: 0, row: 3 });
    assert_eq!(state.selection_cell(SelectionSlot::plain(index(1))), CellCode { column: 0, row: 1 });
}

#[test]
fn a_bounded_class_is_clamped_into_the_record_limits() {
    let mut state = state();
    with_list(&mut state, false, &[5]);
    let mut layers = Layers { code: 1, limits: ClassLimits { upper: 2, lower: 0 }, total_limits: ClassLimits { upper: 15, lower: 0 }, ..Layers::default() };
    choose_classes(record(), &mut state, &mut layers);
    assert_eq!(state.selection_class(SelectionSlot::plain(index(0))), SelectionClass(3));
    assert_eq!(state.selection_cell(SelectionSlot::plain(index(0))), CellCode { column: 0, row: 2 });
}

#[test]
fn a_search_passes_over_occupied_areas_to_the_next_free_class() {
    let mut state = state();
    with_list(&mut state, false, &[4]);
    let mut occupied = AreaResult::default();
    occupied.set_word(AreaWord::Occupied, 1);
    state.set_area(AreaAt::Cell { pass: ResultPass::Plain, cell: GridCell::new(0, 2).unwrap(), index: index(0) }, &occupied);
    let mut layers = Layers { limits: ClassLimits { upper: 4, lower: 0 }, total_limits: ClassLimits { upper: 15, lower: 0 }, ..Layers::default() };
    choose_classes(record(), &mut state, &mut layers);
    assert_eq!(state.selection_class(SelectionSlot::plain(index(0))), SelectionClass(2));
    assert_eq!(state.selection_cell(SelectionSlot::plain(index(0))), CellCode { column: 0, row: 1 });
}

#[test]
fn layout_2_gives_later_areas_the_first_area_class_unchanged() {
    let mut state = state();
    with_list(&mut state, false, &[5, 2]);
    let mut layers = Layers { code: 1, limits: ClassLimits { upper: 2, lower: 0 }, total_limits: ClassLimits { upper: 15, lower: 0 }, ..Layers::default() };
    layers.profile.insert(key(CalculationProfile::At0032), 2);
    choose_classes(record(), &mut state, &mut layers);
    assert_eq!(state.selection_class(SelectionSlot::plain(index(0))), SelectionClass(3));
    assert_eq!(state.selection_class(SelectionSlot::plain(index(1))), SelectionClass(3));
}

#[test]
fn a_fixed_class_reaches_every_plain_and_pass_slot_as_a_column() {
    let mut state = state();
    with_list(&mut state, false, &[0, 0]);
    fix_classes(&mut state, &mut Layers::default());
    for area in 0..2 {
        assert_eq!(state.selection_class(SelectionSlot::plain(index(area))), SelectionClass(4));
        assert_eq!(state.selection_cell(SelectionSlot { kind: None, pass: grid(1), index: index(area) }), CellCode { column: 3, row: 0 });
    }
}

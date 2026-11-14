use crate::calculation_work::CalculationProfile;
use crate::grid_boundary::{BlendQuery, ClassLimits, ClassQuery, DefaultQuery, GridImaging, GridLibrary, GridProfiles, GridSetting, GridSettings, LevelQuery, LimitInputs, ReferenceQuery};
use crate::state::{ShotState, StateMemory};
use crate::state_tests::Memory;
use crate::stock::state as layout;
use std::collections::BTreeMap;
use std::vec::Vec;
use xm1_saclac_interface::calculation::{AreaAt, AreaHalf, AreaIndex, AreaResult, GridCell, GridPass, ResultPass, TrackedKind};
use xm1_saclac_interface::selection::{CellCode, SelectionClass, SelectionList, SubjectQuery};
use xm1_saclac_interface::RecordHandle;

/// Grid layers whose answers the tests set. Lens thresholds step by 100 from 100 at step 2.
pub(crate) struct Layers {
    pub profile: BTreeMap<u8, u32>,
    pub settings: Vec<(GridSetting, i32)>,
    pub mode: u32,
    pub reference: u32,
    pub plain_level: i16,
    pub grid_level: i16,
    pub eligible: bool,
    pub class_changed: bool,
    pub record_class: u32,
    pub lens_band: u32,
    pub first_class: u32,
    pub first_level: u16,
    pub ceiling: i16,
    pub floor: i16,
    pub cell_level: i16,
    pub blend: u32,
    pub distance: i16,
    pub offsets: BTreeMap<u32, i16>,
    pub level_queries: Vec<LevelQuery>,
}

impl Default for Layers {
    fn default() -> Self {
        Self {
            profile: BTreeMap::new(),
            settings: Vec::new(),
            mode: 0,
            reference: 0,
            plain_level: 0,
            grid_level: 0,
            eligible: false,
            class_changed: false,
            record_class: 0,
            lens_band: 0,
            first_class: 1,
            first_level: 0,
            ceiling: i16::MAX,
            floor: i16::MIN,
            cell_level: 0,
            blend: 0,
            distance: 0,
            offsets: BTreeMap::new(),
            level_queries: Vec::new(),
        }
    }
}

impl Layers {
    pub fn set_profile(&mut self, value: CalculationProfile, word: u32) { self.profile.insert(value as u8, word); }

    pub fn set(&mut self, setting: GridSetting, value: i32) { self.settings.push((setting, value)); }
}

impl GridProfiles for Layers {
    fn profile(&mut self, _record: RecordHandle, value: CalculationProfile) -> u32 { self.profile.get(&(value as u8)).copied().unwrap_or(0) }
}

impl GridLibrary for Layers {
    fn selection_mode(&mut self, _record: RecordHandle, _refinement: u32) -> u32 { self.mode }
    fn selection_flags(&mut self, _record: RecordHandle, _refinement: u32) -> u16 { 0 }
    fn selection_code(&mut self, _record: RecordHandle) -> i16 { 0 }
    fn class_limits(&mut self, _inputs: &LimitInputs) -> ClassLimits { ClassLimits { upper: 4, lower: 0 } }
    fn fixed_class_limits(&mut self) -> ClassLimits { ClassLimits { upper: 15, lower: 0 } }
    fn total_class_limits(&mut self, _total: i16) -> ClassLimits { ClassLimits { upper: 15, lower: 0 } }
    fn layout_areas(&mut self, _layout: u32) -> u8 { 1 }
    fn target_reference(&mut self, _query: &ReferenceQuery) -> u32 { self.reference }
    fn target_level(&mut self, query: &LevelQuery) -> i16 {
        self.level_queries.push(*query);
        if query.pass == ResultPass::Plain { self.plain_level } else { self.grid_level }
    }
    fn subject_eligible(&mut self, _query: SubjectQuery) -> bool { self.eligible }
    fn focus_blend(&mut self, _query: &BlendQuery) -> u32 { self.blend }
    fn settle_level(&mut self, level: i16) -> (i16, i16) { if level > self.ceiling { (self.ceiling, level - self.ceiling) } else { (level, 0) } }
    fn floor_level(&mut self, level: i16) -> i16 { level.max(self.floor) }
}

impl GridImaging for Layers {
    fn class_code(&mut self, class: SelectionClass) -> u8 { class.0 as u8 }
    fn code_class(&mut self, code: u8) -> SelectionClass { SelectionClass(u32::from(code)) }
    fn class_cell(&mut self, class: SelectionClass) -> CellCode { CellCode { column: class.0 as u8, row: 0 } }
    fn cell_code(&mut self, class: SelectionClass) -> u8 { class.0 as u8 }
    fn cell_class(&mut self, cell: CellCode) -> u32 { 10 + u32::from(cell.column) }
    fn cell_kind(&mut self, cell: CellCode) -> u32 { u32::from(cell.column >= 2) }
    fn cell_level(&mut self, _cell: CellCode) -> i16 { self.cell_level }
    fn cell_unset(&mut self, cell: CellCode) -> bool { cell.column == 0 }
    fn class_status(&mut self, index: u32) -> u8 { index as u8 }
    fn level_scale(&mut self, level: i16) -> u32 { (level / 2) as u32 }
    fn level_class(&mut self, _level: i32, value: i32) -> SelectionClass { SelectionClass(value as u32) }
    fn exposure_value(&mut self) -> i32 { 2 }
    fn first_class_index(&mut self) -> u32 { self.first_class }
    fn class_level(&mut self, _index: u32) -> u16 { self.first_level }
    fn class_changed(&mut self, _query: &ClassQuery) -> bool { self.class_changed }
    fn record_class(&mut self, _query: &ClassQuery) -> u32 { self.record_class }
    fn record_mode(&mut self, _record: RecordHandle) -> u32 { 0 }
    fn record_flags(&mut self, _record: RecordHandle) -> u16 { 0 }
    fn default_class(&mut self, _query: &DefaultQuery) -> SelectionClass { SelectionClass(1) }
}

impl GridSettings for Layers {
    fn setting(&mut self, setting: GridSetting) -> i32 { self.settings.iter().rev().find(|(set, _)| *set == setting).map_or(0, |(_, value)| *value) }
    fn lens_threshold(&mut self, step: u8) -> i16 { 100 * (i16::from(step) - 1) }
    fn lens_band(&mut self, _level: i16) -> u32 { self.lens_band }
    fn reference_level(&mut self, _reference: u32, _lens_reference: u32) -> i16 { self.distance }
    fn reference_offset(&mut self, reference: u32) -> i16 { self.offsets.get(&reference).copied().unwrap_or(0) }
}

pub(crate) fn record() -> RecordHandle { xm1_saclac_interface::stock::record_handle_from_a2(0x77) }

pub(crate) fn index(index: u8) -> AreaIndex { AreaIndex::new(index).unwrap() }

pub(crate) fn kind(kind: u8) -> TrackedKind { TrackedKind::new(kind).unwrap() }

pub(crate) fn cell(column: u8, row: u8) -> GridCell { GridCell::new(column, row).unwrap() }

pub(crate) fn grid(pass: u32) -> ResultPass { ResultPass::Grid(GridPass::new(pass).unwrap()) }

/// The shot's state with the block's counts and the exposure's grid set.
pub(crate) struct Setup {
    pub areas: u32,
    pub kinds: u8,
    pub grid: bool,
    pub columns: u8,
    pub rows: u8,
}

impl Setup {
    pub fn state(&self) -> ShotState<Memory> {
        let mut state = ShotState::new(Memory::default());
        let memory = state.memory();
        memory.write(layout::AREA_COUNT, &self.areas.to_le_bytes());
        memory.write(layout::TRACKING, &u32::from(self.kinds != 0).to_le_bytes());
        memory.write(layout::TRACKED_KINDS, &[self.kinds]);
        memory.write(layout::GRID_SELECTED, &u32::from(self.grid).to_le_bytes());
        memory.write(layout::EXPOSURE_COLUMNS, &[self.columns]);
        memory.write(layout::EXPOSURE_ROWS, &[self.rows]);
        state
    }
}

pub(crate) fn area(reference: u32, halves: &[(AreaHalf, i16)]) -> AreaResult {
    let mut area = AreaResult::default();
    area.set_reference(reference, 0);
    for (half, value) in halves { area.set_half(*half, *value); }
    area
}

pub(crate) fn put_area<M: StateMemory>(state: &mut ShotState<M>, at: AreaAt, area: &AreaResult) { state.set_area(at, area); }

pub(crate) fn save_list<M: StateMemory>(state: &mut ShotState<M>, list: &SelectionList) { state.memory().write(layout::SAVED_SELECTION, list.bytes()); }

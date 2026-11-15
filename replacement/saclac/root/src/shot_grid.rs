#![forbid(unsafe_code)]

use xm1_saclac_calc::{adjust::{exposure_limits::{LimitQuery, LimitSet}, focus_profile::{self, KindInputs}, rate_policy::RateKind, scene_settings::{self, NibbleInputs}, timing::LevelScale}, exposure::{grid, levels, ports::{Lens, Placement, ProfileAccess}, settings}, gains, policy::SelectionKind, record_policy, stock::calculation::Cell};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{calculation::ResultPass, selection::{CellCode, SelectionClass, SubjectQuery}, ImageProcessor, RecordHandle};
use xm1_saclac_profile_geometry::{controller::Tables, ports::Retained, profile::ProfileHandle, services::LevelPair};
use xm1_saclac_profile_records::{stock::{record::group_00, shared::group_27}, Memory, SharedHeaderByte, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{calculation_work::{CalculationProfile, CalculationProfiles}, grid_boundary::{BlendQuery, ClassLimits, ClassQuery, DefaultQuery, GridImaging, GridLibrary, GridProfiles, GridSetting, GridSettings, LevelQuery, LimitInputs, ReferenceQuery}};
use crate::{calculation_services::Calculation, shot_services::Libraries};

fn pass_index(pass: ResultPass) -> u32 {
    match pass { ResultPass::Plain => 4, ResultPass::Grid(pass) => u32::from(pass.index()) }
}

fn pair(cell: CellCode) -> LevelPair { LevelPair([cell.column, cell.row]) }

impl<M, R, E, P, I, T, A, Q> GridProfiles for Libraries<'_, M, R, E, P, I, T, A, Q>
where Self: CalculationProfiles {
    fn profile(&mut self, record: RecordHandle, value: CalculationProfile) -> u32 {
        CalculationProfiles::profile(self, record, value)
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, A: Placement, Q>
    GridLibrary for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn selection_mode(&mut self, record: RecordHandle, refinement: u32) -> u32 {
        self.services.borrow_mut().profile().selection_kind(record, refinement != 0)
    }
    fn selection_flags(&mut self, record: RecordHandle, refinement: u32) -> u16 {
        self.services.borrow_mut().profile().selection_flags(record, refinement != 0)
    }
    fn selection_code(&mut self, record: RecordHandle) -> i16 {
        levels::measured_level(&mut *self.services.borrow_mut(), record)
    }
    fn class_limits(&mut self, inputs: &LimitInputs) -> ClassLimits {
        let limits = scene_settings::nibbles(&mut *self.services.borrow_mut(), NibbleInputs {
            scene: inputs.mode, flags: inputs.flags, measured_level: inputs.code,
            policy: u8::try_from(inputs.at0045).unwrap_or(u8::MAX),
            armed: inputs.adjustment_option != 0,
            selector: u8::try_from(inputs.adjustment_kind).unwrap_or(u8::MAX),
            choice: u8::try_from(inputs.at003e).unwrap_or(u8::MAX),
        });
        ClassLimits { upper: limits.low, lower: limits.high }
    }
    fn fixed_class_limits(&mut self) -> ClassLimits {
        let limits = record_policy::fixed_class_limits(&mut *self.services.borrow_mut());
        ClassLimits { upper: limits.upper, lower: limits.lower }
    }
    fn total_class_limits(&mut self, total: i16) -> ClassLimits {
        let limits = record_policy::level_class_limits(&mut *self.services.borrow_mut(), total);
        ClassLimits { upper: limits.upper, lower: limits.lower }
    }
    fn layout_areas(&mut self, layout: u32) -> u8 {
        // X-M1 1.50 f625ec..f62620 compares the complete layout word.
        match layout { 5 => 2, 2 | 13 => 3, _ => 1 }
    }
    fn target_reference(&mut self, query: &ReferenceQuery) -> u32 {
        grid::target_reference(
            &mut Calculation { services: &mut self.services.borrow_mut(), geometry: &mut self.geometry.borrow_mut(), placement: &mut *self.placement.borrow_mut() },
            &self.exposure.context.borrow(), query.record,
            Cell { column: query.cell.column, row: query.cell.row }, query.lens_reference,
        )
    }
    fn target_level(&mut self, query: &LevelQuery) -> i16 {
        grid::target_level(&mut *self.services.borrow_mut(), &grid::LevelQuery {
            scene: query.mode, flags: query.flags, alternate: query.at004b != 0, variant: query.at0066 != 0,
            limit: LimitQuery { index: pass_index(query.pass), policy: query.at0045,
                armed: query.adjustment_option != 0, set: LimitSet::of_stage(u32::from(query.kind.get())),
                choice: query.at003e, minimum: query.at0024 },
        })
    }
    fn subject_eligible(&mut self, query: SubjectQuery) -> bool {
        SelectionKind(query.source).accepted(query.kind == 1, query.area != 0)
    }
    fn focus_blend(&mut self, query: &BlendQuery) -> u32 {
        u32::from(gains::focus_blend(&mut *self.services.borrow_mut(), &gains::BlendQuery {
            scene: query.mode, column: pass_index(query.pass), sensitivity: query.at0045,
            header_63: query.adjustment_option, variant: query.at003e,
        }))
    }
    fn settle_level(&mut self, level: i16) -> (i16, i16) {
        let split = crate::calibration_inputs::split_distance(&self.services.borrow().machine, level);
        (split.kept, split.excess)
    }
    fn floor_level(&mut self, level: i16) -> i16 {
        record_policy::floor_level(&mut *self.services.borrow_mut(), level)
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, A, Q>
    GridImaging for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn class_code(&mut self, class: SelectionClass) -> u8 { RateKind::from_code(class.0).kind() }
    fn code_class(&mut self, code: u8) -> SelectionClass { SelectionClass(RateKind::new(u32::from(code)).code()) }
    fn class_cell(&mut self, class: SelectionClass) -> CellCode {
        let LevelPair([column, row]) = self.geometry.borrow_mut().level_pair(0, class.0);
        CellCode { column, row }
    }
    fn cell_code(&mut self, class: SelectionClass) -> u8 {
        self.geometry.borrow_mut().ports().decode_level(class.0)
    }
    fn cell_class(&mut self, cell: CellCode) -> u32 { pair(cell).encoded_class() }
    fn cell_kind(&mut self, cell: CellCode) -> u32 { u32::from(pair(cell).class()) }
    fn cell_level(&mut self, cell: CellCode) -> i16 { self.geometry.borrow_mut().record_decade(pair(cell)) }
    fn cell_unset(&mut self, cell: CellCode) -> bool { pair(cell).first_complement() != 0 }
    fn class_status(&mut self, index: u32) -> u8 { self.geometry.borrow().profiles().entry(ProfileHandle(index)).feature }
    fn level_scale(&mut self, level: i16) -> u32 { self.services.borrow_mut().linear(level) }
    fn level_class(&mut self, level: i32, value: i32) -> SelectionClass {
        let machine = self.services.borrow().machine.clone();
        SelectionClass(machine.enter(0xef_743c, &[level as usize, value as usize]))
    }
    fn exposure_value(&mut self) -> i32 { 0 } // X-M1 1.50 eea1e4.
    fn first_class_index(&mut self) -> u32 { 1 } // X-M1 1.50 ef67c0.
    fn class_level(&mut self, index: u32) -> u16 { self.geometry.borrow().profiles().aspect_value(ProfileHandle(index)) as u16 }
    fn class_changed(&mut self, _: &ClassQuery) -> bool { false } // X-M1 1.50 ef6778.
    fn record_class(&mut self, _: &ClassQuery) -> u32 { 1 } // X-M1 1.50 ef67d8.
    fn record_mode(&mut self, record: RecordHandle) -> u32 {
        let mut services = self.services.borrow_mut();
        let mut profile = services.profile();
        if profile.header(record, SharedHeaderByte::At63) == 1 {
            profile.read_shared(record, &group_27::AT_073C)
        } else {
            u32::from(profile.header(record, SharedHeaderByte::At31))
        }
    }
    fn record_flags(&mut self, record: RecordHandle) -> u16 {
        let mut services = self.services.borrow_mut();
        let mut profile = services.profile();
        if profile.header(record, SharedHeaderByte::At63) == 1 {
            profile.read_shared(record, &group_27::AT_0740)
        } else {
            profile.read(record, &group_00::AT_002E)
        }
    }
    fn default_class(&mut self, query: &DefaultQuery) -> SelectionClass {
        SelectionClass(focus_profile::exposure_kind(&mut *self.services.borrow_mut(), &KindInputs {
            level: query.code, fixed: query.selection_kind == 1, scene: query.mode,
            mode: query.adjustment_kind, proposed: query.row_limit,
        }))
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q>
    GridSettings for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn setting(&mut self, setting: GridSetting) -> i32 {
        use xm1_shot_sequence::stock::state::SettingWidth;
        let (key, width) = xm1_shot_sequence::stock::state::grid_setting(setting);
        use xm1_saclac_shared::settings::{self as shared, AdjustmentKey, Adjustments};
        let services = self.services.borrow();
        let key = AdjustmentKey(u32::from(key));
        match width {
            SettingWidth::Byte => i32::from(services.stock.byte(key)),
            SettingWidth::SignedByte => i32::from(shared::signed_byte(&services.stock, key)),
            SettingWidth::Half => i32::from(shared::halfword(&services.stock, key) as i16),
        }
    }
    fn lens_threshold(&mut self, step: u8) -> i16 { settings::level_step(&mut *self.services.borrow_mut(), step) }
    fn lens_band(&mut self, level: i16) -> u32 { u32::from(focus_profile::level_band(&mut *self.services.borrow_mut(), level)) }
    fn reference_level(&mut self, reference: u32, lens_reference: u32) -> i16 { self.services.borrow_mut().level(reference, lens_reference) }
    fn reference_offset(&mut self, reference: u32) -> i16 { self.services.borrow_mut().reference_offset(reference) }
}

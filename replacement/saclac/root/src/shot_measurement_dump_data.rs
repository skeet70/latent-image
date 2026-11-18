#![forbid(unsafe_code)]

use xm1_saclac_interface::{area_measurement::AreaEntry, RecordHandle};
use xm1_saclac_profile_records::{stock::record as r, Index, Memory};
use xm1_shot_sequence::secondary_boundary::{AreaList, AreaLists};
use xm1_shot_sequence::{boundary::Frame, dump_boundary::{AdjustmentKey, DumpSettings, DumpWorkspaces, FieldWidth, Workspace, WorkspaceField}};
use crate::shot_services::Libraries;
use xm1_saclac_shared::{display::{CaptureDisplay, CaptureDisplayStatus}, settings::{Adjustments, Channel}, stock::Machine};
use crate::calibration_inputs::Native;

impl<M: Machine + Clone, R, E, P, I, T, A, Q> DumpSettings for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn adjustment_byte(&mut self, key: AdjustmentKey) -> u8 {
        self.services.borrow().stock.byte(xm1_saclac_shared::settings::AdjustmentKey(key.0))
    }

    fn settings_class(&mut self) -> u32 {
        match self.services.borrow().stock.capture_display() {
            CaptureDisplay::FeatureOff => 0,
            CaptureDisplay::ModeOne => 2,
            CaptureDisplay::ModeTwo => 3,
            CaptureDisplay::OtherMode => 1,
        }
    }

    fn export_configuration(&mut self, frame: Frame, distance: u32) {
        let services = self.services.borrow();
        xm1_saclac_config::stock::StockDumpBuffer { address: frame.0, memory: Native(&services.machine, &services.stock) }.export_configuration(
            &services.stock, &mut Native(&services.machine, &services.stock), &mut Native(&services.machine, &services.stock), Channel(distance as i32),
        );
    }
}

impl<M, R, E, P, I, T, A, Q> DumpWorkspaces for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn workspace_field(&mut self, workspace: Workspace, field: WorkspaceField) -> u32 {
        use xm1_saclac_calc::stock::scene_recognition::{self as stock, Width, Workspace as Owned};
        let workspace = match workspace {
            Workspace::PrimaryGrid => Owned::SavedEvaluation,
            Workspace::SecondaryGrid => Owned::SavedFeatures,
            Workspace::GridParameters => Owned::LiveTrace,
            Workspace::GridLimits => Owned::CaptureTrace,
            Workspace::PrimaryKeyDefaults => Owned::PrimaryCodes,
            Workspace::SecondaryKeyDefaults => Owned::SecondaryCodes,
            Workspace::TertiaryKeyDefaults => Owned::Changes,
            Workspace::KeyReferences => Owned::References,
            Workspace::KeyIndexes => Owned::Stationary,
            Workspace::ScaledInputs => Owned::Comparisons,
        };
        let width = match field.width {
            FieldWidth::Byte => Width::Byte, FieldWidth::Half => Width::Half,
            FieldWidth::SignedHalf => Width::SignedHalf, FieldWidth::Word => Width::Word,
        };
        stock::workspace_field(&self.recognition.borrow(), workspace, field.offset, width)
    }
}

impl<M, R: Memory, E, P, I, T, A, Q> AreaLists for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn area_count(&mut self, record: RecordHandle, list: AreaList) -> u32 {
        let field = match list { AreaList::First => &r::group_30::AT_187C, AreaList::Second => &r::group_30::AT_1C90 };
        self.services.borrow().records.read(record, field).expect("measurement area count record")
    }

    fn area(&mut self, record: RecordHandle, list: AreaList, index: u8) -> AreaEntry {
        let index = Index::new(usize::from(index)).expect("measurement record area index");
        let field = match list { AreaList::First => &r::group_30::AT_1880, AreaList::Second => &r::group_30::AT_1C94 };
        AreaEntry(self.services.borrow().records.read_element(record, field, index).expect("measurement area record"))
    }

    fn list_detail(&mut self, record: RecordHandle, list: AreaList) -> u32 {
        let field = match list { AreaList::First => &r::group_30::AT_20A8, AreaList::Second => &r::group_30::AT_20AC };
        self.services.borrow().records.read(record, field).expect("measurement list detail record")
    }

    fn prefers_second_list(&mut self, record: RecordHandle) -> bool {
        self.services.borrow().records.read(record, &r::group_30::AT_20A4).expect("measurement list preference record") != 0
    }

    fn store_slot_extent(&mut self, record: RecordHandle, extent: [u32; 4]) {
        let _ = self.services.borrow_mut().records.write_element(record, &r::group_07::AT_01C4, Index::new(0).expect("first measurement slot"), words(extent));
    }

    fn store_slot_cells(&mut self, record: RecordHandle, cells: [u32; 4]) {
        let _ = self.services.borrow_mut().records.write_element(record, &r::group_07::AT_01D4, Index::new(0).expect("first measurement slot"), words(cells));
    }
}

fn words(values: [u32; 4]) -> [u8; 16] {
    let mut bytes = [0; 16];
    for (target, value) in bytes.chunks_exact_mut(4).zip(values) { target.copy_from_slice(&value.to_le_bytes()); }
    bytes
}

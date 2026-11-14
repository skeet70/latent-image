#![forbid(unsafe_code)]

use core::cell::RefCell;
use xm1_saclac_calc::adjust::{configuration::{self, ControllerFlag, LibraryDiagnostics, MotionRecord, ProfileStore}, result_dispatch};
use xm1_saclac_calc::exposure::{levels, ports::{ProfileAccess, Properties, Property}};
use xm1_saclac_calc::profile::Profile;
use xm1_saclac_config::{channel, limits, table};
use xm1_saclac_controllers::layers::*;
use xm1_saclac_interface::{statistics::Source, CompletionStatus, RecordHandle};
use xm1_saclac_profile_records::{Memory, ProfileRecords, SharedHeaderByte, StockEntries};
use xm1_saclac_profile_records::stock::{record, shared};
use xm1_saclac_shared::{commands::LensDriveReply, lens::{LensNotices, Notice}, settings::{self, AdjustmentKey, AspectPair, CaptureMode, Channel, WindowScale}, stock::{Argument::Buffer, Machine}};
use crate::{services::{Services, NO_HANDLE}, stock::acquisition::Aligned};

#[derive(Clone, Copy, Default)]
pub struct SelectionPublication {
    pub kind: u32,
    pub speed: u32,
    pub area: u32,
    pub exposure: u32,
}

pub(crate) struct Library<'a, M, R> {
    pub machine: &'a mut M,
    pub records: &'a mut ProfileRecords<R>,
    pub selection: &'a mut SelectionPublication,
}

impl<M: Machine, R: Memory> LibraryDiagnostics for Library<'_, M, R> {
    fn publish_selection(&mut self, record: RecordHandle) {
        let mut motion = MotionRecord::decode(self.records.read_shared(record, &shared::group_27::AT_07EC).unwrap_or([0; 40]));
        motion.published_kind = self.selection.kind;
        motion.published_speed = self.selection.speed as u16;
        motion.published_area = self.selection.area;
        motion.published_exposure = self.selection.exposure;
        let _ = self.records.write_shared(record, &shared::group_27::AT_07EC, motion.encode());
    }
    fn snapshot_diagnostics(&mut self) {
        if self.machine.enter(0x69_f8fc, &[0xa8]) & 0x80 != 0 {
            self.machine.enter(0x74_213c, &[0x5ffd_b9ec, 0x5fea_d820, 0xb598]);
        }
    }
    fn reset_selection(&mut self) {
        *self.selection = SelectionPublication { kind: 3, ..SelectionPublication::default() };
    }
    fn reset_diagnostics(&mut self) {
        for base in [0x1fea_2280usize, 0x5fea_d820] {
            self.machine.enter(0x74_2154, &[base, 0, 0xb598]);
            self.machine.enter(0x74_2154, &[base, 255, 0x2c00]);
            self.machine.enter(0x74_2154, &[base + 0x2c18, 255, 0x8400]);
            self.machine.enter(0x74_2154, &[base + 0xb018, 255, 0x168]);
            self.machine.write_u32((base + 0xb188) as u32, 3);
        }
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> ProfileAccess for Services<M, R, E, P> {
    type Records = R;
    type Entries = E;
    fn profile(&mut self) -> Profile<'_, R, E> { Profile::new(&self.records, &mut self.entries) }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P> ProfileStore for Services<M, R, E, P> {
    fn records(&mut self) -> &mut ProfileRecords<R> { &mut self.records }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Properties for Services<M, R, E, P> {
    fn property(&mut self, property: Property) -> u32 { u32::from(settings::Adjustments::byte(&self.stock, AdjustmentKey(property.key()))) }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Services<M, R, E, P> {
    pub fn publish_scaled_selection(&mut self, selection: SelectionPublication) { self.selection_publication = selection; }
    pub(crate) fn selection_mode(&mut self, record: RecordHandle) -> u32 {
        if self.entries.header_byte(record, SharedHeaderByte::At63) != 1 {
            return u32::from(self.entries.header_byte(record, SharedHeaderByte::At31));
        }
        if self.entries.measurement_selection(record) == xm1_saclac_profile_records::MeasurementSelection::Record {
            self.shared(record, &shared::group_27::AT_073C)
        } else { 0 }
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Geometry for Services<M, R, E, P> {
    fn value(&mut self, record: RecordHandle, value: GeometryValue) -> u32 {
        match value {
            GeometryValue::StartValue => 1, // 0xfaaea8.
            GeometryValue::SelectionMode => self.selection_mode(record),
            GeometryValue::Adjustment => result_dispatch::second_result(self, record).code(),
            GeometryValue::ScanSelection => u32::from(self.profile().grid_layout(record)),
            GeometryValue::Format => u32::from(self.field(record, &record::group_00::AT_004E)),
        }
    }
    fn motion(&mut self) -> u32 { configuration::shared::seen(self) }
    fn outgrown(&mut self, record: RecordHandle, check: GeometryCheck) -> bool {
        let mode = match check { GeometryCheck::Autofocus => 0, GeometryCheck::Command17 => 1, GeometryCheck::Command20 => 2, GeometryCheck::Command23 => 5 };
        result_dispatch::adopt_result(self, record, mode) == result_dispatch::Adoption::Refused
    }
    fn admits(&mut self, _: u32, _: u32) -> bool { true } // 0xfa7c0c returns 1.
    fn set(&mut self, flag: GeometryFlag, on: bool) {
        match flag {
            GeometryFlag::Alternate => self.search.configuration.set_controller_flag(ControllerFlag::Second, on),
            GeometryFlag::Capture => self.search.configuration.set_controller_flag(ControllerFlag::First, on),
            GeometryFlag::Configuring => configuration::shared::store_seen(self, u32::from(on)),
        }
    }
    fn alternate(&mut self) -> bool { self.search.configuration.controller_flag(ControllerFlag::Second) }
    fn lens_follows(&mut self) -> bool { configuration::shared::word(self) == 1 }
    fn reset_selection(&mut self) {
        let mut library = Library { machine: &mut self.machine, records: &mut self.records, selection: &mut self.selection_publication };
        self.search.configuration.reset_histories(&mut library);
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Configuration for Services<M, R, E, P> {
    fn hold(&mut self, held: bool) { self.machine.enter(if held { 0x74_046c } else { 0x74_0404 }, &[4]); }
    fn claim_selection(&mut self) { self.machine.enter(0x74_0d50, &[4]); }
    fn publish(&mut self, handle: RecordHandle, publication: Publication) {
        let (entry, value) = match publication {
            Publication::Mode => (0x74_0cf8, u32::from(self.field(handle, &record::group_00::AT_0041))),
            Publication::SelectionMode => (0x74_0cc4, self.selection_mode(handle)),
            Publication::Range => (0x74_0bd4, u32::from(self.field(handle, &record::group_00::AT_0044))),
            Publication::Position => (0x74_0b80, levels::measured_level(self, handle) as u32),
            Publication::Window => (0x74_0ad8, u32::from(self.field(handle, &record::group_00::AT_005A))),
        };
        self.machine.enter(entry, &[value as usize]);
    }
    fn search_property(&mut self) -> u32 { u32::from(settings::Adjustments::byte(&self.stock, AdjustmentKey(0x3e0009))) }
    fn combine_lens_pair(&mut self, first: u32, second: u32, third: u32) -> u32 {
        self.machine.enter(0x74_714c, &[first as usize, second as usize, third as usize])
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> MirrorSources for Services<M, R, E, P> {
    fn capture_window(&mut self, mode: u32, ratio: u32) -> Option<[u32; 2]> {
        let mode = CaptureMode::new(mode)?;
        match settings::capture_window(mode, AspectPair([ratio as u16, (ratio >> 16) as u16]), &mut self.stock).ok()? {
            WindowScale::Unscaled => None,
            WindowScale::Scaled(window) => Some([window.alternate, window.value]),
        }
    }
    fn collapse_index(&mut self, index: u32) -> u32 { table::column(&self.stock, table::Step(index)) }
    fn linearize(&mut self, value: u32) -> u32 { self.machine.enter(0xee_9ab0, &[value as usize]) }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Pipeline for Services<M, R, E, P> {
    fn stop_queried(&mut self) -> bool {
        let mut words = Aligned([0; 8]);
        self.machine.enter_with(0xfe_a7cc, &[Buffer(0)], &mut words);
        u32::from_le_bytes(words[..4].try_into().expect("pipeline status")) == 1
    }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Adjustment for Services<M, R, E, P> {
    fn mode_position(&mut self, record: RecordHandle) -> u32 { self.selection_mode(record) }
    fn refresh_dispatch(&mut self, record: RecordHandle) -> u32 { u32::from(self.field(record, &record::group_00::AT_004E)) }
    fn first_dispatch(&mut self, _: RecordHandle) -> u32 { 1 } // 0xfaaea8.
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> LensControl for Services<M, R, E, P> {
    fn overridden(&mut self, probe: LensProbe) -> bool {
        self.machine.enter(match probe { LensProbe::First => 0x76_d9a8, LensProbe::Second => 0x76_da38, LensProbe::Third => 0x76_dba4 }, &[]) == 2
    }
    fn value(&mut self) -> u32 { self.machine.enter(0x76_da48, &[]) }
    fn position(&mut self) -> u32 { self.machine.enter(0x76_da84, &[]) }
    fn mode_index(&mut self) -> u32 { self.machine.enter(0x76_dad4, &[]) }
    fn reference(&mut self) -> u32 { self.machine.enter(0x76_f340, &[]) }
    fn step(&mut self) -> u32 { self.machine.enter(0x76_fc4c, &[]) }
    fn channel(&mut self) -> (u32, u16) {
        let mut bytes = Aligned([0; 8]);
        self.machine.enter_with(0x76_fbac, &[Buffer(0), Buffer(4)], &mut bytes);
        (u32::from_le_bytes(bytes[..4].try_into().expect("lens channel")), u16::from_le_bytes(bytes[4..6].try_into().expect("lens fraction")))
    }
    fn third_notice(&mut self) -> u32 { self.stock.notice(Notice::Third) }
    fn configured_channel(&mut self) -> u32 {
        let channel = self.machine.enter(0x76_d9c8, &[]);
        if channel::is_valid(&self.stock, Channel(channel as i32)) { channel } else { 0 }
    }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P> LensDriveReplies for Services<M, R, E, P> {
    fn report_progress(&mut self, progress: DriveProgress) {
        let reply = LensDriveReply { status: CompletionStatus(18), channel: Channel(progress.channel as i32), words: [u32::from(progress.fourth_carried), progress.reported.0] };
        self.commands.reply_lens_drive(reply, &channel::ConfiguredChannels(&self.stock), &mut xm1_saclac_shared::stock::Stock::new(self.machine.clone()));
    }
}

fn position_query(query: CurveQuery) -> limits::PositionQuery {
    limits::PositionQuery { first: query.range, export_first: query.reference, direction_mode: query.mode_index, export_second: query.step }
}
fn positions(value: limits::CurvePositions) -> [u32; 3] { [value.centre, value.lower, value.upper] }
impl<M: Machine + Clone, R: Memory, E: StockEntries, P> LensCurve for Services<M, R, E, P> {
    fn level(&mut self, steps: u16, base: u32, reference: u32, mode_index: u32) -> i16 {
        self.machine.enter(0x74_99a0, &[usize::from(steps), base as usize, 0, reference as usize, mode_index as usize]) as i16
    }
    fn interpolates(&mut self) -> bool { self.machine.enter(0x74_6018, &[]) != 0 }
    fn positions(&mut self, level: i16, channel: u32, query: CurveQuery) -> [u32; 3] {
        positions(limits::curve_positions(&self.stock, &mut xm1_saclac_search_bindings::calc::Derivations::new(&self.machine), level, Channel(channel as i32), position_query(query)))
    }
    fn interpolated_positions(&mut self, level: i16, channel: u32, fraction: u16, query: CurveQuery) -> Option<[u32; 3]> {
        limits::interpolated_curve_positions(&self.stock, &mut xm1_saclac_search_bindings::calc::Derivations::new(&self.machine), level, Channel(channel as i32), u32::from(fraction), position_query(query)).map(positions)
    }
    fn mapped_value(&mut self, second: u32, first: u32) -> u32 { u32::from(table::value(&self.stock, table::Step(second), table::Row(first))) }
    fn combine_lens_pair(&mut self, first: u32, second: u32, third: u32) -> u32 { Configuration::combine_lens_pair(self, first, second, third) }
}

#[derive(Clone, Copy)]
pub enum WindowExtent { Live, S1 }

pub struct SharedRecords<'a, M, R, E, P> {
    pub services: &'a RefCell<Services<M, R, E, P>>,
    pub extent: WindowExtent,
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P> ProfileShared for SharedRecords<'_, M, R, E, P> {
    fn measured_window(&mut self) -> ([u16; 3], u32) {
        let services = self.services.borrow();
        let bytes = services.shared_bytes(NO_HANDLE, &shared::group_22::AT_0180);
        let half = |at| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
        // Machine 26 reads word 4; machine 17 tests word 3 (0xf22960).
        let extent = match self.extent { WindowExtent::Live => 16, WindowExtent::S1 => 12 };
        ([half(0), half(2), half(4)], u32::from_le_bytes(bytes[extent..extent + 4].try_into().expect("window extent")))
    }
    fn source(&mut self) -> Source {
        let services = self.services.borrow();
        let bytes = services.shared_bytes(NO_HANDLE, &shared::group_23::AT_0194);
        Source { word: u32::from_le_bytes(bytes[4..8].try_into().expect("measurement source")), byte: bytes[12] }
    }
}

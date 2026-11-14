#![forbid(unsafe_code)]

use core::cell::RefCell;
use xm1_saclac_calc::{adjust::{configuration::ProfileStore, result_dispatch::{self, ServiceState}}, exposure::ports::ProfileAccess, profile::Profile};
use xm1_saclac_controllers::live_profile::{ConfigurationSource, ConfigurationValues, ValueGroup};
use xm1_saclac_interface::{LensDc, RecordHandle};
use xm1_saclac_profile_records::{field::Index, stock::{record, shared}, Memory, ProfileRecords, SharedHeaderByte, StockEntries};
use xm1_saclac_shared::{lens, settings::{self, AdjustmentKey, ChannelRemap, RemapArguments}, stock::Machine};
use xm1_shot_sequence::{shot_ports::ShotPorts, state::StateMemory};
use crate::services::Services;

struct Records<'a, R, E> { records: &'a mut ProfileRecords<R>, entries: &'a mut E }
impl<R: Memory, E: StockEntries> ProfileAccess for Records<'_, R, E> {
    type Records = R;
    type Entries = E;
    fn profile(&mut self) -> Profile<'_, R, E> { Profile::new(self.records, self.entries) }
}
impl<R: Memory, E: StockEntries> ProfileStore for Records<'_, R, E> {
    fn records(&mut self) -> &mut ProfileRecords<R> { self.records }
}
struct Service<'a, M>(&'a M);
impl<M: Machine> ServiceState for Service<'_, M> {
    fn service_state(&mut self) -> u32 { self.0.enter(0x74_2054, &[]) }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Services<M, R, E, P> {
    pub(crate) fn configuration_values(&mut self, handle: RecordHandle) -> ConfigurationValues {
        let first = self.shared(handle, &shared::group_27::AT_0724);
        let fourth = self.shared(handle, &shared::group_27::AT_071C);
        let base: i16 = self.shared(handle, &shared::group_27::AT_070E);
        let seventh = base.wrapping_add(self.machine.enter(0x77_2f28, &[fourth as usize]) as i16);
        let group = ValueGroup {
            first, second: self.shared(handle, &shared::group_27::AT_0728), third: self.shared(handle, &shared::group_27::AT_072A),
            fourth, fifth: self.shared(handle, &shared::group_27::AT_0708), sixth: self.shared(handle, &shared::group_27::AT_070C) as u16,
            seventh: seventh as u16, eighth: self.shared(handle, &shared::group_27::AT_0716) as u16,
            ninth: self.shared(handle, &shared::group_27::AT_072C), tenth: self.shared(handle, &shared::group_27::AT_0718) as u16,
            eleventh: u32::from(matches!(self.shared(handle, &shared::group_27::WINDOW_STATUS).0, 3 | 4)),
        };
        let chosen = self.records.chosen_word(handle).unwrap_or_default();
        let pair = [self.field(handle, &record::group_04::AT_0164), self.field(handle, &record::group_04::AT_0168)];
        let window = [chosen, pair[0], pair[1]];
        let bytes = self.field(handle, &record::group_13::AT_20B8);
        let block = core::array::from_fn(|index| u32::from_le_bytes(bytes[4 * index..4 * index + 4].try_into().expect("view window word")));
        let ratio = self.field(handle, &record::group_00::AT_0000);
        let classes = self.field(handle, &record::group_00::AT_0004);
        let rate = result_dispatch::dispatch_option(&mut Records { records: &mut self.records, entries: &mut self.entries }, &mut Service(&self.machine), handle);
        let scan_selection = Profile::new(&self.records, &mut self.entries).grid_layout(handle);
        ConfigurationValues {
            head: [first, self.shared(handle, &shared::group_20::AT_014C), self.field(handle, &record::group_13::AT_20C8)],
            windows: [window; 2], block, group, converted: [ratio as u16, (ratio >> 16) as u16],
            properties: [0x8797, 0x8798, 0x8799, 0x879a].map(|key| u16::from(settings::Adjustments::byte(&self.stock, AdjustmentKey(key)))),
            tail: [u32::from(scan_selection), u32::from(self.field(handle, &record::group_00::AT_004F)), u32::from(self.field(handle, &record::group_00::AT_0050))],
            restorable: self.field(handle, &record::group_00::AT_0067), level: self.field(handle, &record::group_00::AT_000C),
            detail: self.field(handle, &record::group_00::AT_0070), classes: [classes[0], classes[4]],
            scale: self.entries.header_byte(handle, SharedHeaderByte::At61),
            capture: u8::from(self.search.configuration.controller_flag(xm1_saclac_calc::adjust::configuration::ControllerFlag::First)),
            rate, restarts: u32::from(self.entries.header_byte(handle, SharedHeaderByte::At47)),
        }
    }
}

pub struct LiveSource<'a, M, R, E, P, S, X> {
    pub services: &'a RefCell<Services<M, R, E, P>>,
    pub shot: &'a RefCell<ShotPorts<S, X>>,
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P: LensDc, S: StateMemory, X> ConfigurationSource for LiveSource<'_, M, R, E, P, S, X> {
    fn values(&mut self, record: RecordHandle) -> ConfigurationValues { self.services.borrow_mut().configuration_values(record) }
    fn take_offset(&mut self, record: RecordHandle) -> u16 {
        let mut services = self.services.borrow_mut();
        let mut range = services.shared_bytes(record, &shared::group_17::LENS_SNAPSHOT);
        let word = |at| u32::from_le_bytes(range[at..at + 4].try_into().expect("release range word"));
        let mut position = services.machine.enter(0x76_da84, &[]) as i16;
        if word(32) == 0 { return services.machine.enter(0x76_da84, &[]) as u16; }
        if services.entries.header_byte(record, SharedHeaderByte::At47) == 0 {
            let Services { peers, stock, .. } = &mut *services;
            let _ = lens::synchronize(peers, stock);
            position = services.machine.enter(0x76_da84, &[]) as i16;
        }
        let reference = self.shot.borrow_mut().state().request_word();
        let selector = services.records.read_element(record, &record::group_05::AT_0170, Index::new(0).expect("first lens selector")).unwrap_or_default();
        let convert = services.field(record, &record::group_00::AT_0064) == 1;
        let mode = services.machine.enter(0x76_dad4, &[]);
        let arguments = RemapArguments([position as i32 as u32, reference, selector, word(8), u32::from(convert), 0, mode]);
        let position = services.stock.remap(arguments).0;
        range[32..36].fill(0);
        let _ = services.records.write_shared(record, &shared::group_17::LENS_SNAPSHOT, range);
        position
    }
}

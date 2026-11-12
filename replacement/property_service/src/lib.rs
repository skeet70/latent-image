#![cfg_attr(all(not(test), not(property_embedded_storage)), no_std)]

#[cfg(not(property_embedded_storage))]
extern crate xm1_settings_storage_core as storage;
#[cfg(property_embedded_storage)]
use crate as storage;
mod property_table;
#[cfg(property_recipe_edit)]
mod recipe_edit;
#[cfg(not(test))]
pub(crate) mod storage_boundary;
use storage::table;
use property_table::{PROPERTIES, SLOTS, SPECIALS};
use storage::{Field, SlotField, FieldValue, HeaderField, MirrorSession, PreviousUpdate, HeaderBackend, SettingsConfiguration, SettingsStorage, SlotDefaultsWindow, TailDefaults};
#[cfg(not(test))]
use storage::SettingsBackend;
pub use storage::{ProfileSelection, SettingsBank, WriteValue};
use table::{BankDefault, BankOffset, Bits, FieldDefault, Hook, HookedField, Kind, ProfileSelector, Save, SlotDefault, SlotIndex, SlotRecord, Source, SpecialField, SpecialIndex, StockDefault, WriteOnlyPair};
#[cfg(not(test))]
use storage::native_configuration::NativeConfiguration;
#[cfg(not(test))]
use xm1_kernel::{Kernel, SlotTable};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PropertyId(u32);
impl PropertyId {
    pub fn decode(raw: u32) -> Option<Self> { if raw.wrapping_sub(1) <= 644 { Some(Self(raw)) } else { None } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValueBits(u32);
impl ValueBits {
    pub const fn new(value: u32) -> Self { Self(value) }
    pub const fn get(self) -> u32 { self.0 }
}
impl FieldValue for ValueBits {
    fn from_bits(value: u32) -> Self { Self(value) }
    fn bits(self) -> u32 { self.0 }
}
#[derive(Clone, Copy)]
pub enum ReadMode { Live, Default }
#[derive(Clone, Copy)]
enum Location<'a> { Bank(Field<ValueBits>), Slot(&'a SlotField<ValueBits>, ProfileSelection) }
struct Target<'a> { bank: SettingsBank, location: Location<'a>, update: PreviousUpdate }
trait FieldWrites { fn write_target(&mut self, target: &Target<'_>, value: ValueBits, update: PreviousUpdate); }
impl<B> FieldWrites for SettingsStorage<'_, B> {
    #[inline(never)]
    fn write_target(&mut self, target: &Target<'_>, value: ValueBits, update: PreviousUpdate) {
        match target.location {
            Location::Bank(field) => self.write(target.bank, field, value, update),
            Location::Slot(field, selection) => self.write_slot(target.bank, selection, field, value, update),
        }
    }
}
impl<B: HeaderBackend> FieldWrites for MirrorSession<'_, '_, B> {
    #[inline(never)]
    fn write_target(&mut self, target: &Target<'_>, value: ValueBits, update: PreviousUpdate) {
        let Location::Bank(field) = target.location else { panic!("slot in header session") };
        assert!(target.bank == SettingsBank::Active);
        self.write(HeaderField::try_from(field).ok().unwrap(), value, update);
    }
}

#[inline(always)]
fn entry(id: PropertyId) -> Option<Kind> {
    let index = if cfg!(property_defect_index_without_subtract) { id.0 } else { id.0 - 1 };
    PROPERTIES.get(index as usize)?.kind()
}
#[inline(always)]
fn slot_record(index: SlotIndex) -> Option<SlotRecord> { SLOTS.get(index.0 as usize)?.record() }
#[inline(always)]
fn special(index: SpecialIndex) -> Option<SpecialField> { SPECIALS.get(index.0 as usize)?.field() }
#[inline(always)]
fn field(offset: BankOffset, bits: Bits) -> Field<ValueBits> { Field::from_bank_layout(offset, bits).unwrap() }
#[inline(always)]
fn slot_field(current: Field<ValueBits>, record: SlotRecord) -> SlotField<ValueBits> {
    SlotField::from_layout(current, record.offset, record.bits).unwrap()
}
#[inline(always)]
fn previous_update(bits: Bits, save: Save) -> PreviousUpdate {
    match save {
        Save::None => PreviousUpdate::None,
        Save::Bank0Previous if bits.whole() => PreviousUpdate::WholeContainer,
        Save::Bank0Previous => PreviousUpdate::FieldBits,
    }
}
#[inline(always)]
fn target<'a>(bank: SettingsBank, location: Location<'a>, bits: Bits, save: Save) -> Target<'a> {
    Target { bank, location, update: previous_update(bits, save) }
}

#[inline(always)]
fn slot_target<'a>(bank: SettingsBank, descriptor: &'a SlotField<ValueBits>, selection: ProfileSelection, current_bits: Bits, record: &SlotRecord) -> Target<'a> {
    target(bank, Location::Slot(descriptor, selection), if selection == ProfileSelection::Current { current_bits } else { record.bits }, record.save)
}

#[cfg(any(storage_read_before_open, storage_defect_read_bank_before_refresh))]
#[inline(never)]
fn restore_stale_header<B: HeaderBackend>(session: &mut MirrorSession<'_, '_, B>, field: Field<ValueBits>, value: ValueBits) {
    session.write(HeaderField::try_from(field).ok().unwrap(), value, PreviousUpdate::None);
}
fn bank_default(default: BankDefault) -> u32 {
    match default {
        BankDefault::Constant(value) => value,
        BankDefault::TailByte(offset) => {
            let offset = if cfg!(storage_tail_defaults_moved) { table::TailOffset(if offset.0 == 39 { 0 } else { offset.0 + 1 }) } else { offset };
            u32::from(TailDefaults::byte(offset).unwrap())
        }
    }
}
fn field_default(default: FieldDefault) -> u32 {
    match default {
        FieldDefault::Constant(value) => value,
        FieldDefault::TailByte(offset) => u32::from(TailDefaults::byte(offset).unwrap()),
        FieldDefault::TailHalf(offset) => {
            let offset = if cfg!(storage_tail16_moved) { table::TailOffset(offset.0 + 4) } else { offset };
            let value = u32::from(TailDefaults::halfword(offset).unwrap());
            if cfg!(storage_tail16_byte) || cfg!(storage_defect_halfword_default_truncated) { value & 0xff } else { value }
        }
    }
}
fn stock_default<C: SettingsConfiguration>(configuration: &C, stock: StockDefault) -> u32 {
    use storage::DefaultProfile;
    match stock {
        StockDefault::Stock61df44 => configuration.default_61df44(),
        StockDefault::Stock61d8a4 => configuration.default_61d8a4(),
        StockDefault::Stock61d59c => configuration.default_61d59c(),
        StockDefault::Stock61d618 => configuration.default_61d618(),
        StockDefault::Stock61d5f0 => configuration.default_61d5f0(),
        StockDefault::Stock61d65c(profile) => configuration.default_61d65c(match profile { ProfileSelector::Two => DefaultProfile::Two, ProfileSelector::Three => DefaultProfile::Three, ProfileSelector::Four => DefaultProfile::Four }),
    }.0
}
fn config_default<C: SettingsConfiguration>(configuration: &C, bits: Bits) -> u32 {
    let byte = u32::from(configuration.network_name_default().0);
    if cfg!(property_defect_mask_config_default) { (byte >> bits.lsb()) & bits.mask() } else { byte }
}
#[inline(never)]
fn write_stock_default<W: FieldWrites, C: SettingsConfiguration>(writer: &mut W, configuration: &C, target: &Target<'_>, provisional: u32, stock: StockDefault) {
    if !cfg!(property_defect_drop_provisional) { writer.write_target(target, ValueBits(provisional), target.update); }
    let value = stock_default(configuration, stock);
    let update = if cfg!(property_defect_drop_second_save) { PreviousUpdate::None } else { target.update };
    writer.write_target(target, ValueBits(value), update);
}
#[inline(always)]
fn apply_hooked<W: FieldWrites, C: SettingsConfiguration>(writer: &mut W, configuration: &C, field: HookedField, value: WriteValue<ValueBits>, bank: SettingsBank) {
    let target = target(bank, Location::Bank(self::field(field.offset, field.bits)), field.bits, field.save);
    let stored = match (value, field.source) {
        (WriteValue::Value(value), _) => value.0,
        (WriteValue::Default, Source::Constant(value)) => value,
        (WriteValue::Default, Source::Stock(stock, provisional)) => return write_stock_default(writer, configuration, &target, provisional, stock),
        (WriteValue::Default, Source::Config(_, provisional)) => {
            writer.write_target(&target, ValueBits(provisional), target.update);
            let update = if cfg!(property_defect_config_save) { target.update } else { PreviousUpdate::None };
            writer.write_target(&target, ValueBits(u32::from(configuration.network_name_default().0)), update);
            return;
        }
    };
    writer.write_target(&target, ValueBits(stored), target.update);
    if let Hook::GatedResave(_) = field.hook {
        if (configuration.startup_resave_policy() == storage::StartupResavePolicy::Resave) != cfg!(property_defect_invert_gate) {
            writer.write_target(&target, ValueBits(stored), target.update);
        }
    }
}
#[inline(never)]
fn write_pair<W: FieldWrites>(writer: &mut W, pair: WriteOnlyPair, offset: BankOffset, value: WriteValue<ValueBits>, bank: SettingsBank) {
    let value = match value { WriteValue::Value(value) => value, WriteValue::Default => ValueBits(pair.default) };
    let target = target(bank, Location::Bank(field(offset, pair.bits)), pair.bits, pair.save);
    writer.write_target(&target, value, target.update);
}
fn get<B, C: SettingsConfiguration>(storage: &SettingsStorage<'_, B>, configuration: &C, id: PropertyId, bank: SettingsBank, mode: ReadMode, selection: ProfileSelection) -> u32 {
    match entry(id) {
        Some(Kind::Field(layout)) => match mode {
            ReadMode::Default => field_default(layout.default),
            ReadMode::Live => storage.read(bank, field(layout.offset, layout.bits)).0,
        },
        Some(Kind::SlotField(layout)) => {
            let selection = swapped_selection(selection);
            let Some(record) = slot_record(layout.index) else { return 0 };
            let selection = if layout.mirror { ProfileSelection::Record0 } else { selection };
            let descriptor = slot_field(field(layout.offset, layout.bits), record);
            match (selection, mode, record.default) {
                (_, ReadMode::Default, SlotDefault::Stock(stock)) => stock_default(configuration, stock),
                (ProfileSelection::Current, ReadMode::Default, SlotDefault::Record) => bank_default(record.bank_default),
                (_, ReadMode::Default, SlotDefault::Record) => SlotDefaultsWindow::read(selection, &descriptor).0,
                (_, ReadMode::Live, _) => storage.read_slot(bank, selection, &descriptor).0,
            }
        }
        Some(Kind::Special(index)) => {
            let (bits, offset, source) = match special(index) {
                Some(SpecialField::Hooked(field)) => (field.bits, field.offset, field.source),
                Some(SpecialField::ReadOnly(field)) => (field.bits, field.offset, Source::Constant(field.default)),
                Some(SpecialField::WriteOnlyPair(_)) | None => return 0,
            };
            match mode {
                ReadMode::Live => storage.read(bank, field(offset, bits)).0,
                ReadMode::Default => match source {
                    Source::Constant(value) => value,
                    Source::Stock(stock, _) => stock_default(configuration, stock),
                    Source::Config(_, _) => config_default(configuration, bits),
                },
            }
        }
        Some(Kind::Unassigned) | None => 0,
    }
}
/// What `set` did, without the driver report: the caller decides where that report lives.
#[derive(Clone, Copy, PartialEq, Eq)]
struct WriteSummary { assignment: PropertyWriteOutcome, #[cfg(test)] mirrored: bool }
impl WriteSummary {
    const ASSIGNED: Self = Self { assignment: PropertyWriteOutcome::Assigned, #[cfg(test)] mirrored: false };
    const UNASSIGNED: Self = Self { assignment: PropertyWriteOutcome::Unassigned, #[cfg(test)] mirrored: false };
    const MIRRORED: Self = Self { assignment: PropertyWriteOutcome::Assigned, #[cfg(test)] mirrored: true };
}

#[inline(always)]
fn set<B: HeaderBackend, C: SettingsConfiguration, M: ReportDestination>(storage: &mut SettingsStorage<'_, B>, configuration: &C, id: PropertyId, value: WriteValue<ValueBits>, bank: SettingsBank, selection: ProfileSelection, report: &mut M) -> WriteSummary {
    match entry(id) {
        Some(Kind::Field(layout)) => set_field(storage, layout, value, bank),
        Some(Kind::SlotField(layout)) => set_slot(storage, configuration, layout, value, bank, selection),
        Some(Kind::Special(index)) => set_special(storage, configuration, index, value, bank, report),
        Some(Kind::Unassigned) | None => WriteSummary::UNASSIGNED,
    }
}

#[inline(never)]
fn set_field<B: HeaderBackend>(storage: &mut SettingsStorage<'_, B>, layout: table::Field, value: WriteValue<ValueBits>, bank: SettingsBank) -> WriteSummary {
    let value = match value { WriteValue::Default => ValueBits(field_default(layout.default)), WriteValue::Value(value) => value };
    let target = target(bank, Location::Bank(field(layout.offset, layout.bits)), layout.bits, layout.save);
    storage.write_target(&target, value, target.update);
    WriteSummary::ASSIGNED
}

#[inline(never)]
fn set_slot<B: HeaderBackend, C: SettingsConfiguration>(storage: &mut SettingsStorage<'_, B>, configuration: &C, layout: table::SlotField, value: WriteValue<ValueBits>, bank: SettingsBank, selection: ProfileSelection) -> WriteSummary {
    let selection = swapped_selection(selection);
    let Some(record) = slot_record(layout.index) else { return WriteSummary::UNASSIGNED };
    let descriptor = slot_field(field(layout.offset, layout.bits), record);
    let target = slot_target(bank, &descriptor, selection, layout.bits, &record);
    match value {
        WriteValue::Value(value) => storage.write_target(&target, value, target.update),
        WriteValue::Default => {
            let provisional = if selection == ProfileSelection::Current { bank_default(record.bank_default) } else { SlotDefaultsWindow::read(selection, &descriptor).0 };
            match record.default {
                SlotDefault::Record => storage.write_target(&target, ValueBits(provisional), target.update),
                SlotDefault::Stock(stock) => write_stock_default(storage, configuration, &target, provisional, stock),
            }
        }
    }
    if layout.mirror && !cfg!(storage_defect_e9_record0_mirror_omitted) {
        let stored = match value { WriteValue::Value(value) => value, WriteValue::Default => SlotDefaultsWindow::read(ProfileSelection::Record0, &descriptor) };
        let target = slot_target(bank, &descriptor, ProfileSelection::Record0, layout.bits, &record);
        storage.write_target(&target, stored, target.update);
    }
    WriteSummary::ASSIGNED
}

#[inline(never)]
fn set_special<B: HeaderBackend, C: SettingsConfiguration, M: ReportDestination>(storage: &mut SettingsStorage<'_, B>, configuration: &C, index: SpecialIndex, value: WriteValue<ValueBits>, bank: SettingsBank, report: &mut M) -> WriteSummary {
    match special(index) {
        Some(SpecialField::Hooked(layout)) => set_hooked(storage, configuration, layout, value, bank, report),
        Some(SpecialField::WriteOnlyPair(pair)) => set_pair(storage, pair, value, bank, report),
        Some(SpecialField::ReadOnly(_)) | None => WriteSummary::UNASSIGNED,
    }
}

#[inline(never)]
fn set_hooked<B: HeaderBackend, C: SettingsConfiguration, M: ReportDestination>(storage: &mut SettingsStorage<'_, B>, configuration: &C, layout: HookedField, value: WriteValue<ValueBits>, bank: SettingsBank, report: &mut M) -> WriteSummary {
    let bracket = matches!(layout.hook, Hook::Bank0Bracket) && (bank == SettingsBank::Active || cfg!(storage_bracket_bank1) || cfg!(storage_defect_bracket_bank1));
    if bracket && !cfg!(storage_open_removed) {
        #[cfg(any(storage_read_before_open, storage_defect_read_bank_before_refresh))]
        let stale_field = field(layout.offset, layout.bits);
        #[cfg(any(storage_read_before_open, storage_defect_read_bank_before_refresh))]
        let stale = storage.read(bank, stale_field);
        let mut session = storage.open_header();
        report.opened(Some(session.opened));
        #[cfg(any(storage_read_before_open, storage_defect_read_bank_before_refresh))]
        restore_stale_header(&mut session, stale_field, stale);
        if cfg!(storage_close_before_set) || cfg!(storage_defect_close_before_field_store) {
            report.close(session);
            apply_hooked(storage, configuration, layout, value, bank);
        } else {
            apply_hooked(&mut session, configuration, layout, value, SettingsBank::Active);
            report.close(session);
        }
        WriteSummary::MIRRORED
    } else {
        apply_hooked(storage, configuration, layout, value, bank);
        if bracket {
            report.opened(None);
            report.write_active(storage);
            WriteSummary::MIRRORED
        } else {
            WriteSummary::ASSIGNED
        }
    }
}

#[inline(never)]
fn set_pair<B: HeaderBackend, M: ReportDestination>(storage: &mut SettingsStorage<'_, B>, pair: WriteOnlyPair, value: WriteValue<ValueBits>, bank: SettingsBank, report: &mut M) -> WriteSummary {
    if bank == SettingsBank::Active || cfg!(storage_bracket_bank1) || cfg!(storage_defect_bracket_bank1) {
        if cfg!(storage_open_removed) {
            write_pair(storage, pair, pair.first, value, bank);
            report.opened(None);
            report.write_active(storage);
        } else {
            #[cfg(any(storage_read_before_open, storage_defect_read_bank_before_refresh))]
            let stale_field = field(pair.first, pair.bits);
            #[cfg(any(storage_read_before_open, storage_defect_read_bank_before_refresh))]
            let stale = storage.read(bank, stale_field);
            let mut session = storage.open_header();
            report.opened(Some(session.opened));
            #[cfg(any(storage_read_before_open, storage_defect_read_bank_before_refresh))]
            restore_stale_header(&mut session, stale_field, stale);
            if cfg!(storage_close_before_set) || cfg!(storage_defect_close_before_field_store) {
                report.close(session);
                write_pair(storage, pair, pair.first, value, bank);
            } else {
                write_pair(&mut session, pair, pair.first, value, SettingsBank::Active);
                #[cfg(any(storage_pair_before_close, storage_defect_pair_close_after_both_writes))]
                unsafe {
                    extern "C" { fn xm1_storage_store8(address: *mut u8, value: u32); }
                    let stored = match value { WriteValue::Default => pair.default, WriteValue::Value(value) => value.0 };
                    xm1_storage_store8((0x96cc64 + pair.second.0) as *mut u8, stored);
                }
                report.close(session);
            }
        }
        write_pair(storage, pair, pair.second, value, bank);
        WriteSummary::MIRRORED
    } else {
        write_pair(storage, pair, pair.first, value, bank);
        write_pair(storage, pair, pair.second, value, bank);
        WriteSummary::ASSIGNED
    }
}

fn swapped_selection(selection: ProfileSelection) -> ProfileSelection {
    if cfg!(storage_bank_slot_swap) {
        if selection == ProfileSelection::Current { ProfileSelection::Record0 } else { ProfileSelection::Current }
    } else { selection }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyWriteOutcome { Assigned, Unassigned }

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PropertyMirrorOutcome {
    pub opened: Option<storage::RequestOutcome>,
    pub closed: storage::MirrorOutcome,
}
#[cfg(test)]
impl PropertyMirrorOutcome {
    const NONE: Self = Self { opened: None, closed: storage::MirrorOutcome::unchanged() };
}
/// Where a property write's driver reports land.
trait ReportDestination {
    fn opened(&mut self, outcome: Option<storage::RequestOutcome>);
    fn close<B: HeaderBackend>(&mut self, session: MirrorSession<'_, '_, B>);
    fn write_active<B: HeaderBackend>(&mut self, storage: &mut SettingsStorage<'_, B>);
}
#[cfg(test)]
impl ReportDestination for PropertyMirrorOutcome {
    #[inline(always)]
    fn opened(&mut self, outcome: Option<storage::RequestOutcome>) { self.opened = outcome; }
    #[inline(always)]
    fn close<B: HeaderBackend>(&mut self, session: MirrorSession<'_, '_, B>) { session.close_into(&mut self.closed); }
    #[inline(always)]
    fn write_active<B: HeaderBackend>(&mut self, storage: &mut SettingsStorage<'_, B>) { storage.write_active_header_into(&mut self.closed); }
}
/// The device paths keep none of a write's driver reports.
#[cfg(not(test))]
struct Discarded;
#[cfg(not(test))]
impl ReportDestination for Discarded {
    #[inline(always)]
    fn opened(&mut self, _: Option<storage::RequestOutcome>) {}
    #[inline(always)]
    fn close<B: HeaderBackend>(&mut self, session: MirrorSession<'_, '_, B>) { session.close_discarding(); }
    #[inline(always)]
    fn write_active<B: HeaderBackend>(&mut self, storage: &mut SettingsStorage<'_, B>) { storage.write_active_header_discarding(); }
}
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PropertyWriteResult {
    pub assignment: PropertyWriteOutcome,
    pub mirror: Option<PropertyMirrorOutcome>,
}

pub fn read_property<B, C: SettingsConfiguration>(storage: &SettingsStorage<'_, B>, configuration: &C, id: PropertyId, bank: SettingsBank, mode: ReadMode, selection: ProfileSelection) -> ValueBits {
    ValueBits(get(storage, configuration, id, bank, mode, selection))
}
#[cfg(test)]
pub fn write_property<B: HeaderBackend, C: SettingsConfiguration>(storage: &mut SettingsStorage<'_, B>, configuration: &C, id: PropertyId, value: WriteValue<ValueBits>, bank: SettingsBank, selection: ProfileSelection) -> PropertyWriteResult {
    let mut mirror = PropertyMirrorOutcome::NONE;
    let summary = set(storage, configuration, id, value, bank, selection, &mut mirror);
    PropertyWriteResult { assignment: summary.assignment, mirror: if summary.mirrored { Some(mirror) } else { None } }
}

/// The device paths keep none of a write's driver reports.
#[cfg(not(test))]
#[inline(always)]
fn write_discarding<B: SettingsBackend, C: SettingsConfiguration>(storage: &mut SettingsStorage<'_, B>, configuration: &C, id: PropertyId, value: WriteValue<ValueBits>, bank: SettingsBank, selection: ProfileSelection) -> PropertyWriteOutcome {
    #[cfg(property_reload)]
    if bank == SettingsBank::Active {
        storage.refresh_active_body();
    }
    let summary = set(storage, configuration, id, value, bank, selection, &mut Discarded);
    #[cfg(property_reload)]
    if bank == SettingsBank::Active && summary.assignment == PropertyWriteOutcome::Assigned {
        storage.write_active_body_discarding();
    }
    summary.assignment
}

/// The wrappers' bodies over the owned service: slot `0x6439e0`, bank-0 body reload and write-back.
#[cfg(not(test))]
pub struct Properties {
    kernel: Kernel<SlotTable>,
}

#[cfg(not(test))]
impl Properties {
    /// # Safety
    /// The X-M1 settings windows and the retained configuration callbacks must be mapped and exclusively accessible while this value exists.
    pub unsafe fn stock() -> Self {
        Self { kernel: unsafe { Kernel::<SlotTable>::stock() } }
    }

    pub fn read(&self, id: PropertyId, bank: SettingsBank, mode: ReadMode) -> ValueBits {
        let configuration = unsafe { NativeConfiguration::adopt() };
        let selection = storage_boundary::current_selection();
        unsafe { storage_boundary::with_storage(&self.kernel, |storage| read_property(storage, &configuration, id, bank, mode, selection)) }
    }

    pub fn write(&mut self, id: PropertyId, value: WriteValue<ValueBits>, bank: SettingsBank) -> PropertyWriteOutcome {
        let configuration = unsafe { NativeConfiguration::adopt() };
        let selection = storage_boundary::current_selection();
        unsafe { storage_boundary::with_storage(&self.kernel, |storage| write_discarding(storage, &configuration, id, value, bank, selection)) }
    }
}


#[cfg(not(test))]
/// # Safety
/// The X-M1 compatibility backing and retained configuration callbacks must be mapped and exclusively accessible for this call.
#[no_mangle]
pub unsafe extern "C" fn xm1_property_get(id: u32, bank: u32, defaults: u32, selection: u32) -> u32 {
    let Some(id) = PropertyId::decode(id) else { return rejected_id(id) };
    #[cfg(property_recipe_edit)]
    if let Some(field) = recipe_edit::RecipeEditProperty::parse(id.0) {
        return unsafe { field.stored_code() }.word();
    }
    let Some(bank) = SettingsBank::from_abi(bank) else { return 0 };
    let Some(selection) = ProfileSelection::from_abi(selection) else { return 0 };
    if cfg!(storage_defect_wrapped_selector_rejected) && selection == ProfileSelection::WrappedPredecessor { return 0; }
    let defaults = if cfg!(property_defect_nonzero_means_default) { defaults != 0 } else { defaults == 1 };
    let configuration = unsafe { NativeConfiguration::adopt() };
    let kernel = unsafe { Kernel::<SlotTable>::stock() };
    unsafe { storage_boundary::with_storage(&kernel, |storage| {
        #[cfg(storage_defect_handover_reinitialize_all_banks)]
        storage.clear_images();
        get(storage, &configuration, id, bank, if defaults { ReadMode::Default } else { ReadMode::Live }, selection)
    }) }
}
#[cfg(not(test))]
/// # Safety
/// The requirements of `xm1_property_get` apply.
#[no_mangle]
pub unsafe extern "C" fn xm1_property_set(id: u32, value: u32, bank: u32, selection: u32) -> u32 {
    let Some(id) = PropertyId::decode(id) else { return rejected_id(id) };
    #[cfg(property_recipe_edit)]
    if recipe_edit::RecipeEditProperty::parse(id.0).is_some() {
        return 0;
    }
    let Some(bank) = SettingsBank::from_abi(bank) else { return 0 };
    let Some(selection) = ProfileSelection::from_abi(selection) else { return 0 };
    if cfg!(storage_defect_wrapped_selector_rejected) && selection == ProfileSelection::WrappedPredecessor { return 0; }
    let value = if cfg!(property_defect_sentinel_plus_one) && value == 0x1b000000 { WriteValue::Default } else if cfg!(property_defect_sentinel_plus_one) { WriteValue::Value(ValueBits(value)) } else { WriteValue::from_abi(value) };
    let configuration = unsafe { NativeConfiguration::adopt() };
    let kernel = unsafe { Kernel::<SlotTable>::stock() };
    let result = unsafe { storage_boundary::with_storage(&kernel, |storage| {
        if cfg!(storage_defect_snapshot_entire_bank_per_set) && bank == SettingsBank::Active { storage.capture_active(); }
        let result = set(storage, &configuration, id, value, bank, selection, &mut Discarded).assignment;
        #[cfg(storage_defect_save_on_bank1)]
        if bank == SettingsBank::Alternate { storage.capture_alternate(); }
        if cfg!(storage_defect_save_after_final_store) && bank == SettingsBank::Active { storage.capture_active(); }
        result
    }) };
    match result { PropertyWriteOutcome::Assigned => 1, PropertyWriteOutcome::Unassigned => 0 }
}
#[cfg(not(test))]
fn rejected_id(raw: u32) -> u32 {
    let index = raw.wrapping_sub(1);
    if cfg!(property_defect_signed_bound) && (index as i32) <= 644 || cfg!(property_defect_upper_bound_plus_one) && index == 645 { 1 } else { 0 }
}
#[cfg(all(not(test), not(feature = "component"), not(property_rlib)))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }

#[cfg(test)]
mod tests;

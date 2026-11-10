#![cfg_attr(not(test), no_std)]
#![cfg_attr(settings_custom_native, no_builtins)]

mod projection;
mod field;
mod defaults;
mod default_bytes;
mod memory;
pub mod mirror;
#[cfg(not(settings_storage_property))]
pub mod custom;
#[cfg(settings_custom_native)]
pub mod native_custom;
#[cfg(all(not(test), any(not(settings_storage_library), settings_bank_native)))]
pub mod native_bank;
#[cfg(all(not(test), settings_storage_firmware))]
pub mod native_configuration;
#[path = "../../property_service/src/table.rs"]
pub mod table;
#[path = "../../property_service/src/property_table.rs"]
mod property_table;

pub use projection::*;
pub use field::{Field, HeaderField, NotHeaderField, SlotField, FieldValue, PropertyValue, PreviousUpdate, WriteValue};
pub use defaults::*;
pub use mirror::{HeaderBackend, SettingsBackend, BodyOutcome, MirrorOutcome, RequestOutcome};

#[inline(always)]
pub(crate) fn part(bytes: &[u8], start: usize, length: usize) -> &[u8] {
    bytes.get(start..start + length).unwrap_or_else(|| contract_violation())
}
#[inline(always)]
pub(crate) fn part_mut(bytes: &mut [u8], start: usize, length: usize) -> &mut [u8] {
    bytes.get_mut(start..start + length).unwrap_or_else(|| contract_violation())
}
#[cold]
#[inline(never)]
pub(crate) fn contract_violation() -> ! {
    #[cfg(settings_storage_firmware)]
    loop { core::hint::spin_loop(); }
    #[cfg(not(settings_storage_firmware))]
    panic!("settings-storage buffer contract violated");
}

pub struct PreviousValues<'a> {
    bytes: *mut [u8; PREVIOUS_WINDOW],
    lifetime: core::marker::PhantomData<&'a mut [u8; PREVIOUS_WINDOW]>,
}
impl<'a> PreviousValues<'a> {
    #[inline(always)]
    pub fn adopt(bytes: &'a mut [u8; PREVIOUS_WINDOW]) -> Self { Self { bytes, lifetime: core::marker::PhantomData } }
    pub fn bytes(&self) -> &[u8; PREVIOUS_WINDOW] { unsafe { &*self.bytes } }
    fn bytes_mut(&mut self) -> &mut [u8; PREVIOUS_WINDOW] { unsafe { &mut *self.bytes } }
}

pub struct SettingsStorage<'a, B> {
    banks: &'a mut [u8; BANK_WINDOW],
    previous: PreviousValues<'a>,
    backend: B,
}
impl<'a, B> SettingsStorage<'a, B> {
    #[inline(always)]
    pub fn adopt(banks: &'a mut [u8; BANK_WINDOW], previous: PreviousValues<'a>, backend: B) -> Self { Self { banks, previous, backend } }
    pub fn bank(&self, bank: SettingsBank) -> BankImage<'_> {
        let offset = bank.offset();
        BankImage(self.banks.get(offset..offset+BANK_STRIDE).unwrap().try_into().ok().unwrap())
    }
    pub fn slot(&self, bank: SettingsBank, selection: ProfileSelection) -> SlotRecord<'_> {
        let offset = bank.offset()+selection.offset();
        SlotRecord(self.banks.get(offset..offset+40).unwrap().try_into().ok().unwrap())
    }
    #[inline(always)]
    pub fn read<V: FieldValue>(&self, bank: SettingsBank, field: Field<V>) -> V { V::from_bits(field.extract(field.load(self.banks, bank.offset()))) }
    pub fn read_previous<V: FieldValue>(&self, field: Field<V>) -> V { V::from_bits(field.extract(field.load(self.previous.bytes(), 0))) }
    #[inline(never)]
    pub fn write<V: FieldValue>(&mut self, bank: SettingsBank, field: Field<V>, value: V, update: PreviousUpdate) {
        self.write_at(bank, field, bank.offset(), 0, value, update);
    }
    #[inline(never)]
    fn write_at<V: FieldValue>(&mut self, bank: SettingsBank, field: Field<V>, live_base: usize, saved_base: usize, value: V, update: PreviousUpdate) {
        let old = field.load(self.banks, live_base);
        if bank == SettingsBank::Active {
            let saved = match update {
                PreviousUpdate::None => None,
                PreviousUpdate::WholeContainer => Some(old),
                PreviousUpdate::FieldBits => Some(field.insert(field.load(self.previous.bytes(), saved_base), field.extract(old))),
            };
            if let Some(saved) = saved { field.store(self.previous.bytes_mut(), saved_base, saved); }
        }
        field.store(self.banks, live_base, field.insert(old, value.bits()));
    }
    #[inline(always)]
    pub fn read_slot<V: FieldValue>(&self, bank: SettingsBank, selection: ProfileSelection, field: &SlotField<V>) -> V {
        if selection == ProfileSelection::Current { return self.read(bank, field.current); }
        let displaced = if cfg!(storage_defect_slot_projections_independent) { 0x40 } else { 0 };
        V::from_bits(field.record.extract(field.record.load(self.banks, bank.offset()+selection.offset()+displaced)))
    }
    #[inline(always)]
    pub fn write_slot<V: FieldValue>(&mut self, bank: SettingsBank, selection: ProfileSelection, field: &SlotField<V>, value: V, update: PreviousUpdate) {
        if selection == ProfileSelection::Current { self.write(bank, field.current, value, update); }
        else {
            let displaced = if cfg!(storage_defect_slot_projections_independent) { 0x40 } else { 0 };
            let saved_base = if cfg!(storage_record_save_at_bank) { field.current.offset.checked_sub(field.record.offset).unwrap() } else { selection.offset() };
            self.write_at(bank, field.record, bank.offset()+selection.offset()+displaced, saved_base, value, update);
        }
    }
    pub fn capture_active(&mut self) { memory::copy_bytes(part_mut(self.previous.bytes_mut(), 0, BANK_STRIDE), part(self.banks, 0, BANK_STRIDE)); }
    #[inline(always)]
    pub(crate) fn restore_active_image(&mut self) {
        memory::copy_bytes(part_mut(self.banks, 0, BANK_STRIDE), part(self.previous.bytes(), 0, BANK_STRIDE));
    }
    #[cfg(storage_defect_save_on_bank1)]
    pub fn capture_alternate(&mut self) { memory::copy_bytes(part_mut(self.previous.bytes_mut(), 0, BANK_STRIDE), part(self.banks, BANK_STRIDE, BANK_STRIDE)); }
    #[cfg(storage_defect_handover_reinitialize_all_banks)]
    pub fn clear_images(&mut self) {
        for byte in self.banks.iter_mut() { memory::write_byte(byte, 0); }
        for byte in self.previous.bytes_mut().iter_mut() { memory::write_byte(byte, 0); }
    }
    pub fn previous(&self) -> &PreviousValues<'a> { &self.previous }
    pub fn backend(&mut self) -> &mut B { &mut self.backend }
}
impl<'a, B: HeaderBackend> SettingsStorage<'a, B> {
    #[inline(always)]
    pub fn open_header(&mut self) -> MirrorSession<'_, 'a, B> {
        let opened = self.backend.read_header(part_mut(self.banks, 0, 16).try_into().unwrap_or_else(|_| contract_violation()));
        MirrorSession { storage: self, opened }
    }
    #[inline(always)]
    pub fn write_active_header(&mut self) -> MirrorOutcome {
        self.backend.write_header(part(self.banks, 0, 16).try_into().unwrap_or_else(|_| contract_violation()))
    }
}
impl<'a, B: SettingsBackend> SettingsStorage<'a, B> {
    pub fn restore_active_body(&mut self) -> BodyOutcome {
        self.restore_active_image();
        self.write_active_body()
    }
    pub fn refresh_active_body(&mut self) -> RequestOutcome { self.backend.read_body(part_mut(self.banks, 16, 0x118).try_into().unwrap_or_else(|_| contract_violation())) }
    pub fn write_active_body(&mut self) -> BodyOutcome { self.backend.write_body(part(self.banks, 16, 0x118).try_into().unwrap_or_else(|_| contract_violation())) }
    #[inline(always)]
    pub fn load_images(&mut self) -> ImageLoadOutcome {
        let mut requests = [RequestOutcome::success(); 9];
        for image in 0..3 {
            if cfg!(storage_defect_initialize_only_active_bank) && image != 0 { continue; }
            let destination = match image { 0 => part_mut(self.banks, 0, BANK_STRIDE), 1 => part_mut(self.banks, BANK_STRIDE, BANK_STRIDE), _ => part_mut(self.previous.bytes_mut(), 0, BANK_STRIDE) };
            requests[image] = self.backend.read_header(part_mut(destination, 0, 16).try_into().unwrap_or_else(|_| contract_violation()));
        }
        for image in 0..3 {
            if cfg!(storage_defect_initialize_only_active_bank) && image != 0 { continue; }
            let destination = match image { 0 => part_mut(self.banks, 0, BANK_STRIDE), 1 => part_mut(self.banks, BANK_STRIDE, BANK_STRIDE), _ => part_mut(self.previous.bytes_mut(), 0, BANK_STRIDE) };
            requests[3+image] = self.backend.read_body(part_mut(destination, 16, 0x118).try_into().unwrap_or_else(|_| contract_violation()));
        }
        for image in 0..3 {
            if cfg!(storage_defect_initialize_only_active_bank) && image != 0 { continue; }
            let destination = match image { 0 => part_mut(self.banks, 0, BANK_STRIDE), 1 => part_mut(self.banks, BANK_STRIDE, BANK_STRIDE), _ => part_mut(self.previous.bytes_mut(), 0, BANK_STRIDE) };
            requests[6+image] = self.backend.read_custom(part_mut(destination, 0x140, 40).try_into().unwrap_or_else(|_| contract_violation()));
        }
        for image in 0..3 {
            if cfg!(storage_defect_initialize_only_active_bank) && image != 0 { continue; }
            let destination = match image { 0 => part_mut(self.banks, 0, BANK_STRIDE), 1 => part_mut(self.banks, BANK_STRIDE, BANK_STRIDE), _ => part_mut(self.previous.bytes_mut(), 0, BANK_STRIDE) };
            if cfg!(storage_defect_tail_loaded_from_record) {
                let payload: [u8;40] = part(destination, 0x140, 40).try_into().unwrap_or_else(|_| contract_violation());
                memory::copy_bytes(part_mut(destination, 0x19a, 40), &payload);
            } else {
                memory::copy_bytes(part_mut(destination, 0x19a, 40), TailDefaults::tail().bytes());
            }
            if cfg!(storage_defect_zero_loader_gaps) {
                for byte in part_mut(destination, 0x128, 24) { memory::write_byte(byte, 0); }
                for byte in part_mut(destination, 0x168, 50) { memory::write_byte(byte, 0); }
            }
            if cfg!(storage_defect_load_whole_bank_from_area3) {
                let payload: [u8;40] = part(destination, 0x140, 40).try_into().unwrap_or_else(|_| contract_violation());
                for (byte, value) in destination.iter_mut().zip(payload.iter().cycle()) { memory::write_byte(byte, *value); }
            }
        }
        ImageLoadOutcome { requests }
    }
    pub fn initialize<C: SettingsConfiguration>(&mut self, configuration: &C, mut startup_resave: impl FnMut(&mut Self)) -> InitializationOutcome {
        let loaded = self.load_images();
        let resaved = !cfg!(storage_defect_skip_startup_resave) && configuration.startup_resave_policy() == StartupResavePolicy::Resave;
        if resaved { startup_resave(self); }
        InitializationOutcome { requests: loaded.requests, startup_resaved: resaved }
    }
    pub fn commit(&mut self, target: CommitTarget) -> CommitOutcome {
        match target {
            CommitTarget::HeaderMirror => CommitOutcome::HeaderSynchronized(self.write_active_header()),
            CommitTarget::ActiveBody => CommitOutcome::BodyWritten(self.write_active_body()),
        }
    }
    pub fn exchange(&mut self, direction: BankExchange) -> ExchangeOutcome {
        let (active, remaining) = self.banks.split_at_mut(BANK_STRIDE);
        let alternate = part_mut(remaining, 0, BANK_STRIDE);
        match direction {
            BankExchange::ActiveToAlternate => {
                memory::copy_bytes(alternate, active);
                ExchangeOutcome::CopiedToAlternate
            }
            BankExchange::AlternateToActive => {
                memory::copy_bytes(active, alternate);
                ExchangeOutcome::ActiveRestored(self.write_active_body())
            }
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageLoadOutcome { pub requests: [RequestOutcome;9] }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BankExchange { ActiveToAlternate, AlternateToActive }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExchangeOutcome { CopiedToAlternate, ActiveRestored(BodyOutcome) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InitializationOutcome { pub requests: [RequestOutcome;9], pub startup_resaved: bool }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitTarget { HeaderMirror, ActiveBody }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitOutcome { Unchanged, HeaderSynchronized(MirrorOutcome), BodyWritten(BodyOutcome) }

pub struct MirrorSession<'s, 'a, B: HeaderBackend> { storage: &'s mut SettingsStorage<'a, B>, pub opened: RequestOutcome }
impl<B: HeaderBackend> MirrorSession<'_, '_, B> {
    pub fn read<V: FieldValue>(&self, field: HeaderField<V>) -> V { self.storage.read(SettingsBank::Active, field.0) }
    #[inline(always)]
    pub fn write<V: FieldValue>(&mut self, field: HeaderField<V>, value: V, update: PreviousUpdate) { self.storage.write(SettingsBank::Active, field.0, value, update); }
    #[inline(always)]
    pub fn close(self) -> MirrorOutcome {
        let outcome = self.storage.write_active_header();
        #[cfg(all(not(test), storage_defect_checksum_written_into_bank))]
        memory::write_byte(&mut self.storage.banks[15], unsafe { core::ptr::read_volatile(0x409c282b as *const u8) });
        #[cfg(all(not(test), storage_defect_close_appends_area3))]
        unsafe { native_mirror::append_bank_as_record(self.storage.banks.as_ptr()); }
        outcome
    }
}

#[cfg(test)] mod tests;
#[cfg(all(not(test), not(settings_storage_library)))] mod native;
#[cfg(all(not(test), any(not(settings_storage_library), settings_storage_firmware)))] pub mod native_mirror;

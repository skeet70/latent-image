//! A test world for the engine: profile records in plain bytes, configuration values, lens and
//! capture-profile services by table, and stock memory as an address map that keeps its stores.

use std::collections::BTreeMap;
use std::vec::Vec;

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::field::{RecordField, RecordRead, RecordWrite, SharedField, SharedRead, SharedWrite, Value};
use xm1_saclac_profile_records::stock::layout;
use xm1_saclac_profile_records::{AccessGroup, Bytes, Grants, Handle, MeasurementSelection, OpenMode, ProfileRecords, RecordImage, Scope, Serial, SharedHeaderByte, Slot, StockEntries, Tag};

use crate::exposure::ports::{CaptureProfiles, Lens, ProfileAccess, Properties, Property, StockMemory};
use crate::profile::Profile;

const SLOT: u8 = 2;
const SERIAL: u16 = 0x31;

#[derive(Default)]
pub struct Headers {
    pub bytes: BTreeMap<u8, u8>,
    pub selection: MeasurementSelection,
}

fn header_key(byte: SharedHeaderByte) -> u8 {
    match byte {
        SharedHeaderByte::At31 => 0x31,
        SharedHeaderByte::At46 => 0x46,
        SharedHeaderByte::At47 => 0x47,
        SharedHeaderByte::At61 => 0x61,
        SharedHeaderByte::At63 => 0x63,
    }
}

impl StockEntries for Headers {
    fn measurement_selection(&mut self, _handle: RecordHandle) -> MeasurementSelection { self.selection }
    fn header_byte(&mut self, _handle: RecordHandle, byte: SharedHeaderByte) -> u8 { self.bytes.get(&header_key(byte)).copied().unwrap_or(0) }
    fn lens_designation(&mut self, _handle: RecordHandle) -> [u8; 28] { [0; 28] }
    fn replace_header(&mut self, _handle: RecordHandle, _staged: &RecordImage) {}
    fn open(&mut self, _mode: OpenMode, _grants: Grants) -> RecordHandle { handle() }
    fn close(&mut self, _handle: RecordHandle) {}
    fn initialize(&mut self) {}
}

/// The handle every test record is bound through: its slot names the first record and grants
/// every group.
pub fn handle() -> RecordHandle {
    xm1_saclac_profile_records::stock::record_handle(Handle {
        slot: Slot::new(SLOT).unwrap(),
        serial: Serial::new(SERIAL).unwrap(),
        owner: true,
        writable: true,
        scope: Scope::ALL,
        tag: Tag(0),
    })
}

fn granted_records() -> ProfileRecords<Bytes> {
    let mut bytes = Bytes::default();
    let slot = Slot::new(SLOT).unwrap();
    let put = |bytes: &mut Bytes, at: u16, word: u32| bytes.registry[at as usize..at as usize + 4].copy_from_slice(&word.to_le_bytes());
    for other in 0..16 {
        put(&mut bytes, layout::slot_serial(Slot::new(other).unwrap()), u32::MAX);
    }
    put(&mut bytes, layout::slot_serial(slot), u32::from(SERIAL));
    put(&mut bytes, layout::slot_record(slot), 0);
    for group in 0..43 {
        bytes.registry[layout::slot_grant(slot, AccessGroup::new(group).unwrap()) as usize] = layout::GRANT_READ | layout::GRANT_WRITE;
    }
    ProfileRecords::new(bytes)
}

pub struct World {
    pub records: ProfileRecords<Bytes>,
    pub headers: Headers,
    pub properties: BTreeMap<u32, u32>,
    pub lens_levels: BTreeMap<(u32, u32), i16>,
    pub lens_base_levels: BTreeMap<u32, i16>,
    pub reference_offsets: BTreeMap<u32, i16>,
    pub calibration_indices: BTreeMap<u32, u32>,
    pub level_offsets: BTreeMap<u32, i16>,
    pub class_offsets: BTreeMap<u8, i16>,
    pub offset_classes: BTreeMap<u32, u8>,
    pub sensitivity_classes: BTreeMap<u32, u8>,
    pub configured_level: u16,
    pub configured_profiles: BTreeMap<i16, u32>,
    pub memory: BTreeMap<u32, u8>,
    /// Every store, in order: address, width in bytes, value.
    pub stores: Vec<(u32, u8, u32)>,
}

impl Default for World {
    fn default() -> Self {
        Self {
            records: granted_records(),
            headers: Headers::default(),
            properties: BTreeMap::new(),
            lens_levels: BTreeMap::new(),
            lens_base_levels: BTreeMap::new(),
            reference_offsets: BTreeMap::new(),
            calibration_indices: BTreeMap::new(),
            level_offsets: BTreeMap::new(),
            class_offsets: BTreeMap::new(),
            offset_classes: BTreeMap::new(),
            sensitivity_classes: BTreeMap::new(),
            configured_level: 0,
            configured_profiles: BTreeMap::new(),
            memory: BTreeMap::new(),
            stores: Vec::new(),
        }
    }
}

impl World {
    pub fn set<T: Value>(&mut self, field: &RecordField<T, RecordRead, RecordWrite>, value: T) { self.records.write(handle(), field, value).unwrap(); }

    pub fn set_shared<T: Value>(&mut self, field: &SharedField<T, SharedRead, SharedWrite>, value: T) {
        self.records.write_shared(handle(), field, value).unwrap();
    }

    pub fn header(&mut self, byte: SharedHeaderByte, value: u8) { self.headers.bytes.insert(header_key(byte), value); }

    pub fn select(&mut self, selection: MeasurementSelection) { self.headers.selection = selection; }

    pub fn property(&mut self, id: Property, value: u32) { self.properties.insert(id.0, value); }

    /// A two-byte configuration value, low byte first.
    pub fn property_word(&mut self, id: Property, value: u16) {
        self.property(id, u32::from(value as u8));
        self.property(id.high(), u32::from(value >> 8));
    }

    pub fn put_u16(&mut self, at: SharedAddress, value: u16) {
        for (index, byte) in value.to_le_bytes().into_iter().enumerate() {
            self.memory.insert(at.0 + index as u32, byte);
        }
    }

    fn byte(&self, at: u32) -> u8 { self.memory.get(&at).copied().unwrap_or(0) }

    fn put(&mut self, at: SharedAddress, bytes: &[u8]) {
        for (index, byte) in bytes.iter().enumerate() {
            self.memory.insert(at.0 + index as u32, *byte);
        }
    }

    pub fn u16_at(&self, at: SharedAddress) -> u16 { u16::from_le_bytes([self.byte(at.0), self.byte(at.0 + 1)]) }

    pub fn u32_at(&self, at: SharedAddress) -> u32 {
        u32::from_le_bytes([self.byte(at.0), self.byte(at.0 + 1), self.byte(at.0 + 2), self.byte(at.0 + 3)])
    }
}

impl Properties for World {
    fn property(&mut self, property: Property) -> u32 { self.properties.get(&property.0).copied().unwrap_or(0) }
}

impl Lens for World {
    fn level(&mut self, lens: u32, reference: u32) -> i16 { self.lens_levels.get(&(lens, reference)).copied().unwrap_or(0) }
    fn base_level(&mut self, lens: u32) -> i16 { self.lens_base_levels.get(&lens).copied().unwrap_or(0) }
    fn reference_offset(&mut self, reference: u32) -> i16 { self.reference_offsets.get(&reference).copied().unwrap_or(0) }
    fn calibration_index(&mut self, lens: u32) -> u32 { self.calibration_indices.get(&lens).copied().unwrap_or(0) }
}

impl CaptureProfiles for World {
    fn level_offset(&mut self, profile: u32) -> i16 { self.level_offsets.get(&profile).copied().unwrap_or(0) }
    fn class_offset(&mut self, class: u8) -> i16 { self.class_offsets.get(&class).copied().unwrap_or(0) }
    fn offset_class(&mut self, profile: u32) -> u8 { self.offset_classes.get(&profile).copied().unwrap_or(0) }
    fn sensitivity_class(&mut self, profile: u32) -> u8 { self.sensitivity_classes.get(&profile).copied().unwrap_or(0) }
    fn configured_level(&mut self) -> u16 { self.configured_level }
    fn configured_profile(&mut self, level: i16) -> u32 { self.configured_profiles.get(&level).copied().unwrap_or(0) }
    fn record_profile(&mut self) -> u32 { 1 }
}

impl StockMemory for World {
    fn read_u8(&mut self, at: SharedAddress) -> u8 { self.byte(at.0) }
    fn read_u16(&mut self, at: SharedAddress) -> u16 { self.u16_at(at) }
    fn read_u32(&mut self, at: SharedAddress) -> u32 { self.u32_at(at) }

    fn write_u8(&mut self, at: SharedAddress, value: u8) {
        self.stores.push((at.0, 1, u32::from(value)));
        self.put(at, &[value]);
    }

    fn write_u16(&mut self, at: SharedAddress, value: u16) {
        self.stores.push((at.0, 2, u32::from(value)));
        self.put(at, &value.to_le_bytes());
    }

    fn write_u32(&mut self, at: SharedAddress, value: u32) {
        self.stores.push((at.0, 4, value));
        self.put(at, &value.to_le_bytes());
    }
}

impl ProfileAccess for World {
    type Records = Bytes;
    type Entries = Headers;
    fn profile(&mut self) -> Profile<'_, Bytes, Headers> { Profile::new(&self.records, &mut self.headers) }
}

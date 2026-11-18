#![forbid(unsafe_code)]

use xm1_saclac_interface::stock::{record_handle_from_a2, record_handle_word, StockCallbackWord};
use xm1_saclac_interface::RecordHandle;

use crate::memory::{Block, Location, Memory};
use crate::stock::{record, shared};
use crate::{Access, AccessGroup, Bytes, CaptureBuffer, ChangeSet, DeriveRefused, Handle, Index, MeasurementSelection, ProfileRecords, RecordImage, Refusal, Scope, Serial, Slot, StatisticsInUse, Tag, WindowPosition, WindowStatus};

const SLOT: u8 = 3;
const SERIAL: u16 = 0x2a;

// Registry offsets as 0x75e118 initialises them: the selected word at +8, then sixteen 0x38-byte
// slots from +0xc holding the serial, a word, the slot's record and the 43 grant bytes.
const REGISTRY_SELECTED: usize = 0x8;
const fn slot_offset(slot: u8) -> usize { 0xc + 0x38 * slot as usize }

fn group(group: u8) -> AccessGroup { AccessGroup::new(group).unwrap() }

fn put_word(bytes: &mut [u8], offset: usize, word: u32) { bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes()) }

fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) }

struct Registry {
    memory: Bytes,
}

impl Registry {
    fn new(selected: u32) -> Self {
        let mut memory = Bytes::default();
        for slot in 0..16 {
            put_word(&mut memory.registry, slot_offset(slot), u32::MAX);
        }
        put_word(&mut memory.registry, REGISTRY_SELECTED, selected);
        Self { memory }
    }

    /// Opens `SLOT` with `SERIAL`, naming `record` and granting `grants` (bit 0 read, bit 1 write) to `groups`.
    fn open(mut self, record: u32, groups: &[u8], grants: u8) -> Self {
        let base = slot_offset(SLOT);
        put_word(&mut self.memory.registry, base, u32::from(SERIAL));
        put_word(&mut self.memory.registry, base + 0x8, record);
        for &group in groups {
            self.memory.registry[base + 0xc + group as usize] = grants;
        }
        self
    }

    fn view(self) -> ProfileRecords<Bytes> { ProfileRecords::new(self.memory) }
}

fn owner() -> Handle {
    Handle {
        slot: Slot::new(SLOT).unwrap(),
        serial: Serial::new(SERIAL).unwrap(),
        owner: true,
        writable: true,
        scope: Scope::ALL,
        tag: Tag(0),
    }
}

fn word_of(handle: Handle) -> RecordHandle { crate::stock::record_handle(handle) }

const READ: u8 = 1;
const WRITE: u8 = 2;

/// A byte a writer leaves behind, so an untouched byte is distinguishable from a stored zero.
const PREFILL: u8 = 0x5a;

#[test]
fn reads_follow_the_selected_record() {
    for (selected, record) in [(0, 0), (1, 1)] {
        let mut registry = Registry::new(selected);
        registry.memory.records[record][0x2a..0x2c].copy_from_slice(&0x1234u16.to_le_bytes());
        let view = registry.view();
        assert_eq!(view.read(word_of(owner()), &record::group_00::AT_002A), Ok(0x1234));
    }
}

#[test]
fn a_selection_word_naming_neither_record_is_reported() {
    let view = Registry::new(2).view();
    assert_eq!(view.read(word_of(owner()), &record::group_00::AT_002A), Err(Refusal::SelectionOutOfRange(2)));
}

#[test]
fn a_granted_handle_reads_its_slots_record() {
    let mut registry = Registry::new(0).open(1, &[0], READ);
    registry.memory.records[1][0x69] = 7;
    registry.memory.records[0][0x69] = 9;
    assert_eq!(registry.view().read(word_of(owner()), &record::group_00::AT_0069), Ok(7));
}

#[test]
fn a_slot_naming_no_record_reads_the_selected_one() {
    let mut registry = Registry::new(0).open(2, &[0], READ);
    registry.memory.records[0][0x69] = 9;
    assert_eq!(registry.view().read(word_of(owner()), &record::group_00::AT_0069), Ok(9));
}

#[test]
fn reads_without_a_grant_fall_back_to_the_selected_record() {
    let cases: [(&str, Registry, RecordHandle); 4] = [
        ("no read grant", Registry::new(0).open(1, &[0], WRITE), word_of(owner())),
        ("grant on another group", Registry::new(0).open(1, &[1], READ), word_of(owner())),
        ("closed slot", Registry::new(0).open(1, &[0], READ), word_of(Handle { serial: Serial::new(SERIAL + 1).unwrap(), ..owner() })),
        ("no handle", Registry::new(0).open(1, &[0], READ), record_handle_from_a2(u32::MAX)),
    ];
    for (case, mut registry, handle) in cases {
        registry.memory.records[0][0x69] = 9;
        registry.memory.records[1][0x69] = 7;
        assert_eq!(registry.view().read(handle, &record::group_00::AT_0069), Ok(9), "{case}");
    }
}

#[test]
fn a_restricted_scope_admits_only_its_group() {
    let mut registry = Registry::new(0).open(1, &[0, 5], READ);
    registry.memory.records[1][0x69] = 7;
    registry.memory.records[0][0x69] = 9;
    let restricted = word_of(Handle { scope: Scope::one(group(5)), ..owner() });
    let view = registry.view();
    assert_eq!(view.read(restricted, &record::group_00::AT_0069), Ok(9));
    assert!(view.may_read(restricted, group(5)));
}

#[test]
fn a_granted_write_lands_at_its_offset_in_the_slots_record_and_marks_its_change() {
    let mut view = Registry::new(0).open(1, &[0], READ | WRITE).view();
    assert_eq!(view.write(word_of(owner()), &record::group_00::MEASUREMENT_SELECTION, MeasurementSelection::Record), Ok(()));
    let memory = view.into_memory();
    assert_eq!(memory.records[1][0x30], 1);
    assert_eq!(word(&memory.records[1], 0x80), 1);
    assert_eq!(memory.records[0], [0; 0x2158]);
}

#[test]
fn a_second_bank_change_lands_in_the_second_mask() {
    let mut view = Registry::new(0).open(0, &[0], READ | WRITE).view();
    view.write(word_of(owner()), &record::group_00::AT_0069, 1).unwrap();
    let memory = view.into_memory();
    assert_eq!(word(&memory.records[0], 0x88), 1 << 4);
    assert_eq!(word(&memory.records[0], 0x80), 0);
}

#[test]
fn writes_without_the_grant_are_refused_and_store_nothing() {
    let cases: [(&str, Registry, RecordHandle); 4] = [
        ("read grant only", Registry::new(0).open(1, &[0], READ), word_of(owner())),
        ("handle not writable", Registry::new(0).open(1, &[0], READ | WRITE), word_of(Handle { writable: false, ..owner() })),
        ("scope elsewhere", Registry::new(0).open(1, &[0, 4], READ | WRITE), word_of(Handle { scope: Scope::one(group(4)), ..owner() })),
        ("no handle", Registry::new(0).open(1, &[0], READ | WRITE), record_handle_from_a2(u32::MAX)),
    ];
    for (case, registry, handle) in cases {
        let mut view = registry.view();
        assert_eq!(view.write(handle, &record::group_00::MEASUREMENT_SELECTION, MeasurementSelection::Record), Err(Refusal::NotGranted), "{case}");
        let memory = view.into_memory();
        assert_eq!(memory.records, [[0; 0x2158]; 2], "{case}");
    }
}

#[test]
fn a_read_gated_list_write_is_never_refused_and_lands_where_a_read_would() {
    let mut view = Registry::new(1).open(0, &[30], WRITE).view();
    assert_eq!(view.write(word_of(owner()), &record::group_30::AT_0BB4, 0x55), Ok(()));
    let memory = view.into_memory();
    assert_eq!(word(&memory.records[1], 0xbb4), 0x55);
    assert_eq!(word(&memory.records[0], 0xbb4), 0);
}

#[test]
fn an_element_write_needs_every_group_it_spans() {
    let element = [0xa5; 168];
    let second = Index::new(1).unwrap();
    for missing in 5..=10 {
        let groups: Vec<u8> = (5..=10).filter(|&group| group != missing).collect();
        let mut refused = Registry::new(0).open(0, &groups, READ | WRITE).view();
        assert_eq!(refused.write_element(word_of(owner()), &record::group_05::AT_016C_168, second, element), Err(Refusal::NotGranted), "group {missing}");
    }

    let mut granted = Registry::new(0).open(0, &[5, 6, 7, 8, 9, 10], READ | WRITE).view();
    assert_eq!(granted.write_element(word_of(owner()), &record::group_05::AT_016C_168, second, element), Ok(()));
    assert_eq!(granted.read_element(word_of(owner()), &record::group_05::AT_016C_168, second), Ok(element));
    let memory = granted.into_memory();
    assert_eq!(&memory.records[0][0x214..0x2bc], &element[..]);
    assert_eq!(memory.records[0][0x213], 0);
    assert_eq!(memory.records[0][0x2bc], 0);
}

#[test]
fn list_entries_are_104_bytes_apart() {
    let mut registry = Registry::new(0);
    registry.memory.records[0][0x390 + 3 * 104..][..104].copy_from_slice(&[0x3c; 104]);
    let view = registry.view();
    assert_eq!(view.read_element(word_of(owner()), &record::group_30::AT_0390, Index::new(3).unwrap()), Ok([0x3c; 104]));
    assert!(Index::<10>::new(10).is_none());
}

#[test]
fn shared_fields_read_openly_and_write_under_their_group() {
    let mut view = Registry::new(0).open(0, &[20], WRITE).view();
    let handle = word_of(owner());
    let in_use = StatisticsInUse { primary: true, search: true, auxiliary: false };
    assert_eq!(view.write_shared(handle, &shared::group_20::STATISTICS_IN_USE, in_use), Ok(()));
    assert_eq!(view.read_shared(record_handle_from_a2(u32::MAX), &shared::group_20::STATISTICS_IN_USE), Ok(in_use));
    assert_eq!(view.write_shared(handle, &shared::group_27::WINDOW_STATUS, WindowStatus(2)), Err(Refusal::NotGranted));
    let memory = view.into_memory();
    assert_eq!(word(&memory.shared, 0x150), 3);
    assert_eq!(word(&memory.shared, 0x734), 0);
}

#[test]
fn the_guarded_shared_block_reads_under_write_access_and_writes_openly() {
    let block = [0x77; 44];
    let mut view = Registry::new(0).view();
    let handle = word_of(owner());
    assert_eq!(view.write_shared(handle, &shared::group_41::AT_099C, block), Ok(()));
    assert_eq!(view.read_shared(handle, &shared::group_41::AT_099C), Err(Refusal::NotGranted));

    let mut granted = Registry::new(0).open(0, &[41], WRITE);
    granted.memory.shared[0x99c..0x9c8].copy_from_slice(&block);
    assert_eq!(granted.view().read_shared(handle, &shared::group_41::AT_099C), Ok(block));
}

#[test]
fn shared_array_elements_are_eight_bytes_apart() {
    let mut view = Registry::new(0).open(0, &[41], WRITE).view();
    view.write_shared_element(word_of(owner()), &shared::group_41::AT_0974, Index::new(4).unwrap(), [1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
    assert_eq!(&view.into_memory().shared[0x994..0x99c], &[1, 2, 3, 4, 5, 6, 7, 8]);
    assert!(Index::<5>::new(5).is_none());
}

#[test]
fn applying_a_staged_record_writes_only_its_changed_fields() {
    let mut staged = RecordImage::default();
    staged.set(&record::group_00::MEASUREMENT_SELECTION, MeasurementSelection::Record);
    staged.set(&record::group_00::AT_0069, 6);
    let mut registry = Registry::new(0).open(0, &[0], READ | WRITE);
    registry.memory.records[0][0x31] = 0x44;
    registry.memory.records[0][0x80] = 0xff;
    let mut view = registry.view();
    assert_eq!(view.apply_changes(word_of(owner()), staged.header()), Ok(()));
    let record = &view.memory().records[0];
    assert_eq!(record[0x30], 1);
    assert_eq!(record[0x69], 6);
    assert_eq!(record[0x31], 0x44);
    assert_eq!(word(record, 0x80), 1);
    assert_eq!(word(record, 0x88), 1 << 4);
    let selection = record::group_00::MEASUREMENT_SELECTION.change().unwrap();
    let flag = record::group_00::AT_0069.change().unwrap();
    let unset = record::group_00::AT_0031.change().unwrap();
    assert_eq!(view.has_changes(word_of(owner()), ChangeSet::of(selection)), Ok(true));
    assert_eq!(view.has_changes(word_of(owner()), ChangeSet::of(flag)), Ok(true));
    assert_eq!(view.has_changes(word_of(owner()), ChangeSet::of(unset)), Ok(false));
    assert_eq!(view.has_changes(word_of(owner()), ChangeSet::of(selection).with(unset).unwrap()), Ok(false));
    assert_eq!(ChangeSet::of(selection).with(flag), None);
}

#[test]
fn applying_without_write_access_clears_the_masks_and_writes_nothing() {
    let mut staged = RecordImage::default();
    staged.set(&record::group_00::MEASUREMENT_SELECTION, MeasurementSelection::Record);
    let mut registry = Registry::new(0).open(0, &[0], READ);
    registry.memory.records[0][0x80] = 0xff;
    let mut view = registry.view();
    assert_eq!(view.apply_changes(word_of(owner()), staged.header()), Ok(()));
    let record = &view.memory().records[0];
    assert_eq!(record[0x30], 0);
    assert_eq!(word(record, 0x80), 0);
}

#[test]
fn the_chosen_word_follows_the_flag_byte() {
    let mut registry = Registry::new(0).open(0, &[0, 4], READ);
    registry.memory.records[0][0x2c..0x2e].copy_from_slice(&0xbeefu16.to_le_bytes());
    put_word(&mut registry.memory.records[0], 0x160, 0x1234_5678);
    registry.memory.records[0][0x69] = 1;
    let view = registry.view();
    assert_eq!(view.chosen_word(word_of(owner())), Ok(0xbeef));

    let mut other = Registry::new(0).open(0, &[0, 4], READ);
    put_word(&mut other.memory.records[0], 0x160, 0x1234_5678);
    assert_eq!(other.view().chosen_word(word_of(owner())), Ok(0x1234_5678));
}

#[test]
fn the_handle_word_carries_slot_serial_flags_scope_and_tag() {
    let handle = crate::stock::handle(record_handle_from_a2(0x11ff_02a3)).unwrap();
    assert_eq!(handle.slot, Slot::new(3).unwrap());
    assert_eq!(handle.serial, Serial::new(0x2a).unwrap());
    assert!(handle.owner && handle.writable);
    assert_eq!(handle.scope, Scope::ALL);
    assert_eq!(handle.tag, Tag(0x11));
    assert_eq!(record_handle_word(word_of(handle)), 0x11ff_02a3);
    assert_eq!(crate::stock::handle(record_handle_from_a2(u32::MAX)), None);
}

#[test]
fn derived_handles_keep_the_slot_and_never_own_it() {
    let derived = owner().derive(None, Access::ReadWrite, Tag::from_nibbles(1, 1)).unwrap();
    assert_eq!(record_handle_word(word_of(derived)), 0x11fe_02a3);
    let read_only = owner().derive(Some(group(7)), Access::Read, Tag(0)).unwrap();
    assert!(!read_only.writable && !read_only.owner);
    assert_eq!(read_only.scope, Scope::one(group(7)));
}

#[test]
fn derivation_refusals() {
    let read_only = Handle { writable: false, ..owner() };
    assert_eq!(read_only.derive(None, Access::Write, Tag(0)), Err(DeriveRefused::NotWritable));
    let restricted = Handle { scope: Scope::one(group(5)), ..owner() };
    assert_eq!(restricted.derive(None, Access::Read, Tag(0)), Err(DeriveRefused::Restricted));
    let to_group_one = Handle { scope: Scope::one(group(1)), ..owner() };
    assert_eq!(to_group_one.derive(None, Access::Read, Tag(0)).map(|handle| handle.scope), Ok(Scope::one(group(1))));
}

#[test]
fn a_staged_record_matches_a_change_set_exactly_or_when_the_other_mask_is_clear() {
    let selection = ChangeSet::of(record::group_00::MEASUREMENT_SELECTION.change().unwrap());
    let mut staged = RecordImage::default();
    assert!(staged.header().changes_are(selection));
    staged.set(&record::group_00::AT_0069, 1);
    assert!(!staged.header().changes_are(selection));
    staged.set(&record::group_00::MEASUREMENT_SELECTION, MeasurementSelection::Record);
    assert!(staged.header().changes_are(selection));
    staged.set(&record::group_00::AT_0031, 1);
    assert!(!staged.header().changes_are(selection));
    staged.clear_changes();
    staged.set(&record::group_00::AT_0031, 1);
    assert!(staged.header().changes_are(selection));
}

#[test]
fn the_parameter_record_parts_land_at_their_offsets_under_group_2() {
    // A slot granting group 1 only; the three writers gate on group 2.
    let mut refused = Registry::new(0).open(0, &[1], WRITE).view();
    let handle = word_of(owner());
    assert_eq!(refused.write(handle, &record::group_02::AT_0104, 0x11), Err(Refusal::NotGranted));
    assert_eq!(refused.write(handle, &record::group_02::AT_0108, [0x22; 20]), Err(Refusal::NotGranted));
    assert_eq!(refused.write(handle, &record::group_02::AT_011C, [0x33; 40]), Err(Refusal::NotGranted));

    let mut registry = Registry::new(0).open(0, &[2], WRITE);
    registry.memory.records[0][0x100..0x148].fill(PREFILL);
    let mut view = registry.view();
    view.write(handle, &record::group_02::AT_0104, 0x11).unwrap();
    view.write(handle, &record::group_02::AT_0108, [0x22; 20]).unwrap();
    view.write(handle, &record::group_02::AT_011C, [0x33; 40]).unwrap();
    let memory = view.into_memory();
    let record = &memory.records[0];
    assert_eq!(record[0x103], PREFILL);
    assert_eq!(record[0x104], 0x11);
    assert_eq!(&record[0x105..0x108], &[PREFILL; 3]);
    assert_eq!(&record[0x108..0x11c], &[0x22; 20]);
    assert_eq!(&record[0x11c..0x144], &[0x33; 40]);
    assert_eq!(record[0x144], PREFILL);
    // 0x75ac1c is the only setter of the change masks at +0x80 and +0x88; these three writers
    // leave both clear.
    assert_eq!(&record[0x80..0x90], &[0; 16]);
}

#[test]
fn the_shared_list_blocks_write_under_group_27() {
    let mut refused = Registry::new(0).open(0, &[20], WRITE).view();
    let handle = word_of(owner());
    assert_eq!(refused.write_shared(handle, &shared::group_27::AT_026C, [1; 1044]), Err(Refusal::NotGranted));
    assert_eq!(refused.write_shared(handle, &shared::group_27::AT_0680, [5; 40]), Err(Refusal::NotGranted));
    assert_eq!(refused.write_shared(handle, &shared::group_27::AT_06A8, [3; 80]), Err(Refusal::NotGranted));

    let mut view = Registry::new(0).open(0, &[27], WRITE).view();
    view.write_shared(handle, &shared::group_27::AT_026C, [1; 1044]).unwrap();
    view.write_shared(handle, &shared::group_27::AT_0680, [2; 40]).unwrap();
    view.write_shared(handle, &shared::group_27::AT_06A8, [3; 80]).unwrap();
    let memory = view.into_memory();
    assert_eq!(&memory.shared[0x26c..0x680], &[1; 1044][..]);
    assert_eq!(&memory.shared[0x680..0x6a8], &[2; 40]);
    assert_eq!(&memory.shared[0x6a8..0x6f8], &[3; 80]);
    assert_eq!(memory.shared[0x6f8], 0);
}

/// `Bytes` that also records each store as (block, offset, width).
#[derive(Default)]
struct Recording {
    bytes: Bytes,
    stores: Vec<(Block, u16, usize)>,
}

impl Recording {
    fn record<T>(&mut self, at: &Location<T>, width: usize) { self.stores.push((at.block(), at.offset(), width)) }
}

impl Memory for Recording {
    fn read_u8(&self, at: Location<u8>) -> u8 { self.bytes.read_u8(at) }

    fn read_u16(&self, at: Location<u16>) -> u16 { self.bytes.read_u16(at) }

    fn read_u32(&self, at: Location<u32>) -> u32 { self.bytes.read_u32(at) }

    fn write_u8(&mut self, at: Location<u8>, value: u8) {
        self.record(&at, 1);
        self.bytes.write_u8(at, value)
    }

    fn write_u16(&mut self, at: Location<u16>, value: u16) {
        self.record(&at, 2);
        self.bytes.write_u16(at, value)
    }

    fn write_u32(&mut self, at: Location<u32>, value: u32) {
        self.record(&at, 4);
        self.bytes.write_u32(at, value)
    }
}

fn recording(registry: Registry) -> ProfileRecords<Recording> { ProfileRecords::new(Recording { bytes: registry.memory, stores: Vec::new() }) }

const FIRST: Block = Block::Record(crate::RecordIndex::First);

#[test]
fn block_fields_are_stored_in_ascending_widest_aligned_units() {
    let mut view = recording(Registry::new(0).open(0, &[0, 15], READ | WRITE));
    let handle = word_of(owner());
    view.write(handle, &record::group_00::AT_0004, [1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
    view.write(handle, &record::group_00::AT_000E, [9, 10, 11, 12, 13, 14]).unwrap();
    view.write_shared(handle, &shared::group_15::FOCUS_CONFIRMATION, [0x5a; 32]).unwrap();
    let memory = view.into_memory();
    let field_stores: Vec<(Block, u16, usize)> = memory.stores.iter().copied().filter(|&(block, offset, _)| block != FIRST || offset < 0x80).collect();
    let confirmation = (0..8u16).map(|word| (Block::Shared, 0x84 + 4 * word, 4));
    let expected: Vec<(Block, u16, usize)> = [(FIRST, 0x4, 4), (FIRST, 0x8, 4), (FIRST, 0xe, 2), (FIRST, 0x10, 4)].into_iter().chain(confirmation).collect();
    assert_eq!(field_stores, expected);
    assert_eq!(&memory.bytes.records[0][0x4..0x14], &[1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 9, 10, 11, 12, 13, 14]);
}

#[test]
fn applying_a_staged_record_stores_fields_in_the_tracked_order() {
    let mut staged = RecordImage::default();
    staged.set(&record::group_00::AT_005A, 1);
    staged.set(&record::group_00::AT_0071, -1);
    staged.set(&record::group_00::AT_0000, 0x0102_0304);
    staged.set(&record::group_00::AT_002E, 0x0506);
    staged.set(&record::group_00::MEASUREMENT_SELECTION, MeasurementSelection::Record);
    let mut view = recording(Registry::new(0).open(0, &[0], READ | WRITE));
    view.apply_changes(word_of(owner()), staged.header()).unwrap();
    let field_offsets: Vec<u16> = view.memory().stores.iter().filter(|&&(block, offset, _)| block == FIRST && !(0x80..0x90).contains(&offset)).map(|&(_, offset, _)| offset).collect();
    assert_eq!(field_offsets, [0x30, 0x2e, 0x0, 0x71, 0x5a]);
}

#[test]
fn signed_fields_read_back_negative() {
    let mut registry = Registry::new(0);
    registry.memory.records[0][0x1a..0x1c].copy_from_slice(&[0xfe, 0xff]);
    registry.memory.records[0][0x71] = 0x80;
    registry.memory.shared[0x1fc..0x1fe].copy_from_slice(&[0x00, 0x80]);
    let view = registry.view();
    let handle = word_of(owner());
    assert_eq!(view.read(handle, &record::group_00::AT_001A), Ok(-2));
    assert_eq!(view.read(handle, &record::group_00::AT_0071), Ok(-128));
    assert_eq!(view.read_shared(handle, &shared::group_26::AT_01FC), Ok(i16::MIN));
}

#[test]
fn a_write_through_a_slot_naming_no_record_lands_in_the_selected_one() {
    let mut view = Registry::new(1).open(5, &[0], READ | WRITE).view();
    assert_eq!(view.write(word_of(owner()), &record::group_00::AT_0069, 3), Ok(()));
    let memory = view.into_memory();
    assert_eq!(memory.records[1][0x69], 3);
    assert_eq!(memory.records[0][0x69], 0);
}

#[test]
fn named_fields_store_their_stock_encoding() {
    let mut view = Registry::new(0).open(0, &[0, 20, 27], READ | WRITE).view();
    let handle = word_of(owner());
    view.write_shared(handle, &shared::group_20::STATISTICS_IN_USE, StatisticsInUse { primary: true, search: false, auxiliary: true }).unwrap();
    view.write_shared(handle, &shared::group_20::CAPTURE_BUFFER, CaptureBuffer::new(0x0123_4560)).unwrap();
    view.write_shared(handle, &shared::group_20::IMAGE_CALLBACK, StockCallbackWord(0x00f3_2c18)).unwrap();
    view.write_shared(handle, &shared::group_27::WINDOW_POSITION, WindowPosition([0x0102, 0x0304])).unwrap();
    view.write(handle, &record::group_00::MEASUREMENT_SELECTION, MeasurementSelection::Other(7)).unwrap();
    assert_eq!(view.read_shared(handle, &shared::group_20::CAPTURE_BUFFER), Ok(CaptureBuffer::new(0x0123_4560)));
    let memory = view.into_memory();
    assert_eq!(word(&memory.shared, 0x150), 0b101);
    assert_eq!(word(&memory.shared, 0x154), 0x0123_4560);
    assert_eq!(word(&memory.shared, 0x164), 0x00f3_2c18);
    assert_eq!(word(&memory.shared, 0x838), 0x0304_0102);
    assert_eq!(memory.records[0][0x30], 7);

    let mut detached = Registry::new(0).open(0, &[20], WRITE);
    put_word(&mut detached.memory.shared, 0x154, 0x0123_4560);
    let mut view = detached.view();
    view.write_shared(handle, &shared::group_20::CAPTURE_BUFFER, None).unwrap();
    assert_eq!(view.read_shared(handle, &shared::group_20::CAPTURE_BUFFER), Ok(None));
    assert_eq!(word(&view.into_memory().shared, 0x154), 0);
}

#[test]
fn a_location_outside_its_block_or_misaligned_cannot_be_formed() {
    let cases: [(&str, fn() -> u16); 4] = [
        ("word past the shared block", || Location::<u32>::new(Block::Shared, 0x9d4).offset()),
        ("block running past the registry", || Location::<[u8; 8]>::new(Block::Registry, 0x388).offset()),
        ("misaligned word", || Location::<u32>::new(FIRST, 0x82).offset()),
        ("misaligned halfword", || Location::<i16>::new(Block::Shared, 0x1fd).offset()),
    ];
    for (case, form) in cases {
        assert!(std::panic::catch_unwind(form).is_err(), "{case}");
    }
    assert_eq!(Location::<u32>::new(Block::Shared, 0x9d0).offset(), 0x9d0);
}

#[test]
fn streaming_copy_refuses_before_reading_a_source_without_every_destination_grant() {
    for missing in 5..=10 {
        let groups: std::vec::Vec<_> = (5..=10).filter(|&group| group != missing).collect();
        let mut view = Registry::new(0).open(1, &groups, READ | WRITE).view();
        assert_eq!(view.copy_element_with_source(word_of(owner()), &record::group_05::AT_016C_168, Index::new(0).unwrap(), crate::CopyGranule::Byte, |_, _| panic!("refused source read")), Err(Refusal::NotGranted));
        assert_eq!(view.into_memory().records[1], [0; 0x2158]);
    }
}

#[test]
fn streaming_copy_preserves_neighboring_fields_and_marks_only_the_admitted_record() {
    let mut registry = Registry::new(0).open(1, &[0], READ | WRITE);
    registry.memory.records[1].fill(PREFILL);
    let mut view = registry.view();
    assert_eq!(view.copy_with_source(word_of(owner()), &record::group_00::AT_0004, crate::CopyGranule::Byte, |offset, out| {
        out[0] = [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0][offset.get()];
    }), Ok(()));
    let memory = view.into_memory();
    assert_eq!(&memory.records[1][3..13], &[PREFILL, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, PREFILL]);
    assert_eq!(word(&memory.records[1], 0x80), 0x5e5a5a5a);
    assert_eq!(memory.records[0], [0; 0x2158]);
}

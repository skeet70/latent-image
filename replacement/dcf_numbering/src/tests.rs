use crate::numbering::*;
use std::collections::BTreeMap;

const CONTEXT: u32 = 0x0100_0000;
const PATH: u32 = 0x0200_0000;
const DCIM: u32 = 0x0300_0000;
const SCRATCH: u32 = 0x0400_0000;
const TEXT: u32 = 0x0500_0000;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Call {
    FolderLimit,
    StoreDirectory { group: u32, directory: u32, file: u32 },
    StoreFile { group: u32, file: u32 },
    ResetProfile,
    Snapshot,
    OpenDirectory(String),
}

struct Card {
    memory: BTreeMap<u32, u8>,
    media_status: u32,
    drive: u32,
    profile: BTreeMap<u32, (u16, u16)>,
    configuration: BTreeMap<u32, u8>,
    open_status: u32,
    calls: Vec<Call>,
}

impl Card {
    fn new() -> Card {
        let mut card = Card {
            memory: BTreeMap::new(),
            media_status: 0,
            drive: b'C' as u32,
            profile: BTreeMap::new(),
            configuration: BTreeMap::new(),
            open_status: 0,
            calls: Vec::new(),
        };
        card.bytes(DCIM, b"\\DCIM\\");
        card.bytes(CONTEXT, b"DSCF");
        card.write_u32(CONTEXT + 0x2210, 1);
        card.bytes(CONTEXT + 0x21da, b"_FUJI");
        card.write_u8(CONTEXT + 0x2600, 1);
        card
    }

    fn bytes(&mut self, address: u32, bytes: &[u8]) {
        for (index, byte) in bytes.iter().enumerate() {
            self.memory.insert(address + index as u32, *byte);
        }
    }

    fn text(&self, address: u32, length: u32) -> String {
        (0..length).map(|index| self.read_u8(address + index) as char).collect()
    }

    fn last_used(&mut self, group: u32, directory: u16, file: u16) {
        self.write_u16(CONTEXT + 0x21d0 + 16 * group + 4, directory);
        self.write_u16(CONTEXT + 0x21d0 + 16 * group + 6, file);
    }

    fn card_maximum(&mut self, directory: u32, file: u32) {
        self.write_u32(CONTEXT + 0x2168, directory);
        self.write_u32(CONTEXT + 0x216c, file);
        self.write_u32(CONTEXT + 0x21b8, directory);
    }

    fn name(&mut self) -> (Status, String) {
        let status = prepare_name(self, PATH, 0, 0);
        (status, self.text(PATH, 0x19))
    }

    fn scratch() -> FolderScratch {
        FolderScratch { request: SCRATCH, entry: SCRATCH + 0x40, mask: SCRATCH + 0x100 }
    }
}

impl Guest for Card {
    fn read_u8(&self, address: u32) -> u8 {
        self.memory.get(&address).copied().unwrap_or(0)
    }
    fn read_u16(&self, address: u32) -> u16 {
        u16::from_le_bytes([self.read_u8(address), self.read_u8(address + 1)])
    }
    fn read_u32(&self, address: u32) -> u32 {
        u32::from_le_bytes([0, 1, 2, 3].map(|index| self.read_u8(address + index)))
    }
    fn write_u8(&mut self, address: u32, value: u8) {
        self.memory.insert(address, value);
    }
    fn write_u16(&mut self, address: u32, value: u16) {
        self.bytes(address, &value.to_le_bytes());
    }
    fn write_u32(&mut self, address: u32, value: u32) {
        self.bytes(address, &value.to_le_bytes());
    }
    fn context(&self) -> u32 {
        CONTEXT
    }
    fn dcim_text(&self) -> u32 {
        DCIM
    }
    fn media_status(&mut self) -> u32 {
        self.media_status
    }
    fn current_drive(&mut self) -> u32 {
        self.drive
    }
    fn group_index(&mut self, mask: u32) -> u32 {
        if mask == 0 { 0xff } else { mask.trailing_zeros() }
    }
    fn report_folder_limit(&mut self) {
        self.calls.push(Call::FolderLimit);
    }
    fn drive_prefix(&mut self, destination: u32) {
        let drive = self.drive as u8;
        self.bytes(destination, &[drive, b':', 0]);
    }
    fn copy_bytes(&mut self, destination: u32, source: u32, length: u32) {
        let bytes: Vec<u8> = (0..length).map(|index| self.read_u8(source + index)).collect();
        self.bytes(destination, &bytes);
    }
    fn format_decimal(&mut self, destination: u32, value: u32, digits: u32) {
        let text = format!("{:0width$}", value, width = digits as usize);
        self.bytes(destination, &text.as_bytes()[text.len() - digits as usize..]);
    }
    fn configuration_byte(&mut self, key: u32) -> u32 {
        u32::from(self.configuration.get(&key).copied().unwrap_or(0))
    }
    fn profile_directory(&mut self, _drive: u32, group: u32) -> u32 {
        u32::from(self.profile.get(&group).map_or(100, |pair| pair.0))
    }
    fn profile_file(&mut self, _drive: u32, group: u32) -> u32 {
        u32::from(self.profile.get(&group).map_or(0, |pair| pair.1))
    }
    fn store_profile_directory(&mut self, _drive: u32, group: u32, directory: u32, file: u32) {
        self.calls.push(Call::StoreDirectory { group, directory, file });
        self.profile.entry(group).or_insert((100, 0)).0 = directory as u16;
    }
    fn store_profile_file(&mut self, _drive: u32, group: u32, file: u32) {
        self.calls.push(Call::StoreFile { group, file });
        self.profile.entry(group).or_insert((100, 0)).1 = file as u16;
    }
    fn reset_profile(&mut self) {
        self.calls.push(Call::ResetProfile);
        self.profile.clear();
    }
    fn refresh_snapshot(&mut self) {
        self.calls.push(Call::Snapshot);
    }
    fn clear_path_tail(&mut self, destination: u32) {
        self.bytes(destination, &[0; 30]);
    }
    fn format_folder_number(&mut self, destination: u32, number: u32) {
        self.format_decimal(destination, number, 3);
    }
    fn copy_folder_suffix(&mut self, destination: u32, source: u32) {
        self.copy_bytes(destination, source, 5);
    }
    fn open_directory(&mut self, path: u32, entry: u32) -> u32 {
        let path = self.text(path, 17);
        self.calls.push(Call::OpenDirectory(path));
        if self.open_status == 0 {
            self.write_u16(entry + 0x24 + 26, 0x0042);
        }
        self.open_status
    }
    fn entry_cluster(&mut self, entry: u32) -> u32 {
        u32::from(self.read_u16(entry + 26))
    }
    fn path_groups(&mut self, mask: u32, path: u32) {
        let suffix = self.text(path + 11, 5);
        self.write_u32(mask, if suffix == "_FUJI" { 1 } else { 0 });
    }
    fn translate_status(&mut self, status: u32) -> u32 {
        if status == 0 { 0 } else { 0x80 | status }
    }
}

#[test]
fn an_empty_card_names_the_first_file_of_folder_100() {
    let mut card = Card::new();
    assert_eq!(card.name(), (Status::Done, "C:\\DCIM\\100_FUJI\\DSCF0001".into()));
}

#[test]
fn the_next_name_follows_the_last_file_in_its_folder() {
    let mut card = Card::new();
    card.last_used(0, 103, 3095);
    card.card_maximum(103, 3095);
    assert_eq!(card.name(), (Status::Done, "C:\\DCIM\\103_FUJI\\DSCF3096".into()));
}

#[test]
fn a_full_thousand_block_moves_to_the_next_folder() {
    let mut card = Card::new();
    card.last_used(0, 103, 3999);
    card.card_maximum(103, 3999);
    assert_eq!(card.name(), (Status::Done, "C:\\DCIM\\104_FUJI\\DSCF4001".into()));
    assert_eq!(card.read_u16(CONTEXT + 0x127d4), 1);
}

#[test]
fn file_numbers_wrap_after_9999_into_a_new_folder() {
    let mut card = Card::new();
    card.last_used(0, 109, 9999);
    card.card_maximum(109, 9999);
    assert_eq!(card.name(), (Status::Done, "C:\\DCIM\\110_FUJI\\DSCF0001".into()));
}

#[test]
fn a_new_folder_starts_past_the_highest_folder_on_the_card() {
    let mut card = Card::new();
    card.last_used(0, 103, 3999);
    card.card_maximum(250, 1);
    assert_eq!(card.name(), (Status::Done, "C:\\DCIM\\251_FUJI\\DSCF1001".into()));
}

#[test]
fn folder_999_full_refuses_and_reports_without_writing_a_name() {
    let mut card = Card::new();
    card.last_used(0, 999, 9999);
    card.card_maximum(999, 9999);
    assert_eq!(prepare_name(&mut card, PATH, 0, 0), Status::FolderLimit);
    assert_eq!(card.calls, vec![Call::FolderLimit]);
    assert_eq!(card.text(PATH, 1), "\0");
}

#[test]
fn a_file_outside_its_folders_block_moves_to_the_folder_matching_its_thousands() {
    let mut card = Card::new();
    card.last_used(0, 100, 3095);
    card.card_maximum(100, 3095);
    assert_eq!(card.name(), (Status::Done, "C:\\DCIM\\103_FUJI\\DSCF3096".into()));
}

#[test]
fn a_file_below_its_folders_block_restarts_at_the_block() {
    let mut card = Card::new();
    card.last_used(0, 105, 2010);
    card.card_maximum(105, 2010);
    assert_eq!(card.name(), (Status::Done, "C:\\DCIM\\105_FUJI\\DSCF5001".into()));
}

#[test]
fn valid_caller_text_replaces_the_folder_suffix_and_file_prefix() {
    let mut card = Card::new();
    card.bytes(TEXT, b"_TEST");
    card.bytes(TEXT + 8, b"ABCD");
    assert_eq!(prepare_name(&mut card, PATH, TEXT, TEXT + 8), Status::Done);
    assert_eq!(card.text(PATH, 0x19), "C:\\DCIM\\100_TEST\\ABCD0001");
}

#[test]
fn invalid_caller_text_falls_back_to_the_context_suffix_and_prefix() {
    let mut card = Card::new();
    card.bytes(TEXT, b"_T-ST");
    card.bytes(TEXT + 8, b"AB.D");
    assert_eq!(prepare_name(&mut card, PATH, TEXT, TEXT + 8), Status::Done);
    assert_eq!(card.text(PATH, 0x19), "C:\\DCIM\\100_FUJI\\DSCF0001");
}

#[test]
fn a_sequenced_prefix_takes_configuration_bytes_and_the_advanced_sequence() {
    let mut card = Card::new();
    card.configuration.insert(0x144, b'M');
    card.configuration.insert(0x145, b'V');
    card.write_u16(CONTEXT + 0x127d4, 6);
    set_prefix_mode(&mut card, 2);
    assert_eq!(card.name(), (Status::Done, "C:\\DCIM\\100_FUJI\\MV070001".into()));
    assert_eq!(card.read_u8(CONTEXT + 0x16bd0), 1);
    assert_eq!(card.name(), (Status::Done, "C:\\DCIM\\100_FUJI\\MV070001".into()));
}

#[test]
fn an_exhausted_sequence_starts_a_new_folder() {
    let mut card = Card::new();
    card.configuration.insert(0x142, b'S');
    card.configuration.insert(0x143, b'Q');
    card.last_used(0, 100, 5);
    card.card_maximum(100, 5);
    card.write_u16(CONTEXT + 0x127d4, 99);
    set_prefix_mode(&mut card, 1);
    assert_eq!(card.name(), (Status::Done, "C:\\DCIM\\101_FUJI\\SQ011001".into()));
}

#[test]
fn an_unknown_prefix_mode_leaves_the_mode_unchanged() {
    let mut card = Card::new();
    set_prefix_mode(&mut card, 1);
    set_prefix_mode(&mut card, 7);
    assert_eq!(card.read_u32(CONTEXT + 0x16bd4), 1);
    set_prefix_mode(&mut card, 0);
    assert_eq!((card.read_u8(CONTEXT + 0x16bd0), card.read_u32(CONTEXT + 0x16bd4)), (0, 0));
}

#[test]
fn continuous_numbering_continues_from_the_persisted_pair() {
    let mut card = Card::new();
    card.last_used(0, 103, 3096);
    card.card_maximum(103, 3096);
    card.profile.insert(0, (105, 5010));
    apply_continuous_numbering(&mut card, 0);
    card.profile.insert(0, (105, 5010));
    assert_eq!(card.name(), (Status::Done, "C:\\DCIM\\103_FUJI\\DSCF3097".into()));
    card.write_u32(CONTEXT + 0x16c6c, 1);
    assert_eq!(card.name(), (Status::Done, "C:\\DCIM\\105_FUJI\\DSCF5011".into()));
}

#[test]
fn frame_number_cont_persists_each_group_and_renew_resets() {
    let mut card = Card::new();
    card.write_u8(CONTEXT + 0x2600, 2);
    card.last_used(0, 103, 3096);
    card.last_used(1, 120, 12);
    apply_continuous_numbering(&mut card, 1);
    assert_eq!(
        card.calls,
        vec![
            Call::StoreDirectory { group: 0, directory: 103, file: 3096 },
            Call::StoreFile { group: 0, file: 3096 },
            Call::StoreDirectory { group: 1, directory: 120, file: 12 },
            Call::StoreFile { group: 1, file: 12 },
        ]
    );
    assert_eq!(card.read_u32(CONTEXT + 0x16c6c), 1);
    card.calls.clear();
    apply_continuous_numbering(&mut card, 0);
    assert_eq!(card.calls, vec![Call::ResetProfile]);
    assert_eq!(card.read_u32(CONTEXT + 0x16c6c), 0);
}

#[test]
fn numbering_without_a_drive_or_group_is_refused_and_media_status_passes_through() {
    let mut card = Card::new();
    card.drive = 0;
    assert_eq!(read_numbers(&mut card), Err(Status::Refused));
    card.drive = b'C' as u32;
    card.write_u32(CONTEXT + 0x2210, 0);
    assert_eq!(read_numbers(&mut card), Err(Status::Refused));
    card.media_status = 0x11;
    assert_eq!(prepare_name(&mut card, PATH, 0, 0), Status::Media(0x11));
}

#[test]
fn a_closed_file_advances_its_group_and_persists_it() {
    let mut card = Card::new();
    card.last_used(0, 103, 3095);
    card.card_maximum(103, 3095);
    card.write_u16(CONTEXT + 2 * 16 + 4, 103);
    card.write_u16(CONTEXT + 2 * 16 + 6, 3096);
    card.write_u16(CONTEXT + 0x127d4, 4);
    note_written(&mut card, 2, Card::scratch());
    assert_eq!((card.read_u16(CONTEXT + 0x21d4), card.read_u16(CONTEXT + 0x21d6)), (103, 3096));
    assert_eq!(card.read_u32(CONTEXT + 0x21d0), 1);
    assert_eq!((card.read_u32(CONTEXT + 0x2168), card.read_u32(CONTEXT + 0x216c)), (103, 3096));
    assert_eq!(card.read_u32(CONTEXT + 0x1fc), 1);
    assert_eq!(card.read_u16(CONTEXT + 0x16c64), 4);
    assert_eq!(
        card.calls,
        vec![
            Call::StoreDirectory { group: 0, directory: 103, file: 3096 },
            Call::StoreFile { group: 0, file: 3096 },
            Call::Snapshot,
        ]
    );
}

#[test]
fn an_older_closed_file_changes_nothing() {
    let mut card = Card::new();
    card.last_used(0, 103, 3095);
    card.write_u16(CONTEXT + 4, 103);
    card.write_u16(CONTEXT + 6, 3001);
    note_written(&mut card, 0, Card::scratch());
    assert!(card.calls.is_empty());
    assert_eq!(card.read_u16(CONTEXT + 0x21d6), 3095);
}

#[test]
fn a_closed_file_in_a_foreign_group_folder_prepares_the_next_folder() {
    let mut card = Card::new();
    card.write_u8(CONTEXT + 0x2218 + 104, 1);
    card.write_u16(CONTEXT + 4, 104);
    card.write_u16(CONTEXT + 6, 4001);
    note_written(&mut card, 0, Card::scratch());
    assert!(card.calls.contains(&Call::OpenDirectory("C:\\DCIM\\105_FUJI\\".into())));
    assert_eq!(card.read_u32(CONTEXT + 0x2604 + 4 * 105), 0x42);
    assert_eq!(card.read_u32(CONTEXT + 0x2168), 105);
}

#[test]
fn creating_a_folder_records_its_cluster_group_and_maximum() {
    let mut card = Card::new();
    assert_eq!(create_folder(&mut card, 107, CONTEXT + 0x21da, Card::scratch()), 0);
    assert_eq!(card.calls, vec![Call::OpenDirectory("C:\\DCIM\\107_FUJI\\".into())]);
    assert_eq!(card.read_u32(CONTEXT + 0x2604 + 4 * 107), 0x42);
    assert_eq!(card.read_u8(CONTEXT + 0x2218 + 107), 0);
    assert_eq!((card.read_u16(CONTEXT + 0x21d4), card.read_u16(CONTEXT + 0x21d6)), (107, 0));
    assert_eq!(card.read_u32(CONTEXT + 0x21b8), 107);
    assert_eq!((card.read_u32(CONTEXT + 0x2168), card.read_u32(CONTEXT + 0x216c)), (107, 0));
}

#[test]
fn an_existing_or_out_of_range_folder_is_refused_without_opening() {
    let mut card = Card::new();
    card.write_u32(CONTEXT + 0x2604 + 4 * 107, 9);
    assert_eq!(create_folder(&mut card, 107, CONTEXT + 0x21da, Card::scratch()), 8);
    assert_eq!(create_folder(&mut card, 1000, CONTEXT + 0x21da, Card::scratch()), 8);
    assert_eq!(create_folder(&mut card, 99, CONTEXT + 0x21da, Card::scratch()), 8);
    assert!(card.calls.is_empty());
}

#[test]
fn a_failed_directory_open_returns_the_translated_status_and_records_nothing() {
    let mut card = Card::new();
    card.open_status = 5;
    assert_eq!(create_folder(&mut card, 107, CONTEXT + 0x21da, Card::scratch()), 0x85);
    assert_eq!(card.read_u32(CONTEXT + 0x2604 + 4 * 107), 0);
    assert_eq!(card.read_u32(CONTEXT + 0x2168), 0);
}

#[test]
fn names_accept_only_letters_digits_and_underscore() {
    let mut card = Card::new();
    card.bytes(TEXT, b"Ab_9-");
    assert_eq!(check_name(&card, TEXT, 4), Status::Done);
    assert_eq!(check_name(&card, TEXT, 5), Status::Refused);
    assert_eq!(check_name(&card, TEXT, 0), Status::Done);
}

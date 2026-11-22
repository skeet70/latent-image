use crate::completion::RawStatus;
use crate::paths::*;
use crate::requests::Callback;
use crate::workers::Offset;

#[derive(Debug, PartialEq, Clone)]
enum Event {
    Complete(u32, u32),
    Refresh,
    DeleteFile,
    DeleteDirectory,
    ReadOnly,
    Rebuild(Rebuild),
    Reload,
    Split(u32),
    Wildcard(u32),
    Number(Vec<u8>, u32),
    Update(EntryService, u32, Vec<u8>, u32, u32, u32),
    DeviceState(u32),
    FileNumber(Vec<u8>),
    ExtensionId(u32),
    ExtensionKind(u32),
    Find,
    Delete(u32),
}

const PAYLOAD: u32 = 0x5000;
const CALLBACK_WORD: u32 = 0x380000;
const DIRECTORY: &[u8] = b"100_FUJI";
const NAME: &[u8] = b"DSCF3097.JPG";

struct Fake {
    payload: Vec<u8>,
    context: bool,
    cancelled: Option<Cancel>,
    record: u32,
    helper: u32,
    rebuild: u32,
    reload: u32,
    split: u32,
    wildcard: u32,
    number: u32,
    update: u32,
    state: u8,
    file: u32,
    found: u32,
    entry: (u16, u16, u8, u8, u8),
    extension: u32,
    kind: u32,
    delete: u32,
    events: Vec<Event>,
}

/// Raw 0 translates to 0 and other raw results to raw + 100.
fn translated(raw: u32) -> u32 {
    if raw == 0 { 0 } else { raw + 100 }
}

impl Fake {
    fn new() -> Fake {
        let mut fake = Fake {
            payload: vec![0; 0x60],
            context: true,
            cancelled: None,
            record: 0x4000,
            helper: 0,
            rebuild: 0,
            reload: 0,
            split: 0,
            wildcard: 0,
            number: 0x67,
            update: 0,
            state: 0,
            file: 3097,
            found: 0,
            entry: (0x67, 3097, 0, 1, 0),
            extension: 2,
            kind: 1,
            delete: 0,
            events: Vec::new(),
        };
        fake.payload[0..4].copy_from_slice(&CALLBACK_WORD.to_le_bytes());
        fake
    }

    fn path(mut self, path: &[u8]) -> Fake {
        self.payload[4..4 + path.len()].copy_from_slice(path);
        self
    }

    fn word_at(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.payload[offset..offset + 4].try_into().unwrap())
    }

    fn path_bytes(&self) -> &[u8] {
        &self.payload[4..0x34]
    }

    fn completions(&self) -> Vec<Event> {
        self.events.iter().filter(|event| matches!(event, Event::Complete(..))).cloned().collect()
    }
}

impl Port for Fake {
    fn word(&self, at: Offset) -> u32 {
        self.word_at(at.0 as usize)
    }
    fn set_word(&mut self, at: Offset, value: u32) {
        self.payload[at.0 as usize..at.0 as usize + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn byte(&self, at: Offset) -> u8 {
        self.payload[at.0 as usize]
    }
    fn set_byte(&mut self, at: Offset, value: u8) {
        self.payload[at.0 as usize] = value;
    }
    fn address(&self, at: Offset) -> u32 {
        PAYLOAD + at.0
    }
    fn context(&self) -> bool {
        self.context
    }
    fn cancelled(&self, cancel: Cancel) -> bool {
        self.cancelled == Some(cancel)
    }
    fn directory_record(&self, number: DirectoryNumber) -> DirectoryRecord {
        DirectoryRecord(if number.0 == self.number { self.record } else { 0 })
    }
    fn refresh_media(&mut self) {
        self.events.push(Event::Refresh);
    }
    fn translate(&mut self, raw: RawStatus) -> u32 {
        translated(raw.0)
    }
    fn complete_status(&mut self, callback: Callback, status: u32) {
        if callback.0 != 0 {
            self.events.push(Event::Complete(callback.0, status));
        }
    }
    fn delete_file(&mut self) -> u32 {
        assert_eq!(self.word_at(0), 0);
        self.events.push(Event::DeleteFile);
        self.helper
    }
    fn delete_directory(&mut self) -> u32 {
        self.events.push(Event::DeleteDirectory);
        self.helper
    }
    fn set_read_only(&mut self) -> u32 {
        self.events.push(Event::ReadOnly);
        self.helper
    }
    fn rebuild_catalog(&mut self, rebuild: Rebuild) -> RawStatus {
        self.events.push(Event::Rebuild(rebuild));
        RawStatus(self.rebuild)
    }
    fn reload_catalog(&mut self) -> RawStatus {
        self.events.push(Event::Reload);
        RawStatus(self.reload)
    }
    fn split_path<const D: usize, const N: usize>(&mut self, path: u32, split: &mut Split<D, N>) -> RawStatus {
        self.events.push(Event::Split(path));
        split.directory[..DIRECTORY.len()].copy_from_slice(DIRECTORY);
        split.name[..NAME.len()].copy_from_slice(NAME);
        split.word = 4;
        RawStatus(self.split)
    }
    fn wildcard(&mut self, path: u32) -> u32 {
        self.events.push(Event::Wildcard(path));
        self.wildcard
    }
    fn directory_number<const D: usize, const N: usize>(&mut self, split: &Split<D, N>) -> DirectoryNumber {
        self.events.push(Event::Number(split.directory[..DIRECTORY.len()].to_vec(), split.word));
        DirectoryNumber(self.number)
    }
    fn update_entry<const D: usize, const N: usize>(
        &mut self,
        service: EntryService,
        record: DirectoryRecord,
        split: &Split<D, N>,
        mode: u32,
        value: u32,
        wildcard: u32,
    ) -> RawStatus {
        self.events.push(Event::Update(service, record.0, split.name[..NAME.len()].to_vec(), mode, value, wildcard));
        RawStatus(self.update)
    }
    fn device_state(&mut self, device: u32, state: &mut [u8; 48]) -> DeviceState {
        self.events.push(Event::DeviceState(device));
        state[0] = self.state;
        DeviceState(state[0])
    }
    fn file_number(&mut self, name: &[u8; 48]) -> FileNumber {
        self.events.push(Event::FileNumber(name[..NAME.len()].to_vec()));
        FileNumber(self.file)
    }
    fn extension_id(&mut self, extension: u32) -> ExtensionId {
        self.events.push(Event::ExtensionId(extension));
        ExtensionId(self.extension)
    }
    fn extension_kind(&mut self, id: ExtensionId) -> u32 {
        self.events.push(Event::ExtensionKind(id.0));
        self.kind
    }
    fn find_entry(&mut self, entry: &mut Entry) -> RawStatus {
        self.events.push(Event::Find);
        let (directory, file, first, second, third) = self.entry;
        entry.0[0x6c..0x6e].copy_from_slice(&directory.to_le_bytes());
        entry.0[0x6e..0x70].copy_from_slice(&file.to_le_bytes());
        entry.0[0x7c] = first;
        entry.0[0x7d] = second;
        entry.0[0x7e] = third;
        RawStatus(self.found)
    }
    fn delete_path(&mut self, path: u32) -> RawStatus {
        self.events.push(Event::Delete(path));
        RawStatus(self.delete)
    }
}

// File delete, read-only and directory delete -------------------------------------------------

#[test]
fn a_completed_file_delete_rebuilds_the_catalog_and_delivers_the_translated_rebuild() {
    let mut port = Fake::new();
    port.rebuild = 9;
    assert_eq!(file_delete(&mut port), 0);
    assert_eq!(port.events, vec![Event::DeleteFile, Event::Rebuild(Rebuild::Completed), Event::Complete(CALLBACK_WORD, translated(9))]);
    assert_eq!(port.word_at(0), CALLBACK_WORD);
}

#[test]
fn a_failed_file_delete_resets_the_catalog_and_delivers_the_status_untranslated() {
    let mut port = Fake::new();
    port.helper = 3;
    file_delete(&mut port);
    assert_eq!(port.events, vec![Event::DeleteFile, Event::Rebuild(Rebuild::Failed), Event::Complete(CALLBACK_WORD, 3)]);
}

#[test]
fn a_file_delete_status_of_six_leaves_the_catalog() {
    let mut port = Fake::new();
    port.helper = 6;
    file_delete(&mut port);
    assert_eq!(port.events, vec![Event::DeleteFile, Event::Complete(CALLBACK_WORD, 6)]);
}

#[test]
fn a_cancelled_file_delete_delivers_eleven_and_leaves_the_callback_word_clear() {
    let mut port = Fake::new();
    port.cancelled = Some(Cancel::Filename);
    assert_eq!(file_delete(&mut port), 0);
    assert_eq!(port.events, vec![Event::Complete(CALLBACK_WORD, 11)]);
    assert_eq!(port.word_at(0), 0);
}

#[test]
fn the_file_delete_ignores_the_metadata_cancel_and_the_read_only_ignores_the_filename_cancel() {
    let mut port = Fake::new();
    port.cancelled = Some(Cancel::Metadata);
    file_delete(&mut port);
    assert_eq!(port.events[0], Event::DeleteFile);
    let mut port = Fake::new();
    port.cancelled = Some(Cancel::Filename);
    read_only(&mut port);
    assert_eq!(port.events[0], Event::ReadOnly);
    let mut port = Fake::new();
    port.cancelled = Some(Cancel::Metadata);
    read_only(&mut port);
    assert_eq!(port.events, vec![Event::Complete(CALLBACK_WORD, 11)]);
}

#[test]
fn without_a_context_the_request_workers_deliver_five() {
    for worker in [file_delete::<Fake>, read_only::<Fake>] {
        let mut port = Fake::new();
        port.context = false;
        assert_eq!(worker(&mut port), 0);
        assert_eq!(port.events, vec![Event::Complete(CALLBACK_WORD, 5)]);
        assert_eq!(port.word_at(0), CALLBACK_WORD);
    }
    let mut port = Fake::new();
    port.context = false;
    assert_eq!(directory_delete(&mut port), 5);
}

#[test]
fn the_directory_delete_returns_its_status_and_never_resets_the_catalog() {
    let mut port = Fake::new();
    port.helper = 6;
    assert_eq!(directory_delete(&mut port), 6);
    assert_eq!(port.events, vec![Event::DeleteDirectory, Event::Complete(CALLBACK_WORD, 6)]);
    let mut port = Fake::new();
    port.helper = 3;
    assert_eq!(directory_delete(&mut port), 3);
    assert_eq!(port.events, vec![Event::DeleteDirectory, Event::Complete(CALLBACK_WORD, 3)]);
    let mut port = Fake::new();
    port.rebuild = 13;
    assert_eq!(directory_delete(&mut port), translated(13));
    assert_eq!(port.events, vec![Event::DeleteDirectory, Event::Rebuild(Rebuild::Completed), Event::Complete(CALLBACK_WORD, translated(13))]);
}

// Filename update -------------------------------------------------------------------------------

fn filename(keep: u8) -> Fake {
    let mut port = Fake::new().path(b"B:/DCIM/100_FUJI/DSCF3097.JPG");
    port.payload[0x34] = keep;
    port
}

fn split_events() -> Vec<Event> {
    vec![Event::Split(PAYLOAD + 4), Event::Wildcard(PAYLOAD + 4), Event::Number(DIRECTORY.to_vec(), 4)]
}

#[test]
fn a_filename_update_of_a_numbered_directory_updates_the_entry_refreshes_and_rebuilds() {
    let mut port = filename(0);
    port.wildcard = 1;
    port.rebuild = 9;
    assert_eq!(filename_set(&mut port), 0);
    let mut expected = split_events();
    expected.extend([
        Event::Update(EntryService::Update, 0x4000, NAME.to_vec(), 0, 0, 1),
        Event::Refresh,
        Event::Rebuild(Rebuild::Completed),
        Event::Complete(CALLBACK_WORD, translated(9)),
    ]);
    assert_eq!(port.events, expected);
    assert_eq!(port.word_at(0), CALLBACK_WORD);
}

#[test]
fn a_kept_catalog_filename_update_delivers_the_update_result_and_counts_twenty_two_as_success() {
    let mut port = filename(1);
    port.update = 22;
    filename_set(&mut port);
    assert_eq!(&port.events[3..], &[Event::Update(EntryService::Update, 0x4000, NAME.to_vec(), 0, 0, 0), Event::Refresh, Event::Complete(CALLBACK_WORD, translated(22))]);
    let mut port = filename(0);
    port.update = 22;
    filename_set(&mut port);
    assert_eq!(&port.events[4..], &[Event::Refresh, Event::Rebuild(Rebuild::Completed), Event::Complete(CALLBACK_WORD, 0)]);
}

#[test]
fn a_failed_filename_update_resets_the_catalog_unless_it_failed_with_seven() {
    let mut port = filename(0);
    port.update = 9;
    filename_set(&mut port);
    assert_eq!(&port.events[4..], &[Event::Refresh, Event::Rebuild(Rebuild::Failed), Event::Complete(CALLBACK_WORD, translated(9))]);
    let mut port = filename(0);
    port.update = 7;
    filename_set(&mut port);
    assert_eq!(&port.events[4..], &[Event::Refresh, Event::Complete(CALLBACK_WORD, translated(7))]);
}

#[test]
fn a_filename_path_without_a_usable_directory_record_only_refreshes_and_rebuilds() {
    for (number, record) in [(0, 0x4000), (0x67, 0), (0x67, u32::MAX), (99, 0x4000)] {
        let mut port = filename(0);
        port.number = number;
        port.record = record;
        filename_set(&mut port);
        assert_eq!(&port.events[3..], &[Event::Refresh, Event::Rebuild(Rebuild::Completed), Event::Complete(CALLBACK_WORD, 0)], "{number} {record:#x}");
    }
    let mut port = filename(1);
    port.number = 0;
    filename_set(&mut port);
    assert_eq!(&port.events[3..], &[Event::Refresh, Event::Complete(CALLBACK_WORD, 0)]);
}

#[test]
fn a_refused_split_ends_the_filename_update_after_the_wildcard_test() {
    let mut port = filename(0);
    port.split = 4;
    filename_set(&mut port);
    assert_eq!(port.events, vec![Event::Split(PAYLOAD + 4), Event::Wildcard(PAYLOAD + 4), Event::Complete(CALLBACK_WORD, translated(4))]);
}

#[test]
fn the_filename_update_refuses_without_a_context_and_cancels_on_the_filename_byte() {
    let mut port = filename(0);
    port.context = false;
    filename_set(&mut port);
    assert_eq!(port.events, vec![Event::Complete(CALLBACK_WORD, translated(2))]);
    assert_eq!(port.word_at(0), CALLBACK_WORD);
    let mut port = filename(0);
    port.cancelled = Some(Cancel::Filename);
    filename_set(&mut port);
    assert_eq!(port.events, vec![Event::Complete(CALLBACK_WORD, 11)]);
    assert_eq!(port.word_at(0), 0);
}

// Attribute update ------------------------------------------------------------------------------

fn attribute(selector: u8) -> Fake {
    let mut port = Fake::new().path(b"B:/DCIM/100_FUJI/DSCF3097.JPG");
    port.payload[0x34..0x38].copy_from_slice(&0x21u32.to_le_bytes());
    port.payload[0x38] = selector;
    port
}

#[test]
fn selector_zero_updates_the_entry_with_the_value_and_delivers_the_reload_result() {
    let mut port = attribute(0);
    port.reload = 13;
    attribute_set(&mut port);
    assert_eq!(&port.events[3..], &[Event::Update(EntryService::Update, 0x4000, NAME.to_vec(), 1, 0x21, 0), Event::Reload, Event::Complete(CALLBACK_WORD, translated(13))]);
    assert_eq!(port.word_at(0), CALLBACK_WORD);
}

#[test]
fn a_failed_selector_zero_update_still_reloads_and_delivers_the_update_result() {
    let mut port = attribute(0);
    port.update = 9;
    port.reload = 13;
    attribute_set(&mut port);
    assert_eq!(&port.events[3..], &[Event::Update(EntryService::Update, 0x4000, NAME.to_vec(), 1, 0x21, 0), Event::Reload, Event::Complete(CALLBACK_WORD, translated(9))]);
}

#[test]
fn selector_one_updates_through_the_second_service_without_a_reload() {
    let mut port = attribute(1);
    port.update = 9;
    attribute_set(&mut port);
    assert_eq!(&port.events[3..], &[Event::Update(EntryService::UpdateOne, 0x4000, NAME.to_vec(), 1, 0x21, 0), Event::Complete(CALLBACK_WORD, translated(9))]);
}

#[test]
fn other_selectors_and_unnumbered_paths_update_nothing() {
    let mut port = attribute(2);
    attribute_set(&mut port);
    assert_eq!(&port.events[3..], &[Event::Complete(CALLBACK_WORD, 0)]);
    let mut port = attribute(1);
    port.number = 99;
    attribute_set(&mut port);
    assert_eq!(&port.events[3..], &[Event::Complete(CALLBACK_WORD, 0)]);
    let mut port = attribute(0);
    port.record = 0;
    port.reload = 13;
    attribute_set(&mut port);
    assert_eq!(&port.events[3..], &[Event::Reload, Event::Complete(CALLBACK_WORD, translated(13))]);
}

#[test]
fn the_attribute_update_cancels_on_the_metadata_byte_only() {
    let mut port = attribute(0);
    port.cancelled = Some(Cancel::Metadata);
    attribute_set(&mut port);
    assert_eq!(port.events, vec![Event::Complete(CALLBACK_WORD, 11)]);
    let mut port = attribute(0);
    port.cancelled = Some(Cancel::Filename);
    attribute_set(&mut port);
    assert_eq!(port.events[0], Event::Split(PAYLOAD + 4));
}

// Image remove ----------------------------------------------------------------------------------

const IMAGE: &[u8] = b"b:/dcim/100_fuji/dscf3097.jpg";
const IMAGE_UPPER: &[u8] = b"B:/DCIM/100_FUJI/DSCF3097.JPG";

fn remove(path: &[u8]) -> Fake {
    Fake::new().path(path)
}

fn removal_events() -> Vec<Event> {
    vec![
        Event::DeviceState(1),
        Event::Split(PAYLOAD + 4),
        Event::Number(DIRECTORY.to_vec(), 4),
        Event::FileNumber(NAME.to_vec()),
        Event::Wildcard(PAYLOAD + 4),
        Event::Find,
        Event::ExtensionId(PAYLOAD + 4 + 26),
        Event::ExtensionKind(2),
        Event::Delete(PAYLOAD + 4),
        Event::Rebuild(Rebuild::Completed),
    ]
}

#[test]
fn a_removable_image_is_uppercased_deleted_and_the_rebuild_result_delivered_with_the_callback_word_clear() {
    let mut port = remove(IMAGE);
    port.rebuild = 9;
    assert_eq!(image_remove(&mut port), 0);
    let mut expected = removal_events();
    expected.push(Event::Complete(CALLBACK_WORD, translated(9)));
    assert_eq!(port.events, expected);
    assert_eq!(&port.path_bytes()[..IMAGE.len()], IMAGE_UPPER);
    assert_eq!(port.word_at(0), 0);
}

#[test]
fn uppercasing_stops_after_the_terminator_or_forty_eight_bytes() {
    let mut port = remove(b"b:/a.jpg\0xyz");
    image_remove(&mut port);
    assert_eq!(&port.path_bytes()[..12], b"B:/A.JPG\0xyz");
    let mut port = remove(&[b'q'; 0x30]);
    port.payload[0x34] = b'q';
    image_remove(&mut port);
    assert_eq!(port.path_bytes(), &[b'Q'; 0x30]);
    assert_eq!(port.payload[0x34], b'q');
}

#[test]
fn a_path_without_a_drive_letter_and_colon_is_refused_before_any_service() {
    for path in [&b"@:/A.JPG"[..], b"[:/A.JPG", b"B/A.JPG"] {
        let mut port = remove(path);
        image_remove(&mut port);
        assert_eq!(port.events, vec![Event::Complete(CALLBACK_WORD, translated(4))]);
    }
}

#[test]
fn the_device_state_refuses_in_bit_order() {
    for (state, raw) in [(0x1f, 1), (0x02, 2), (0x08, 2), (0x10, 2), (0x04, 18)] {
        let mut port = remove(IMAGE);
        port.state = state;
        image_remove(&mut port);
        assert_eq!(port.events, vec![Event::DeviceState(1), Event::Complete(CALLBACK_WORD, translated(raw))], "{state:#x}");
    }
}

#[test]
fn each_catalog_check_refuses_with_four() {
    let cases: [(fn(&mut Fake), usize); 6] = [
        (|port| port.number = 0, 4),
        (|port| port.file = 0, 4),
        (|port| port.wildcard = 1, 5),
        (|port| port.entry.0 = 0x68, 6),
        (|port| port.entry.1 = 3098, 6),
        (|port| port.extension = 0, 7),
    ];
    for (index, (change, calls)) in cases.into_iter().enumerate() {
        let mut port = remove(IMAGE);
        change(&mut port);
        image_remove(&mut port);
        assert_eq!(port.events[..calls], removal_events()[..calls], "case {index}");
        assert_eq!(port.events[calls..], [Event::Complete(CALLBACK_WORD, translated(4))], "case {index}");
    }
}

#[test]
fn split_find_and_delete_failures_deliver_their_results() {
    let mut port = remove(IMAGE);
    port.split = 5;
    image_remove(&mut port);
    assert_eq!(port.events[2..], [Event::Complete(CALLBACK_WORD, translated(5))]);
    let mut port = remove(IMAGE);
    port.found = 8;
    image_remove(&mut port);
    assert_eq!(port.events[6..], [Event::Complete(CALLBACK_WORD, translated(8))]);
    let mut port = remove(IMAGE);
    port.delete = 9;
    image_remove(&mut port);
    assert_eq!(port.events[9..], [Event::Complete(CALLBACK_WORD, translated(9))]);
}

#[test]
fn the_entry_flags_refuse_protected_unkinded_and_read_only_entries() {
    for (flags, raw) in [((0x80, 1, 0), 4), ((0, 0x10, 0), 4), ((0, 1, 1), 17)] {
        let mut port = remove(IMAGE);
        (port.entry.2, port.entry.3, port.entry.4) = flags;
        image_remove(&mut port);
        assert_eq!(port.events[..8], removal_events()[..8]);
        assert_eq!(port.events[8..], [Event::Complete(CALLBACK_WORD, translated(raw))], "{flags:?}");
    }
}

#[test]
fn an_unknown_extension_kind_refuses_with_four() {
    let mut port = remove(IMAGE);
    port.kind = 0;
    image_remove(&mut port);
    assert_eq!(port.events[8..], [Event::Complete(CALLBACK_WORD, translated(4))]);
}

#[test]
fn a_dot_outside_the_last_five_bytes_reads_the_extension_of_the_whole_path_and_refuses() {
    let mut port = remove(b"b:/dcim/100_fuji/dscf3097.jpeg");
    image_remove(&mut port);
    assert_eq!(port.events[6..], [Event::ExtensionId(PAYLOAD + 4), Event::Complete(CALLBACK_WORD, translated(4))]);
}

#[test]
fn a_path_of_four_bytes_is_refused_after_the_catalog_search() {
    let mut port = remove(b"B:/A");
    image_remove(&mut port);
    assert_eq!(port.events[5..], [Event::Find, Event::Complete(CALLBACK_WORD, translated(4))]);
}

#[test]
fn the_image_remove_without_a_context_delivers_the_translated_two() {
    let mut port = remove(IMAGE);
    port.context = false;
    image_remove(&mut port);
    assert_eq!(port.completions(), vec![Event::Complete(CALLBACK_WORD, translated(2))]);
    assert_eq!(&port.path_bytes()[..IMAGE.len()], IMAGE);
    assert_eq!(port.word_at(0), 0);
}

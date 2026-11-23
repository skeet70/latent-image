use crate::machine::{self, Extension, Fault, Guest, Message, OpenMode, Request, Task, Worker, EXTENSIONS, PENDING, RECORD, SAVED_PATH};
use std::collections::BTreeMap;

#[derive(Debug, PartialEq, Eq)]
enum Call {
    Register(Worker),
    Unregister,
    Post(Message, u32, u32),
    Request(Request),
    Report(Fault),
    PrepareName(u32, Option<[u8; 5]>),
    PrefixMode(u32),
    Complete(u32, i32),
    Run(u32),
}

const HANDLE: u32 = 0x4a11;
const DESCRIPTOR: u32 = 0x0040_0000;
const MESSAGE: u32 = 0x0050_0000;
const COMPLETION: u32 = 0x00e4_e5dc;

/// Byte-addressed guest memory; unwritten bytes read as zero. Strings and copies act on it.
#[derive(Default)]
struct Sim {
    memory: BTreeMap<u32, u8>,
    calls: Vec<Call>,
}

impl Sim {
    fn put(&mut self, address: u32, bytes: &[u8]) {
        for (index, byte) in bytes.iter().enumerate() {
            self.memory.insert(address + index as u32, *byte);
        }
    }
    fn bytes(&self, address: u32, length: u32) -> Vec<u8> {
        (0..length).map(|index| self.r8(address + index)).collect()
    }
    fn text(&self, address: u32) -> String {
        let length = (0..).find(|index| self.r8(address + index) == 0).unwrap();
        String::from_utf8(self.bytes(address, length)).unwrap()
    }
    /// A started record whose current message carries `status`.
    fn started(status: u32) -> Sim {
        let mut sim = Sim::default();
        machine::begin(&mut sim, Worker(1), DESCRIPTOR, COMPLETION, false);
        sim.w32(RECORD + 0x1c, RECORD);
        sim.w32(RECORD + 0x18, MESSAGE);
        sim.w32(MESSAGE + 4, status);
        sim.w32(DESCRIPTOR, HANDLE);
        sim.calls.clear();
        sim
    }
}

impl Guest for Sim {
    fn r8(&self, address: u32) -> u8 {
        *self.memory.get(&address).unwrap_or(&0)
    }
    fn r32(&self, address: u32) -> u32 {
        u32::from_le_bytes([self.r8(address), self.r8(address + 1), self.r8(address + 2), self.r8(address + 3)])
    }
    fn w8(&mut self, address: u32, value: u8) {
        self.memory.insert(address, value);
    }
    fn w32(&mut self, address: u32, value: u32) {
        self.put(address, &value.to_le_bytes())
    }
    fn register(&mut self, _: u32, worker: Worker) {
        self.calls.push(Call::Register(worker));
    }
    fn unregister(&mut self, _: u32) {
        self.calls.push(Call::Unregister);
    }
    fn post(&mut self, _: u32, message: Message, word2: u32, word3: u32) {
        self.calls.push(Call::Post(message, word2, word3));
    }
    fn request(&mut self, request: Request) -> u32 {
        self.calls.push(Call::Request(request));
        HANDLE
    }
    fn report(&mut self, fault: Fault) {
        self.calls.push(Call::Report(fault));
    }
    fn prepare_name(&mut self, name: u32, prefix: Option<[u8; 5]>) {
        self.calls.push(Call::PrepareName(name, prefix));
        self.put(name, b"L:\\DCIM\\100_FUJI\\DSCF0001\0");
    }
    fn set_prefix_mode(&mut self, mode: u32) {
        self.calls.push(Call::PrefixMode(mode));
    }
    fn property(&mut self, key: u32) -> u8 {
        key as u8
    }
    fn copy_string(&mut self, destination: u32, source: u32) {
        let mut bytes = self.text(source).into_bytes();
        bytes.push(0);
        self.put(destination, &bytes);
    }
    fn append_extension(&mut self, destination: u32, extension: Extension) {
        let end = destination + self.text(destination).len() as u32;
        self.put(end, &EXTENSIONS[extension as usize][..5]);
    }
    fn copy_memory(&mut self, destination: u32, source: u32, length: u32) {
        let bytes = self.bytes(source, length);
        self.put(destination, &bytes);
    }
    fn complete(&mut self, completion: u32, result: i32) {
        self.calls.push(Call::Complete(completion, result));
    }
    fn run(&mut self, pending: u32) {
        self.calls.push(Call::Run(pending));
    }
}

fn entry(index: u32) -> u32 {
    DESCRIPTOR + 8 + 8 * index
}

#[test]
fn start_registers_and_posts_the_start_message() {
    let mut sim = Sim::default();
    machine::begin(&mut sim, Worker(1), DESCRIPTOR, COMPLETION, true);
    assert_eq!(sim.calls, vec![Call::Register(Worker(1)), Call::Post(Message::Start, 0, 0)]);
    assert_eq!(sim.r32(DESCRIPTOR + 0x14c), 1);
    assert_eq!(sim.r32(RECORD + 0x20), COMPLETION);
    assert_eq!(sim.r32(RECORD + 0x28), DESCRIPTOR);
}

#[test]
fn a_new_jpeg_is_named_and_opened_for_create() {
    let mut sim = Sim::started(0);
    sim.w32(DESCRIPTOR + 0x88, 5);
    machine::open(&mut sim, Task(RECORD));
    let name = DESCRIPTOR + 0x8c;
    assert_eq!(sim.text(name), "L:\\DCIM\\100_FUJI\\DSCF0001.JPG");
    assert_eq!(sim.calls, vec![Call::PrepareName(name, None), Call::Request(Request::Open { name, mode: OpenMode::Create })]);
    assert_eq!(sim.r32(DESCRIPTOR + 0x88), 0);
    assert_eq!(sim.r32(RECORD + 0xc), 1);
}

#[test]
fn the_prefix_source_names_with_the_code_properties() {
    let mut sim = Sim::started(0);
    sim.w32(DESCRIPTOR + 0x148, 1);
    machine::open(&mut sim, Task(RECORD));
    assert_eq!(sim.calls[0], Call::PrepareName(DESCRIPTOR + 0x8c, Some([0x4f, 0x50, 0x51, 0x52, 0])));
}

#[test]
fn an_append_opens_the_existing_name_and_seeks_before_writing() {
    let mut sim = Sim::started(0);
    sim.w32(DESCRIPTOR + 0x14c, 1);
    sim.w32(DESCRIPTOR + 4, 3);
    machine::open(&mut sim, Task(RECORD));
    machine::opened(&mut sim, Task(RECORD));
    assert_eq!(
        sim.calls,
        vec![
            Call::Request(Request::Open { name: DESCRIPTOR + 0x8c, mode: OpenMode::Append }),
            Call::Request(Request::Seek { handle: HANDLE }),
        ]
    );
}

#[test]
fn writes_follow_the_scatter_list_and_close_at_its_end() {
    let mut sim = Sim::started(0);
    sim.w32(entry(0), 0x1000);
    sim.w32(entry(0) + 4, 0x20);
    sim.w32(entry(1), 0x2000);
    sim.w32(entry(1) + 4, 0x30);
    machine::opened(&mut sim, Task(RECORD));
    machine::written(&mut sim, Task(RECORD));
    machine::written(&mut sim, Task(RECORD));
    assert_eq!(
        sim.calls,
        vec![
            Call::Request(Request::Write { handle: HANDLE, size: 0x20, data: 0x1000 }),
            Call::Request(Request::Write { handle: HANDLE, size: 0x30, data: 0x2000 }),
            Call::Request(Request::Close { handle: HANDLE }),
        ]
    );
    assert_eq!(sim.r32(RECORD + 0xc), 2);
}

#[test]
fn a_prepare_status_fails_the_open_with_minus_four() {
    let mut sim = Sim::started(3);
    machine::opened(&mut sim, Task(RECORD));
    assert_eq!(sim.calls, vec![Call::Unregister, Call::Report(Fault::Open), Call::Complete(COMPLETION, -4)]);
    assert_eq!(sim.r32(RECORD + 0xc), 0);
}

#[test]
fn a_failed_write_releases_and_completes_after_the_close() {
    let mut sim = Sim::started(5);
    machine::written(&mut sim, Task(RECORD));
    assert_eq!(sim.calls, vec![Call::Report(Fault::Transfer), Call::Request(Request::Release { handle: HANDLE })]);
    sim.w32(MESSAGE + 4, 0);
    sim.calls.clear();
    machine::closed(&mut sim, Task(RECORD));
    assert_eq!(sim.calls, vec![Call::Unregister, Call::Complete(COMPLETION, -2)]);
}

#[test]
fn a_closed_file_saves_its_path() {
    let mut sim = Sim::started(0);
    sim.put(DESCRIPTOR + 0x8c, b"L:\\A.JPG\0");
    machine::closed(&mut sim, Task(RECORD));
    assert_eq!(sim.text(SAVED_PATH), "L:\\A.JPG");
    assert_eq!(sim.calls, vec![Call::Unregister, Call::Complete(COMPLETION, 0)]);
}

#[test]
fn a_cancel_while_idle_deletes_the_saved_file_then_runs_the_pending_call() {
    let mut sim = Sim::started(0);
    sim.put(SAVED_PATH, b"L:\\A.JPG\0");
    machine::request_cancel(&mut sim, 0x1234);
    machine::cancel_idle(&mut sim, Task(RECORD));
    machine::deleted_idle(&mut sim, Task(RECORD));
    assert_eq!(
        sim.calls,
        vec![
            Call::Register(Worker::CANCEL),
            Call::Post(Message::Cancel, 0, 0),
            Call::Request(Request::Delete { path: SAVED_PATH }),
            Call::Unregister,
            Call::Run(0x1234),
        ]
    );
    assert_eq!(sim.r8(SAVED_PATH), 0);
    assert_eq!(sim.r32(PENDING), 0);
}

#[test]
fn append_fills_the_first_free_entry_below_the_last() {
    let mut sim = Sim::default();
    sim.w32(entry(0), 1);
    sim.w32(entry(1) + 4, 1);
    sim.w32(DESCRIPTOR + 0x88, 9);
    assert_eq!(machine::append_buffer(&mut sim, DESCRIPTOR, 0xaa, 0xbb), 0);
    assert_eq!([sim.r32(entry(2)), sim.r32(entry(2) + 4), sim.r32(DESCRIPTOR + 0x88)], [0xaa, 0xbb, 0]);
    for index in 3..15 {
        sim.w32(entry(index), 1);
    }
    assert_eq!(machine::append_buffer(&mut sim, DESCRIPTOR, 0xcc, 1), -1);
}

#[test]
fn set_name_cuts_at_the_dot_and_appends_the_kind_extension() {
    let mut sim = Sim::default();
    sim.w32(DESCRIPTOR + 4, 3);
    sim.put(0x600, b"L:\\DCIM\\X.TMP\0");
    machine::set_name(&mut sim, 0x600, DESCRIPTOR);
    assert_eq!(sim.text(DESCRIPTOR + 0x8c), "L:\\DCIM\\X.RAF");
    assert_eq!(sim.r32(DESCRIPTOR + 4), 7);
}

#[test]
fn set_name_leaves_the_descriptor_for_a_late_dot() {
    let mut sim = Sim::default();
    let mut text = vec![b'A'; 44];
    text.push(b'.');
    sim.put(0x600, &text);
    machine::set_name(&mut sim, 0x600, DESCRIPTOR);
    assert_eq!(sim.r32(DESCRIPTOR + 4), 0);
    assert_eq!(sim.r8(0x600 + 44), 0);
}

#[test]
fn output_modes_map_to_prefix_modes() {
    let mut sim = Sim::default();
    let results: Vec<u32> = (0..5).map(|mode| machine::set_output_mode(&mut sim, mode)).collect();
    assert_eq!(results, vec![1, 1, 1, 1, 0]);
    assert_eq!(sim.calls, vec![Call::PrefixMode(0), Call::PrefixMode(0), Call::PrefixMode(1), Call::PrefixMode(2)]);
    assert_eq!(machine::output_mode(&sim), 3);
}

use crate::completion::*;
use crate::routing::*;
use std::collections::HashMap;

#[derive(Debug, PartialEq)]
enum Event {
    Send(u16, u8, u8),
    Sleep,
    Wake(u8),
    Complete(u32, u32, u32),
}

struct Model {
    memory: HashMap<u32, u8>,
    class: u32,
    open_result: (u32, u32),
    open_count: u32,
    saved: u32,
    open_set: [u32; 20],
    deferred: Vec<Option<(Vec<u8>, Vec<u8>)>>,
    deferred_count: u32,
    sleeps: Vec<(i32, u32, u32)>,
    send_result: i32,
    events: Vec<Event>,
    scratch: u32,
}

impl Model {
    fn new() -> Model {
        Model {
            memory: HashMap::new(),
            class: 0,
            open_result: (0, 0),
            open_count: 0,
            saved: 0,
            open_set: [0; 20],
            deferred: vec![None; 48],
            deferred_count: 0,
            sleeps: Vec::new(),
            send_result: 0,
            events: Vec::new(),
            scratch: 0x8000,
        }
    }
    fn put(&mut self, address: u32, bytes: &[u8]) {
        for (offset, byte) in bytes.iter().enumerate() {
            self.memory.insert(address + offset as u32, *byte);
        }
    }
    fn get(&self, address: u32, length: u32) -> Vec<u8> {
        (0..length).map(|offset| *self.memory.get(&(address + offset)).unwrap_or(&0)).collect()
    }
    /// A request at `header` with code `code` and an 8-byte payload {callback, handle}.
    fn request(&mut self, header: u32, code: u16, callback: u32, handle: u32) -> Header {
        let payload = header + 0x40;
        self.put(header, &code.to_le_bytes());
        self.put(header + 2, &[0, 0]);
        self.put(header + 4, &8u32.to_le_bytes());
        self.put(header + 8, &payload.to_le_bytes());
        self.put(payload, &callback.to_le_bytes());
        self.put(payload + 4, &handle.to_le_bytes());
        Header(header)
    }
}

impl Mediamsg for Model {
    fn code(&self, header: Header) -> RequestCode {
        let bytes = self.get(header.0, 2);
        RequestCode(u16::from_le_bytes([bytes[0], bytes[1]]))
    }
    fn set_code(&mut self, header: Header, code: RequestCode) {
        self.put(header.0, &code.0.to_le_bytes())
    }
    fn set_route(&mut self, header: Header, source: u8, destination: u8) {
        self.put(header.0 + 2, &[source, destination])
    }
    fn payload(&self, header: Header) -> u32 {
        self.word(header.0 + 8)
    }
    fn word(&self, address: u32) -> u32 {
        let bytes = self.get(address, 4);
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }
    fn set_word(&mut self, address: u32, value: u32) {
        self.put(address, &value.to_le_bytes())
    }
    fn class(&self) -> SessionClass {
        SessionClass(self.class)
    }
    fn set_class(&mut self, class: SessionClass) {
        self.class = class.0
    }
    fn open_result(&self) -> (u32, u32) {
        self.open_result
    }
    fn set_open_result(&mut self, status: u32, handle: u32) {
        self.open_result = (status, handle)
    }
    fn open_count(&self) -> u32 {
        self.open_count
    }
    fn set_open_count(&mut self, count: u32) {
        self.open_count = count
    }
    fn saved_callback(&self) -> u32 {
        self.saved
    }
    fn set_saved_callback(&mut self, callback: u32) {
        self.saved = callback
    }
    fn open_handle(&self, slot: usize) -> u32 {
        self.open_set[slot]
    }
    fn set_open_handle(&mut self, slot: usize, handle: u32) {
        self.open_set[slot] = handle
    }
    fn clear_open_set(&mut self) {
        self.open_set = [0; 20]
    }
    fn deferred_count(&self) -> u32 {
        self.deferred_count
    }
    fn set_deferred_count(&mut self, count: u32) {
        self.deferred_count = count
    }
    fn deferred_code(&self, slot: usize) -> RequestCode {
        match &self.deferred[slot] {
            Some((header, _)) => RequestCode(u16::from_le_bytes([header[0], header[1]])),
            None => RequestCode(0),
        }
    }
    fn store_deferred(&mut self, slot: usize, header: Header) {
        let length = self.word(header.0 + 4);
        let payload = self.get(self.payload(header), length);
        self.deferred[slot] = Some((self.get(header.0, 12), payload));
    }
    fn replay(&mut self, slot: usize, route: fn(&mut Self, Header) -> i32) -> i32 {
        let (header, payload) = self.deferred[slot].take().unwrap();
        let mut bytes = header;
        bytes.resize(16, 0);
        bytes.extend(payload);
        bytes.resize(96, 0);
        let address = self.scratch;
        self.scratch += 0x100;
        self.put(address, &bytes);
        self.set_word(address + 8, address + 16);
        route(self, Header(address))
    }
    fn interceptor(&self) -> u32 {
        0x7217f0
    }
    fn send(&mut self, task: u8, header: Header) -> i32 {
        let bytes = self.get(header.0, 4);
        self.events.push(Event::Send(u16::from_le_bytes([bytes[0], bytes[1]]), bytes[2], bytes[3]));
        assert_eq!(task, MEDIA_TASK);
        self.send_result
    }
    fn sleep(&mut self) -> i32 {
        self.events.push(Event::Sleep);
        let (status, open_status, handle) = self.sleeps.remove(0);
        self.open_result = (open_status, handle);
        status
    }
    fn wake(&mut self, task: u8) {
        self.events.push(Event::Wake(task))
    }
    fn complete(&mut self, callback: u32, status: u32, handle: u32) {
        self.events.push(Event::Complete(callback, status, handle))
    }
}

#[test]
fn translation_follows_the_raw_status_table() {
    assert_eq!(translate(RawStatus(0)), Translation { status: ClientStatus(0), report: None });
    assert_eq!(translate(RawStatus(1)), Translation { status: ClientStatus(1), report: Some(MediaReport(0x50)) });
    assert_eq!(translate(RawStatus(14)).status, ClientStatus(3));
    assert_eq!(translate(RawStatus(33)), Translation { status: ClientStatus(13), report: Some(MediaReport(0x54)) });
    assert_eq!(translate(RawStatus(38)).report, Some(MediaReport(0x5b)));
    assert_eq!(translate(RawStatus(39)), Translation { status: ClientStatus(5), report: None });
    assert_eq!(translate(RawStatus(u32::MAX)).status, ClientStatus(5));
}

#[test]
fn readiness_takes_the_first_flag_in_bit_order() {
    let mode = ReadinessMode(0);
    assert_eq!(readiness(Media::Absent, mode), Translation { status: ClientStatus(5), report: None });
    assert_eq!(readiness(Media::Present { flags: 0b1111 }, mode).report, Some(MediaReport(0x50)));
    assert_eq!(readiness(Media::Present { flags: 0b0010 }, ReadinessMode(2)).status, ClientStatus(0));
    assert_eq!(readiness(Media::Present { flags: 0b0010 }, ReadinessMode(1)).report, Some(MediaReport(0x55)));
    assert_eq!(readiness(Media::Present { flags: 0b1000 }, mode), Translation { status: ClientStatus(2), report: Some(MediaReport(0x52)) });
    assert_eq!(readiness(Media::Present { flags: 0b1000 }, ReadinessMode(1)).status, ClientStatus(0));
    assert_eq!(readiness(Media::Present { flags: 0 }, mode).status, ClientStatus(0));
}

#[test]
fn alternate_codes_reach_the_media_task_as_primary_codes() {
    let mut model = Model::new();
    let header = model.request(0x1000, 0x2c2e, 0, 3);
    assert_eq!(route(&mut model, header), 0);
    assert_eq!(model.events, [Event::Send(0x2c18, MEDIAMSG_TASK, MEDIA_TASK)]);
}

#[test]
fn an_open_records_its_handle_and_session_class() {
    let mut model = Model::new();
    model.sleeps.push((0, 0, 0x77));
    let header = model.request(0x1000, 0x2c17, 0x5000, 0);
    assert_eq!(route(&mut model, header), 0);
    assert_eq!(model.saved, 0x5000);
    assert_eq!(model.word(0x1040), 0x7217f0);
    assert_eq!(model.open_set[0], 0x77);
    assert_eq!(model.open_count, 1);
    assert_eq!(model.class, 2);
}

#[test]
fn a_failed_open_leaves_the_session_alone() {
    let mut model = Model::new();
    model.sleeps.push((0, 4, 0));
    let header = model.request(0x1000, 0x2c2d, 0, 0);
    assert_eq!(route(&mut model, header), 0);
    assert_eq!((model.class, model.open_count), (0, 0));
}

#[test]
fn a_failed_sleep_refuses_the_open() {
    let mut model = Model::new();
    model.sleeps.push((1, 0, 0x77));
    let header = model.request(0x1000, 0x2c17, 0, 0);
    assert_eq!(route(&mut model, header), -255);
    assert_eq!(model.class, 0);
}

#[test]
fn requests_of_the_other_class_wait_and_replay_when_the_last_handle_closes() {
    let mut model = Model::new();
    model.sleeps.push((0, 0, 0x77));
    let open = model.request(0x1000, 0x2c17, 0, 0);
    route(&mut model, open);
    let alternate_read = model.request(0x1100, 0x2c2e, 0, 0x77);
    assert_eq!(route(&mut model, alternate_read), 0);
    assert_eq!(model.deferred_count, 1);
    model.events.clear();
    let close = model.request(0x1200, 0x2c1a, 0, 0x77);
    assert_eq!(route(&mut model, close), 0);
    assert_eq!(
        model.events,
        [Event::Send(0x2c1a, MEDIAMSG_TASK, MEDIA_TASK), Event::Send(0x2c18, MEDIAMSG_TASK, MEDIA_TASK)]
    );
    assert_eq!((model.class, model.open_count, model.deferred_count), (0, 0, 0));
}

#[test]
fn a_close_with_handles_still_open_keeps_the_class() {
    let mut model = Model::new();
    model.class = 2;
    model.open_set[0] = 5;
    model.open_set[1] = 6;
    model.open_count = 2;
    let close = model.request(0x1000, 0x2c1a, 0, 5);
    route(&mut model, close);
    assert_eq!((model.class, model.open_count, model.open_set[0]), (2, 1, 0));
}

#[test]
fn a_refused_send_is_reported_and_skips_the_bookkeeping() {
    let mut model = Model::new();
    model.send_result = 3;
    model.class = 2;
    model.open_set[0] = 5;
    model.open_count = 1;
    let close = model.request(0x1000, 0x2c1a, 0, 5);
    assert_eq!(route(&mut model, close), -255);
    assert_eq!(model.open_count, 1);
}

#[test]
fn detach_clears_the_session_even_when_its_send_fails() {
    let mut model = Model::new();
    model.send_result = 1;
    model.class = 2;
    model.open_set[3] = 9;
    model.open_count = 1;
    model.saved = 0x44;
    let detach = model.request(0x1000, 0x2c01, 0, 0);
    assert_eq!(route(&mut model, detach), -255);
    assert_eq!((model.class, model.open_count, model.saved, model.open_set[3]), (0, 0, 0, 0));
}

#[test]
fn detach_waits_behind_an_alternate_session() {
    let mut model = Model::new();
    model.class = 1;
    let detach = model.request(0x1000, 0x2c01, 0, 0);
    assert_eq!(route(&mut model, detach), 0);
    assert_eq!(model.deferred_count, 1);
    assert!(model.events.is_empty());
}

#[test]
fn a_full_deferred_queue_refuses() {
    let mut model = Model::new();
    model.class = 1;
    for slot in 0..48 {
        model.deferred[slot] = Some((vec![1, 0x2c, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], vec![]));
    }
    let read = model.request(0x1000, 0x2c18, 0, 0);
    assert_eq!(route(&mut model, read), -255);
}

#[test]
fn the_interceptor_records_completes_and_wakes() {
    let mut model = Model::new();
    model.saved = 0x5000;
    intercept(&mut model, 3, 0x21);
    assert_eq!(model.open_result, (3, 0x21));
    assert_eq!(model.saved, 0);
    assert_eq!(model.events, [Event::Complete(0x5000, 3, 0x21), Event::Wake(MEDIAMSG_TASK)]);
}

#[test]
fn the_worker_table_holds_the_open_worker_first_and_no_unknown_code() {
    assert_eq!(worker(RequestCode::OPEN), Some(0x7115a4));
    assert_eq!(worker(RequestCode(0x2c26)), Some(0x715a58));
    assert_eq!(worker(RequestCode(0x2c2d)), None);
    assert_eq!(worker(RequestCode(0)), None);
}

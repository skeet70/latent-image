//! The two request tasks: mediamsg (task 0x65) serializes opens by session class and
//! forwards each request to the media task (task 0x1c), which runs the request's worker.

/// A request header in guest memory: code u16 at +0, source task u8 at +2, destination
/// task u8 at +3, payload length u32 at +4, payload address u32 at +8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestCode(pub u16);

impl RequestCode {
    pub const ATTACH: RequestCode = RequestCode(0x2c00);
    pub const DETACH: RequestCode = RequestCode(0x2c01);
    pub const OPEN: RequestCode = RequestCode(0x2c17);
    pub const READ: RequestCode = RequestCode(0x2c18);
    pub const WRITE: RequestCode = RequestCode(0x2c19);
    pub const CLOSE: RequestCode = RequestCode(0x2c1a);
    pub const CLOSE_WITH_ATTRIBUTES: RequestCode = RequestCode(0x2c1b);
    pub const RELEASE: RequestCode = RequestCode(0x2c1d);
    pub const ALTERNATE_OPEN: RequestCode = RequestCode(0x2c2d);
    pub const ALTERNATE_CLOSE: RequestCode = RequestCode(0x2c30);

    /// The primary code the media task receives for an alternate code.
    pub fn primary(self) -> RequestCode {
        match self.0 {
            0x2c2d => RequestCode(0x2c17),
            0x2c2e => RequestCode(0x2c18),
            0x2c2f => RequestCode(0x2c19),
            0x2c30 => RequestCode(0x2c1a),
            0x2c31 => RequestCode(0x2c1c),
            0x2c32 => RequestCode(0x2c1e),
            0x2c33 => RequestCode(0x2c08),
            0x2c34 => RequestCode(0x2c13),
            _ => self,
        }
    }

    fn alternate(self) -> bool {
        (0x2c2d..=0x2c34).contains(&self.0)
    }
}

/// Which open requests own the media: none, alternate-code opens or primary-code opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionClass(pub u32);

impl SessionClass {
    pub const IDLE: SessionClass = SessionClass(0);
    pub const ALTERNATE: SessionClass = SessionClass(1);
    pub const PRIMARY: SessionClass = SessionClass(2);
}

pub const MEDIAMSG_TASK: u8 = 0x65;
pub const MEDIA_TASK: u8 = 0x1c;
pub const OPEN_SET_SLOTS: usize = 20;
pub const DEFERRED_SLOTS: usize = 48;

/// The stock worker table the media task searches: eight-byte rows of code then worker.
pub const WORKER_TABLE: u32 = 0x814740;

/// The 34 request workers, in the order of their rows in `WORKER_TABLE`.
pub const WORKERS: [(RequestCode, u32); 34] = [
    (RequestCode(0x2c17), 0x7115a4),
    (RequestCode(0x2c18), 0x71edbc),
    (RequestCode(0x2c19), 0x7116e4),
    (RequestCode(0x2c1a), 0x711768),
    (RequestCode(0x2c1b), 0x711840),
    (RequestCode(0x2c1c), 0x71eca4),
    (RequestCode(0x2c1d), 0x71f134),
    (RequestCode(0x2c1e), 0x71ded4),
    (RequestCode(0x2c0a), 0x7118fc),
    (RequestCode(0x2c0b), 0x711b50),
    (RequestCode(0x2c11), 0x7123b4),
    (RequestCode(0x2c0c), 0x711ea4),
    (RequestCode(0x2c06), 0x71e3a4),
    (RequestCode(0x2c07), 0x71e550),
    (RequestCode(0x2c08), 0x712700),
    (RequestCode(0x2c09), 0x7127e0),
    (RequestCode(0x2c13), 0x71e828),
    (RequestCode(0x2c14), 0x71e8c4),
    (RequestCode(0x2c00), 0x7120d8),
    (RequestCode(0x2c01), 0x7122dc),
    (RequestCode(0x2c02), 0x71a8a0),
    (RequestCode(0x2c03), 0x7119b0),
    (RequestCode(0x2c04), 0x711f58),
    (RequestCode(0x2c05), 0x71b9b0),
    (RequestCode(0x2c0d), 0x71b8c0),
    (RequestCode(0x2c0e), 0x718de4),
    (RequestCode(0x2c0f), 0x718a8c),
    (RequestCode(0x2c10), 0x719614),
    (RequestCode(0x2c1f), 0x719098),
    (RequestCode(0x2c25), 0x711bc8),
    (RequestCode(0x2c26), 0x715a58),
    (RequestCode(0x2c2a), 0x715de4),
    (RequestCode(0x2c2b), 0x71a834),
    (RequestCode(0x2c12), 0x712580),
];

/// A request's worker and the index of its row in `WORKERS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkerRow {
    pub row: u32,
    pub worker: u32,
}

pub fn worker_row(code: RequestCode) -> Option<WorkerRow> {
    let mut row = 0;
    for &(known, worker) in WORKERS.iter() {
        if known == code {
            return Some(WorkerRow { row, worker });
        }
        row += 1;
    }
    None
}

pub fn worker(code: RequestCode) -> Option<u32> {
    worker_row(code).map(|found| found.worker)
}

pub trait Mediamsg {
    fn code(&self, header: Header) -> RequestCode;
    fn set_code(&mut self, header: Header, code: RequestCode);
    fn set_route(&mut self, header: Header, source: u8, destination: u8);
    fn payload(&self, header: Header) -> u32;
    fn word(&self, address: u32) -> u32;
    fn set_word(&mut self, address: u32, value: u32);

    fn class(&self) -> SessionClass;
    fn set_class(&mut self, class: SessionClass);
    fn open_result(&self) -> (u32, u32);
    fn set_open_result(&mut self, status: u32, handle: u32);
    fn open_count(&self) -> u32;
    fn set_open_count(&mut self, count: u32);
    fn saved_callback(&self) -> u32;
    fn set_saved_callback(&mut self, callback: u32);
    fn open_handle(&self, slot: usize) -> u32;
    fn set_open_handle(&mut self, slot: usize, handle: u32);
    fn clear_open_set(&mut self);
    fn deferred_count(&self) -> u32;
    fn set_deferred_count(&mut self, count: u32);
    fn deferred_code(&self, slot: usize) -> RequestCode;
    /// Copies the header and `length` payload bytes into the slot and points the copy's
    /// payload at them; a zero length keeps the original payload address.
    fn store_deferred(&mut self, slot: usize, header: Header);
    /// Copies the slot's 96 bytes to a request of its own, clears the slot, points the
    /// copy's payload at the copy's payload bytes and routes the copy.
    fn replay(&mut self, slot: usize, route: fn(&mut Self, Header) -> i32) -> i32;

    /// The interceptor every forwarded open completes through.
    fn interceptor(&self) -> u32;
    fn send(&mut self, task: u8, header: Header) -> i32;
    fn sleep(&mut self) -> i32;
    fn wake(&mut self, task: u8);
    fn complete(&mut self, callback: u32, status: u32, handle: u32);
}

const REFUSED: i32 = -255;

/// Queues a request of the blocked session class until the open class closes.
pub fn defer<P: Mediamsg>(port: &mut P, header: Header) -> i32 {
    let Some(slot) = (0..DEFERRED_SLOTS).find(|&slot| port.deferred_code(slot).0 == 0) else {
        return REFUSED;
    };
    port.store_deferred(slot, header);
    if !cfg!(file_service_control_defer_no_count) {
        port.set_deferred_count(port.deferred_count().wrapping_add(1));
    }
    0
}

/// Addresses the request from mediamsg to the media task, primary code only.
pub fn forward<P: Mediamsg>(port: &mut P, header: Header) -> i32 {
    port.set_route(header, MEDIAMSG_TASK, MEDIA_TASK);
    let code = port.code(header);
    if code.alternate() {
        port.set_code(header, code.primary());
    }
    if port.send(MEDIA_TASK, header) == 0 {
        0
    } else {
        REFUSED
    }
}

fn pass<P: Mediamsg>(port: &mut P, header: Header, blocked: SessionClass) -> i32 {
    if port.class() == blocked {
        defer(port, header)
    } else {
        forward(port, header)
    }
}

fn open<P: Mediamsg>(port: &mut P, header: Header, blocked: SessionClass, class: SessionClass) -> i32 {
    let payload = port.payload(header);
    if port.class() == blocked {
        defer(port, header);
        return 0;
    }
    port.set_saved_callback(port.word(payload));
    port.set_word(payload, port.interceptor());
    let sent = forward(port, header);
    if sent != 0 {
        return sent;
    }
    if port.sleep() != 0 {
        return REFUSED;
    }
    let (status, handle) = port.open_result();
    if status != 0 {
        return 0;
    }
    if !(0..OPEN_SET_SLOTS).any(|slot| port.open_handle(slot) == handle) {
        let Some(slot) = (0..OPEN_SET_SLOTS).find(|&slot| port.open_handle(slot) == 0) else {
            // Stock stops here at 0x7213a0 with more than 20 open handles.
            loop {}
        };
        port.set_open_handle(slot, handle);
        port.set_open_count(port.open_count().wrapping_add(1));
    }
    port.set_class(class);
    0
}

fn close<P: Mediamsg>(port: &mut P, header: Header, blocked: SessionClass, handle: u32) -> i32 {
    if port.class() == blocked {
        defer(port, header);
        return 0;
    }
    let sent = forward(port, header);
    if sent != 0 {
        return sent;
    }
    if let Some(slot) = (0..OPEN_SET_SLOTS).find(|&slot| port.open_handle(slot) == handle) {
        port.set_open_handle(slot, 0);
        port.set_open_count(port.open_count().wrapping_sub(1));
    }
    if port.open_count() != 0 {
        return 0;
    }
    port.set_class(SessionClass::IDLE);
    for slot in 0..DEFERRED_SLOTS {
        let pending = port.deferred_count();
        if pending == 0 {
            break;
        }
        if port.deferred_code(slot).0 != 0 {
            port.set_deferred_count(pending.wrapping_sub(1));
            if port.replay(slot, route) != 0 {
                break;
            }
        }
    }
    0
}

fn detach<P: Mediamsg>(port: &mut P, header: Header) -> i32 {
    if port.class() == SessionClass::ALTERNATE {
        return defer(port, header);
    }
    let sent = forward(port, header);
    port.set_open_result(0, 0);
    port.set_open_count(0);
    port.set_saved_callback(0);
    port.clear_open_set();
    port.set_class(SessionClass::IDLE);
    sent
}

/// mediamsg's routing of one request, by code.
pub fn route<P: Mediamsg>(port: &mut P, header: Header) -> i32 {
    let handle = |port: &P| port.word(port.payload(header).wrapping_add(4));
    match port.code(header).0 {
        0x2c01 => detach(port, header),
        0x2c17 => open(port, header, SessionClass::ALTERNATE, SessionClass::PRIMARY),
        0x2c2d => open(port, header, SessionClass::PRIMARY, SessionClass::ALTERNATE),
        0x2c1a | 0x2c1b | 0x2c1d => {
            let handle = handle(port);
            close(port, header, SessionClass::ALTERNATE, handle)
        }
        0x2c30 => {
            let handle = handle(port);
            close(port, header, SessionClass::PRIMARY, handle)
        }
        0x2c2e | 0x2c2f | 0x2c31..=0x2c34 => pass(port, header, SessionClass::PRIMARY),
        _ => pass(port, header, SessionClass::ALTERNATE),
    }
}

/// The open completion mediamsg installs in place of the caller's: records the result,
/// completes the caller's callback and wakes mediamsg.
pub fn intercept<P: Mediamsg>(port: &mut P, status: u32, handle: u32) {
    port.set_open_result(status, handle);
    let callback = port.saved_callback();
    if callback != 0 {
        port.complete(callback, status, handle);
        if !cfg!(file_service_control_intercept_keeps_callback) {
            port.set_saved_callback(0);
        }
    }
    port.wake(MEDIAMSG_TASK);
}

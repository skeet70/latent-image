//! The caller-facing open, read, write and close requests: the refusals checked before
//! posting, the payload each request carries to mediamsg, and the blocking forms that wait
//! for the worker's result.

use crate::completion::{ReadinessMode, Translation};
use crate::routing::{RequestCode, MEDIA_TASK};

/// A stock completion callback word; zero is none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Callback(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    Posted,
    Blocking,
}

/// Which code family a poster sends: the primary code or its alternate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Primary,
    Alternate,
}

impl Family {
    pub fn from_word(word: u32) -> Family {
        if word == 1 {
            Family::Alternate
        } else {
            Family::Primary
        }
    }
}

impl Wait {
    pub fn from_word(word: u32) -> Wait {
        if word == 0 {
            Wait::Posted
        } else {
            Wait::Blocking
        }
    }
}

/// The result pair a worker fills for a blocking caller: status, then handle or count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResultSlot(pub u32);

/// The request payloads, as the workers read them.
pub enum Payload {
    Open { callback: Callback, path: u32, mode: u32, tag: u32, slot: Option<ResultSlot>, word: u32 },
    Read { callback: Callback, handle: u32, length: u32, buffer: u32, slot: Option<ResultSlot> },
    Write { callback: Callback, handle: u32, second: u32, third: u32, slot: Option<ResultSlot> },
    Close { callback: Callback, handle: u32, word: u32, name: u32, slot: Option<ResultSlot> },
}

pub trait Posting {
    fn current_task(&mut self) -> u32;
    /// The local-mode record's mode word.
    fn local_mode(&mut self) -> u32;
    /// The local-mode open of `path`; true when it names the local file.
    fn local_open(&mut self, path: u32) -> bool;
    fn local_rewind(&mut self);
    fn readiness(&mut self, mode: ReadinessMode) -> Translation;
    fn path_byte(&self, path: u32, index: u32) -> u8;
    fn device_for_letter(&mut self, letter: u8) -> u32;
    fn active_device(&self) -> Option<u32>;
    fn allocate_tag(&mut self) -> u32;
    fn acquire_result(&mut self) -> Option<ResultSlot>;
    fn wait_result(&mut self);
    fn release_result(&mut self);
    fn result(&self, slot: ResultSlot) -> (u32, u32);
    fn complete(&mut self, callback: Callback, status: u32, value: u32);
    fn complete_status(&mut self, callback: Callback, status: u32);
    /// Builds the payload on the stack and posts its header to mediamsg.
    fn post(&mut self, code: RequestCode, source: u32, payload: Payload);
}

const BUSY: u32 = 9;
const BAD_DRIVE: u32 = 8;
const OTHER_DRIVE: u32 = 5;
const LOCAL_NOT_FOUND: u32 = 6;

fn refuses_blocking(task: u32) -> bool {
    task == u32::from(MEDIA_TASK) || task == 0
}

fn ready<P: Posting>(port: &mut P) -> u32 {
    port.readiness(ReadinessMode(0)).status.0
}

fn code(family: Family, primary: u16, alternate: u16) -> RequestCode {
    RequestCode(if family == Family::Alternate { alternate } else { primary })
}

fn drive_refusal<P: Posting>(port: &mut P, path: u32) -> Option<u32> {
    if port.path_byte(path, 1) != b':' {
        return None;
    }
    let letter = match port.path_byte(path, 0) {
        letter @ b'A'..=b'Z' => letter,
        letter @ b'a'..=b'z' => letter - 32,
        _ => return Some(BAD_DRIVE),
    };
    let device = port.device_for_letter(letter);
    if port.active_device() == Some(device) {
        None
    } else {
        Some(OTHER_DRIVE)
    }
}

/// The open request at 0x71c2b0. Returns the tag, or for a local-mode open 1 when the
/// local file matched; every refusal returns 0.
pub fn open<P: Posting>(port: &mut P, callback: Callback, path: u32, mode: u32, wait: Wait, word: u32, family: Family) -> u32 {
    let task = port.current_task();
    if wait == Wait::Blocking && refuses_blocking(task) {
        port.complete(callback, BUSY, 0);
        return 0;
    }
    if port.local_mode() == 1 {
        let matched = port.local_open(path);
        if matched {
            port.local_rewind();
        }
        port.complete(callback, if matched { 0 } else { LOCAL_NOT_FOUND }, u32::from(matched));
        return u32::from(matched);
    }
    let status = ready(port);
    if status != 0 {
        port.complete(callback, status, 0);
        return 0;
    }
    if let Some(refusal) = drive_refusal(port, path) {
        port.complete(callback, refusal, 0);
        return 0;
    }
    let slot = match wait {
        Wait::Posted => None,
        Wait::Blocking => match port.acquire_result() {
            Some(slot) => Some(slot),
            None => {
                port.complete(callback, BUSY, 0);
                return 0;
            }
        },
    };
    let tag = port.allocate_tag();
    let source = port.current_task();
    port.post(code(family, 0x2c17, 0x2c2d), source, Payload::Open { callback, path, mode, tag, slot, word });
    match slot {
        None => tag,
        Some(slot) => {
            port.wait_result();
            let (status, _) = port.result(slot);
            port.release_result();
            if status == 0 || cfg!(file_service_control_open_failure_keeps_tag) {
                tag
            } else {
                0
            }
        }
    }
}

fn blocking_slot<P: Posting>(port: &mut P, wait: Wait, refuse: impl FnOnce(&mut P)) -> Result<Option<ResultSlot>, ()> {
    let task = port.current_task();
    match wait {
        Wait::Posted => Ok(None),
        Wait::Blocking if refuses_blocking(task) => {
            refuse(port);
            Err(())
        }
        Wait::Blocking => match port.acquire_result() {
            Some(slot) => Ok(Some(slot)),
            None => {
                refuse(port);
                Err(())
            }
        },
    }
}

/// The read request at 0x71be74. Outside local mode it first requires ready media.
/// Returns the worker's count for a blocking read that succeeded, else 0.
pub fn read<P: Posting>(port: &mut P, callback: Callback, handle: u32, length: u32, buffer: u32, wait: Wait, family: Family) -> u32 {
    if port.local_mode() == 0 {
        let status = ready(port);
        if status != 0 {
            port.complete(callback, status, 0);
            return 0;
        }
    }
    let Ok(slot) = blocking_slot(port, wait, |port| port.complete(callback, BUSY, 0)) else {
        return 0;
    };
    let source = port.current_task();
    port.post(code(family, 0x2c18, 0x2c2e), source, Payload::Read { callback, handle, length, buffer, slot });
    finish_count(port, slot)
}

fn finish_count<P: Posting>(port: &mut P, slot: Option<ResultSlot>) -> u32 {
    let Some(slot) = slot else {
        return 0;
    };
    port.wait_result();
    let (status, count) = port.result(slot);
    port.release_result();
    if status == 0 {
        count
    } else {
        0
    }
}

/// The write request at 0x71bfe0. Returns the worker's count for a blocking write that
/// succeeded, else 0.
pub fn write<P: Posting>(port: &mut P, callback: Callback, handle: u32, second: u32, third: u32, wait: Wait, family: Family) -> u32 {
    let status = ready(port);
    if status != 0 {
        port.complete(callback, status, 0);
        return 0;
    }
    let Ok(slot) = blocking_slot(port, wait, |port| port.complete(callback, BUSY, 0)) else {
        return 0;
    };
    let source = port.current_task();
    port.post(code(family, 0x2c19, 0x2c2f), source, Payload::Write { callback, handle, second, third, slot });
    finish_count(port, slot)
}

/// The close request at 0x71c134; its callback receives a status only. Returns the
/// worker's status for a blocking close, else 0.
pub fn close<P: Posting>(port: &mut P, callback: Callback, handle: u32, word: u32, name: u32, wait: Wait, family: Family) -> u32 {
    if port.local_mode() == 1 {
        port.complete_status(callback, 0);
        return 0;
    }
    let status = ready(port);
    if status != 0 {
        port.complete_status(callback, status);
        return 0;
    }
    let Ok(slot) = blocking_slot(port, wait, |port| port.complete_status(callback, BUSY)) else {
        return 0;
    };
    let source = port.current_task();
    port.post(code(family, 0x2c1a, 0x2c30), source, Payload::Close { callback, handle, word, name, slot });
    let Some(slot) = slot else {
        return 0;
    };
    port.wait_result();
    let (status, _) = port.result(slot);
    port.release_result();
    status
}

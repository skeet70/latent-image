use crate::shutdown::RequestId;
use crate::storage_shutdown_lens_ring::LensPendingCommand;
use crate::storage_shutdown_lens_service::LensCallback;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensResponseStatus(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensCommandWord(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensControlFailure { QueueFull, ResponseOverflow, MissingResponse, MissingSkippedResponse }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensReplyData { StatusOnly, Channel { pages: u16, flags: u8, present: u8 } }

pub trait LensControlSoftware {
    fn push(&mut self, command: LensPendingCommand) -> bool;
    fn peek(&mut self) -> Option<LensPendingCommand>;
    fn pop(&mut self) -> Option<LensPendingCommand>;
    fn transmit(&mut self, command: LensCommandWord);
    fn diagnostic(&mut self, failure: LensControlFailure);
    fn reply(&mut self, callback: LensCallback, status: LensResponseStatus, data: LensReplyData);
    fn notify_observer(&mut self, status: LensResponseStatus);
}

fn word(context: &[u8; 0x174], offset: usize) -> u32 {
    u32::from_le_bytes([context[offset], context[offset + 1], context[offset + 2], context[offset + 3]])
}
fn half(context: &[u8; 0x174], offset: usize) -> u16 { u16::from_le_bytes([context[offset], context[offset + 1]]) }
fn put_word(context: &mut [u8; 0x174], offset: usize, value: u32) { context[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }

pub fn skip_command(context: &[u8; 0x174], operation: RequestId, command: LensCommandWord) -> bool {
    let code = (command.0 & 0x7fff) >> 6;
    if word(context, 0x170) == 0 || context[0x12c + (code >> 3) as usize] & (1 << (code & 7)) != 0 {
        if operation == RequestId(0x722b) { return (command.0 >> 16) as u16 == half(context, 0x12a); }
        if operation == RequestId(0x722d) && word(context, 0x16c) == 0 {
            let value = (command.0 >> 16) & 0xf;
            return value != 0 && value != 4;
        }
        false
    } else { true }
}

pub fn dispatch_reply(software: &mut impl LensControlSoftware, command: LensPendingCommand, status: LensResponseStatus, response: u32) {
    if !command.is_last() || command.callback.0 == 0 { return; }
    let operation = command.operation();
    let data = if operation == RequestId(0x722e) {
        LensReplyData::Channel { pages: ((response >> 16) & 0x7ff) as u16, flags: ((response >> 27) & 0xf) as u8, present: (response >> 31) as u8 }
    } else { LensReplyData::StatusOnly };
    software.reply(command.callback, status, data);
}

pub fn enqueue_control(software: &mut impl LensControlSoftware, context: &[u8; 0x174]) {
    let callback = LensCallback(word(context, 0x0c));
    let operation = RequestId(half(context, 0));
    let count = context[0x38];
    for index in 0..count {
        let command = LensCommandWord(word(context, 0x10 + usize::from(index) * 4));
        let last = index == count - 1;
        let pending = LensPendingCommand::new(operation, last, command.0, callback, false);
        if !skip_command(context, operation, command) {
            if !software.push(pending) { software.diagnostic(LensControlFailure::QueueFull); }
            software.transmit(command);
            let code = (command.0 & 0x7fff) >> 6;
            if last && matches!(code, 0x62 | 0x56 | 0x58 | 0x40 | 0x80 | 0xa8 | 0xa9) {
                let pending = LensPendingCommand::new(RequestId(0x7202), true, 0x3fc0, LensCallback(0), false);
                if !software.push(pending) { software.diagnostic(LensControlFailure::QueueFull); }
                software.transmit(LensCommandWord(0x3fc0));
            }
        } else if software.peek().is_some() {
            let pending = LensPendingCommand::new(operation, last, command.0, callback, true);
            if !software.push(pending) { software.diagnostic(LensControlFailure::QueueFull); }
        } else {
            dispatch_reply(software, pending, LensResponseStatus(0), word(context, 0xe0));
        }
    }
}

pub fn command_response(software: &mut impl LensControlSoftware, context: &mut [u8; 0x174]) -> bool {
    let status = LensResponseStatus(word(context, 0x10));
    let count = context[0xdc];
    if count < 10 {
        let response = word(context, 0x0c);
        put_word(context, 0xb4 + usize::from(count) * 4, response);
        context[0xdc] = count + 1;
    } else { software.diagnostic(LensControlFailure::ResponseOverflow); }
    let Some(command) = software.pop() else {
        software.diagnostic(LensControlFailure::MissingResponse);
        return false;
    };
    dispatch_reply(software, command, status, word(context, 0xb4));
    if command.is_last() { context[0xdc] = 0; }
    while software.peek().is_some_and(LensPendingCommand::is_skipped) {
        let Some(skipped) = software.pop() else { software.diagnostic(LensControlFailure::MissingSkippedResponse); break; };
        dispatch_reply(software, skipped, status, word(context, 0xe0));
        if skipped.is_last() { context[0xdc] = 0; }
    }
    if command.operation() == RequestId(0x722b) {
        context[0x12a..0x12c].copy_from_slice(&((command.command >> 16) as u16).to_le_bytes());
    }
    let status = if command.operation() == RequestId(0x7217) && status.0 == 0 && half(context, 0xb6) != 0 { LensResponseStatus(4) } else { status };
    if status.0 == 0 { false } else { software.notify_observer(status); true }
}

#[cfg(test)]
#[path = "storage_shutdown_lens_control_tests.rs"]
mod tests;

use crate::shutdown::RequestId;
use crate::storage_shutdown_lens_ring::LensPendingCommand;
use crate::storage_shutdown_lens_service::LensCallback;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensResponseStatus(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensCommandWord(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensControlFailure {
    /// 0x6b92cc(5, 6, argument).
    QueueFull(u8),
    /// 0x6b92cc(5, 7, 0).
    ResponseOverflow,
    /// 0x6b92cc(5, 7, 1).
    MissingResponse,
    /// 0x6b92cc(5, 7, 2).
    MissingSkippedResponse,
}

/// The record stock passes to the reply callback in r1, one variant per body of the
/// 91-entry table at 0x6b86d8.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensReplyData {
    /// 0x6b8844.
    StatusOnly,
    /// 0x6b8850 and 0x6b8a0c.
    Bits16To18([u8; 3]),
    /// 0x6b887c.
    Wide14([u8; 14]),
    /// 0x6b894c.
    ResponseHalfword([u8; 2]),
    /// 0x6b8964. Stock leaves byte 1 unwritten; it is zero here.
    Bit31And14([u8; 4]),
    /// 0x6b8988. Byte 1 is zero here.
    Bit31AndSigned15([u8; 4]),
    /// 0x6b89bc.
    Bit31SevenAndByte([u8; 3]),
    /// 0x6b89e8.
    ByteAndBit31([u8; 2]),
    /// 0x6b8a50.
    Bits16To27([u8; 2]),
    /// 0x6b8a6c and 0x6b8bc8.
    Bits16To31([u8; 16]),
    /// 0x6b8b18: stored at context + 0x120.
    ContextBits9([u8; 9]),
    /// 0x6b8b9c.
    Channel { pages: u16, flags: u8, present: u8 },
    /// 0x6b8c74.
    Bits16To30AndBit31([u8; 3]),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReplyBody {
    None,
    Status,
    Bits16To18,
    Wide14,
    ResponseHalfword,
    Bit31And14,
    Bit31AndSigned15,
    Bit31SevenAndByte,
    ByteAndBit31,
    Bits16To27,
    Bits16To31,
    ContextBits9,
    Channel,
    Bits16To30AndBit31,
}

/// 0x6b86d8, operations 0x7202..=0x725c.
const REPLY_BODIES: [ReplyBody; 91] = {
    use ReplyBody::*;
    [
        Status, Status, Status, Status, Status, Status, Status, Bits16To18, Status, Status,
        Wide14, ResponseHalfword, Bit31And14, Bit31AndSigned15, Bit31SevenAndByte, ByteAndBit31,
        Bits16To18, ResponseHalfword, Bits16To27, Bits16To31, ResponseHalfword, ContextBits9,
        Status, Status, Status, None, None, Status, Status, Status, Status, Status, Status, Status,
        Status, Status, Status, Status, Status, Status, Status, Status, Status, Status, Channel,
        Status, Status, Status, Status, Status, Status, Status, Status, Status, Bits16To31,
        ResponseHalfword, Bits16To30AndBit31, ResponseHalfword, ResponseHalfword, ResponseHalfword,
        ResponseHalfword, Status, Status, Status, Status, Status, Status, Status, Status, Status,
        Status, Status, Status, Status, Status, Status, Status, Status, Status, None, None, None,
        None, None, ResponseHalfword, Status, Status, Status, Status, Status, Status,
    ]
};

pub trait LensControlSoftware {
    fn push(&mut self, command: LensPendingCommand) -> bool;
    fn peek(&mut self) -> Option<LensPendingCommand>;
    fn pop(&mut self) -> Option<LensPendingCommand>;
    fn transmit(&mut self, command: LensCommandWord);
    fn diagnostic(&mut self, failure: LensControlFailure);
    fn reply(&mut self, callback: LensCallback, status: LensResponseStatus, data: LensReplyData);
    /// 0x6b9980..0x6b9988: the observer reads context + 0x120.
    fn notify_observer(&mut self, status: LensResponseStatus, bits: &[u8; 9]);
}

fn word(context: &[u8; 0x174], offset: usize) -> u32 {
    u32::from_le_bytes([context[offset], context[offset + 1], context[offset + 2], context[offset + 3]])
}
fn half(context: &[u8; 0x174], offset: usize) -> u16 { u16::from_le_bytes([context[offset], context[offset + 1]]) }
fn put_word(context: &mut [u8; 0x174], offset: usize, value: u32) { context[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }
fn bit(response: u32, index: u32) -> u8 { ((response >> index) & 1) as u8 }

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

fn reply_data(body: ReplyBody, context: &mut [u8; 0x174], response: u32) -> Option<LensReplyData> {
    let data = match body {
        ReplyBody::None => return None,
        ReplyBody::Status => LensReplyData::StatusOnly,
        ReplyBody::Bits16To18 => LensReplyData::Bits16To18([bit(response, 16), bit(response, 17), bit(response, 18)]),
        ReplyBody::Wide14 => LensReplyData::Wide14([
            bit(response, 31), bit(response, 30), bit(response, 29), bit(response, 28), bit(response, 26),
            ((response >> 24) & 3) as u8,
            bit(response, 23), bit(response, 22), bit(response, 21), bit(response, 20),
            bit(response, 19), bit(response, 18), bit(response, 17), bit(response, 16),
        ]),
        ReplyBody::ResponseHalfword => {
            let halfword = (response >> 16) as u16;
            LensReplyData::ResponseHalfword(halfword.to_le_bytes())
        }
        ReplyBody::Bit31And14 => {
            let field = ((response >> 16) & 0x3fff) as u16;
            let mut record = [0u8; 4];
            record[0] = bit(response, 31);
            record[2..].copy_from_slice(&field.to_le_bytes());
            LensReplyData::Bit31And14(record)
        }
        ReplyBody::Bit31AndSigned15 => {
            let field = ((response >> 16) & 0x7fff) as u16;
            let field = if field & 0x4000 != 0 { field | 0x8000 } else { field };
            let mut record = [0u8; 4];
            record[0] = bit(response, 31);
            record[2..].copy_from_slice(&field.to_le_bytes());
            LensReplyData::Bit31AndSigned15(record)
        }
        ReplyBody::Bit31SevenAndByte => LensReplyData::Bit31SevenAndByte([
            bit(response, 31), ((response >> 24) & 0x7f) as u8, (response >> 16) as u8,
        ]),
        ReplyBody::ByteAndBit31 => LensReplyData::ByteAndBit31([(response >> 16) as u8, bit(response, 31)]),
        ReplyBody::Bits16To27 => LensReplyData::Bits16To27((((response >> 16) & 0xfff) as u16).to_le_bytes()),
        ReplyBody::Bits16To31 => {
            let mut record = [0u8; 16];
            for (index, byte) in record.iter_mut().enumerate() { *byte = bit(response, 16 + index as u32); }
            LensReplyData::Bits16To31(record)
        }
        ReplyBody::ContextBits9 => {
            let bits = [
                bit(response, 16), bit(response, 17), bit(response, 18), bit(response, 19), bit(response, 20),
                bit(response, 21), bit(response, 22), bit(response, 24), bit(response, 28),
            ];
            context[0x120..0x129].copy_from_slice(&bits);
            LensReplyData::ContextBits9(bits)
        }
        ReplyBody::Channel => LensReplyData::Channel {
            pages: ((response >> 16) & 0x7ff) as u16,
            flags: ((response >> 27) & 0xf) as u8,
            present: (response >> 31) as u8,
        },
        ReplyBody::Bits16To30AndBit31 => {
            let field = ((response >> 16) & 0x7fff) as u16;
            let [low, high] = field.to_le_bytes();
            LensReplyData::Bits16To30AndBit31([low, high, bit(response, 31)])
        }
    };
    Some(data)
}

pub fn dispatch_reply(
    software: &mut impl LensControlSoftware,
    context: &mut [u8; 0x174],
    command: LensPendingCommand,
    status: LensResponseStatus,
    response: u32,
) {
    if !command.is_last() || command.callback.0 == 0 { return; }
    let Some(index) = command.operation().0.checked_sub(0x7202).map(usize::from) else { return; };
    let Some(body) = REPLY_BODIES.get(index) else { return; };
    let Some(data) = reply_data(*body, context, response) else { return; };
    software.reply(command.callback, status, data);
}

pub fn enqueue_control(software: &mut impl LensControlSoftware, context: &mut [u8; 0x174]) {
    let callback = LensCallback(word(context, 0x0c));
    let operation = RequestId(half(context, 0));
    let count = context[0x38];
    for index in 0..count {
        let command = LensCommandWord(word(context, 0x10 + usize::from(index) * 4));
        let last = index == count - 1;
        let pending = LensPendingCommand::new(operation, last, command.0, callback, false);
        if !skip_command(context, operation, command) {
            if !software.push(pending) { software.diagnostic(LensControlFailure::QueueFull(1)); }
            software.transmit(command);
            let code = (command.0 & 0x7fff) >> 6;
            if last && matches!(code, 0x62 | 0x56 | 0x58 | 0x40 | 0x80 | 0xa8 | 0xa9) {
                let pending = LensPendingCommand::new(RequestId(0x7202), true, 0x3fc0, LensCallback(0), false);
                if !software.push(pending) { software.diagnostic(LensControlFailure::QueueFull(2)); }
                software.transmit(LensCommandWord(0x3fc0));
            }
        } else if software.peek().is_some() {
            let pending = LensPendingCommand::new(operation, last, command.0, callback, true);
            if !software.push(pending) { software.diagnostic(LensControlFailure::QueueFull(0)); }
        } else {
            dispatch_reply(software, context, pending, LensResponseStatus(0), word(context, 0xe0));
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
    let command = match software.pop() {
        Some(command) => Some(command),
        None => { software.diagnostic(LensControlFailure::MissingResponse); None }
    };
    if let Some(command) = command {
        dispatch_reply(software, context, command, status, word(context, 0xb4));
        if command.is_last() { context[0xdc] = 0; }
    }
    while software.peek().is_some_and(LensPendingCommand::is_skipped) {
        let Some(skipped) = software.pop() else { software.diagnostic(LensControlFailure::MissingSkippedResponse); break; };
        dispatch_reply(software, context, skipped, status, word(context, 0xe0));
        if skipped.is_last() { context[0xdc] = 0; }
    }
    if let Some(command) = command {
        if command.operation() == RequestId(0x722b) {
            context[0x12a..0x12c].copy_from_slice(&((command.command >> 16) as u16).to_le_bytes());
        }
    }
    let status = if command.is_some_and(|command| command.operation() == RequestId(0x7217))
        && status.0 == 0 && half(context, 0xb6) != 0 { LensResponseStatus(4) } else { status };
    if status.0 == 0 { false } else {
        let mut bits = [0u8; 9];
        bits.copy_from_slice(&context[0x120..0x129]);
        software.notify_observer(status, &bits);
        true
    }
}

#[cfg(test)]
#[path = "storage_shutdown_lens_control_tests.rs"]
mod tests;

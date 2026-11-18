#![forbid(unsafe_code)]

//! X-M1 1.50 layout of the live statistics configuration's requests: the block builders
//! 0xf3b568, 0xf3b5f0, 0xf3b6c8, 0xf3b72c, 0xf3b7a0, 0xf3b9d8, 0xf3bb70, 0xf3be7c and the queue-48
//! senders 0x1039638..0x103b040 that copy them into the posted message.

use super::Encoded;
use crate::statistics::*;
use crate::StatisticsOperation as Op;
use crate::stock::StockCallbackWord;

/// 0xf3b5a0..0xf3b5a8: the setup block's last word unless Command11 replaces it (0xf3b5c4..0xf3b5cc).
const SETUP_TAIL: u32 = 0x11de_784a;

const SETUP: usize = 0;
const FORMAT: usize = 16;
const WINDOW: usize = 40;
const OUTPUT: usize = 56;
const EXPOSURE: usize = 72;
const CAPTURE: usize = 128;
const NOTICE: usize = 184;
const CONFIGURATION_BYTES: usize = 196;

fn words(message: &mut Encoded, offset: usize, words: &[u32]) {
    for (index, word) in words.iter().enumerate() {
        message.word(offset + 4 * index, *word);
    }
}

fn setup(message: &mut Encoded, offset: usize, setup: &Setup) {
    let tail = match setup.tail { SetupTail::Default => SETUP_TAIL, SetupTail::Adjustment(word) => word, SetupTail::S1 => 1 };
    words(message, offset, &[setup.descriptor, setup.pair[0], setup.pair[1], tail]);
}

/// 0xf3b5f0..0xf3b698; with nothing selected the mode word (+20) is never written. The statistics
/// task's readers of its copy (0x16fec34+0x44b4, e.g. 0x1029808, 0x102cb64, 0x103f404) act only
/// on 4, 6, 7, 8, 9 and 10, so zero takes each one's general branch.
fn format(message: &mut Encoded, offset: usize, format: &Format) {
    words(message, offset, &format.dimensions);
    message.halfword(offset + 8, format.ratio[0]);
    message.halfword(offset + 10, format.ratio[1]);
    words(message, offset + 12, &[format.value, format.class, format.mode]);
}

/// 0xf3b7a0..0xf3b92c; byte 55 is never written. It lands at 0x16fec34+0x4523, which nothing reads.
fn exposure(message: &mut Encoded, offset: usize, exposure: &Exposure) {
    words(message, offset, &exposure.settings);
    message.word(offset + 0x20, exposure.window.extent);
    for (index, corrected) in exposure.window.corrected.iter().enumerate() {
        message.halfword(offset + 0x24 + 2 * index, *corrected);
    }
    message.halfword(offset + 0x2a, exposure.adjusted);
    message.halfword(offset + 0x2c, exposure.window.level);
    message.halfword(offset + 0x2e, exposure.linear);
    message.word(offset + 0x30, exposure.source.word);
    message.put(offset + 0x34, &[exposure.source.byte, exposure.class, exposure.detail]);
}

/// 0xf3b9d8..0xf3bb0c; bytes 20..28, 34..35 and 54..55 are never written. Nothing in the
/// statistics task reads 34..35 or 54..55; it keeps 20..28 at 0x16fec34+0x453c..0x4544, which its
/// init (0x10287c8) zeroes and the 0x4006/0x4008 receivers (0x10344f0, 0x1034820) zero or keep.
fn capture(message: &mut Encoded, offset: usize, capture: &Capture) {
    words(message, offset, &[capture.position, capture.selection, capture.settings[0], capture.settings[1], capture.retune]);
    message.put(offset + 0x1d, &capture.options);
    words(message, offset + 0x24, &[capture.spans[0], capture.spans[1], capture.dimensions[0], capture.dimensions[1]]);
    message.halfword(offset + 0x34, capture.policy);
}

fn blocks(length: usize, configuration: &Configuration) -> Encoded {
    let mut message = Encoded::new(length);
    setup(&mut message, SETUP, &configuration.setup);
    format(&mut message, FORMAT, &configuration.format);
    words(&mut message, WINDOW, &configuration.window);
    words(&mut message, OUTPUT, &configuration.output);
    exposure(&mut message, EXPOSURE, &configuration.exposure);
    capture(&mut message, CAPTURE, &configuration.capture);
    message
}

fn notice(message: &mut Encoded, offset: usize, notice: &Notice, resolve: &mut impl FnMut(StockCallbackWord) -> StockCallbackWord) {
    words(message, offset, &[notice.pending, notice.results.0, resolve(notice.callback).0]);
}

fn configuration(length: usize, configuration: &Configuration, resolve: &mut impl FnMut(StockCallbackWord) -> StockCallbackWord) -> Encoded {
    let mut message = blocks(length, configuration);
    notice(&mut message, NOTICE, &configuration.notice, resolve);
    message
}

/// Senders: 0x1039638 (0x4002, 208 bytes), 0x1039c7c (0x4006, 204), 0x10398b0 (the S1 start:
/// 0x4004, 208, or 0x4006, 204, when the capture block's selection word is nonzero), 0x1039e48
/// (0x4008, 248), 0x1039dd4 (0x4007), 0x103a1a8 (0x4009), 0x103a470 (0x4014), 0x103aff0
/// (0x4041), 0x103a00c (0x400a), 0x103a0f0 (0x400c), 0x103a594 (0x4017), 0x103a404 (0x4013),
/// 0x103a2c0 (0x4011), 0x103a254 (0x4010), 0x103a32c (0x4012), 0x103a398 (0x402c), 0x103a694
/// (0x402a), 0x103a708 (0x402e), 0x103a09c (0x400b), 0x103a13c (0x400e), 0x103a200 (0x400f),
/// 0x103b064 (0x4043). Bytes they leave unwritten are zero.
pub fn statistics_payload(payload: &StatisticsPayload) -> (Op, Encoded) {
    statistics_payload_with_callbacks(payload, |word| word)
}

/// Resolves callback code words while encoding their fields, without copying the packet again.
pub fn statistics_payload_with_callbacks(payload: &StatisticsPayload, mut resolve: impl FnMut(StockCallbackWord) -> StockCallbackWord) -> (Op, Encoded) {
    use StatisticsPayload as P;
    match payload {
        P::Start { configuration: blocks, feature, callback } => {
            let mut message = configuration(208, blocks, &mut resolve);
            words(&mut message, CONFIGURATION_BYTES, &[resolve(*callback).0, blocks.flag, *feature]);
            (Op::Request02, message)
        }
        P::StartSelected { configuration: blocks, callback } => {
            let mut message = configuration(204, blocks, &mut resolve);
            words(&mut message, CONFIGURATION_BYTES, &[blocks.flag, resolve(*callback).0]);
            (Op::Request06, message)
        }
        // 0x1039998..0x10399d0: the flag word precedes the notice.
        P::S1Start { configuration: start, header, callback } => {
            let mut message = self::blocks(208, start);
            message.word(NOTICE, start.flag);
            notice(&mut message, NOTICE + 4, &start.notice, &mut resolve);
            words(&mut message, NOTICE + 16, &[*header, resolve(*callback).0]);
            (Op::Request04, message)
        }
        // 0x1039f3c..0x1039f68 stores each of the three words at 200 only when it is nonzero;
        // 0xf3bc14..0xf3bc28 passes 0, the profile view word and 0. Bytes 212..223 are never written.
        // The statistics task's 0x1034820 reads 184..199 and 224..247 only.
        P::Configure { configuration: blocks, view, command, callbacks } => {
            let mut message = configuration(248, blocks, &mut resolve);
            message.word(CONFIGURATION_BYTES, blocks.flag);
            message.word(204, *view);
            words(&mut message, 224, &[command.second, command.third, command.fourth]);
            words(&mut message, 236, &[resolve(callbacks[0]).0, resolve(callbacks[1]).0, resolve(callbacks[2]).0]);
            (Op::Request08, message)
        }
        P::Stop { pause, shot_idle, callback } => {
            let mut message = Encoded::new(12);
            words(&mut message, 0, &[u32::from(*pause), u32::from(*shot_idle), resolve(*callback).0]);
            (Op::Request07, message)
        }
        P::Release { callback } => {
            let mut message = Encoded::new(8);
            message.word(4, resolve(*callback).0);
            (Op::Request09, message)
        }
        P::MeasuredWindow(window) => {
            let mut message = Encoded::new(12);
            for (index, corrected) in window.corrected.iter().enumerate() {
                message.halfword(2 * index, *corrected);
            }
            message.halfword(6, window.level);
            message.word(8, window.extent);
            (Op::Request14, message)
        }
        // 0x103affc..0x103b008 stores nothing when the source word is zero, and 0x102d6c8 still
        // takes the word (0xed8bac), the byte (0x16fec34+0x4520) and the position (+0x4528).
        P::MeasuredSource { source, position } => {
            let mut message = Encoded::new(12);
            message.word(0, source.word);
            message.put(4, &[source.byte]);
            message.word(8, *position);
            (Op::Request41, message)
        }
        P::Adjust { setup: block, callbacks } => {
            let mut message = Encoded::new(24);
            setup(&mut message, 0, block);
            words(&mut message, 16, &[resolve(callbacks[0]).0, resolve(callbacks[1]).0]);
            (Op::Request0a, message)
        }
        P::Command13 => (Op::Request0c, Encoded::new(0)),
        P::Prepare { selection, range, exposure: block, callback } => {
            let mut message = Encoded::new(92);
            words(&mut message, 0, selection);
            words(&mut message, 12, range);
            message.word(32, resolve(*callback).0);
            exposure(&mut message, 36, block);
            (Op::Request17, message)
        }
        P::Output { output, callback } => (Op::Request13, block_then_callback(output, resolve(*callback))),
        P::Window { window, callback } => (Op::Request11, block_then_callback(window, resolve(*callback))),
        P::Format { format: block, callback } => {
            let mut message = Encoded::new(28);
            format(&mut message, 0, block);
            message.word(24, resolve(*callback).0);
            (Op::Request10, message)
        }
        P::Exposure { exposure: block, callback } => {
            let mut message = Encoded::new(60);
            exposure(&mut message, 0, block);
            message.word(56, resolve(*callback).0);
            (Op::Request12, message)
        }
        P::Capture { capture: block, callback } => {
            let mut message = Encoded::new(60);
            capture(&mut message, 0, block);
            message.word(56, resolve(*callback).0);
            (Op::Request2c, message)
        }
        P::Selection { pending, results, callbacks, flagged } => {
            let mut message = Encoded::new(20);
            words(&mut message, 0, &[*pending, results.0, resolve(callbacks[0]).0, resolve(callbacks[1]).0, u32::from(*flagged)]);
            (Op::Request2a, message)
        }
        P::Command24 { command, callback } => {
            let mut message = Encoded::new(28);
            words(&mut message, 0, &[resolve(*callback).0, command.second, command.third, command.fourth, command.ratio[0], command.ratio[1], command.fifth]);
            (Op::Request2e, message)
        }
        P::Request0b { value } => {
            let mut message = Encoded::new(4);
            message.word(0, *value);
            (Op::Request0b, message)
        }
        P::Request0e { record, callback } => {
            let mut message = Encoded::new(32);
            message.put(0, &record.0);
            message.word(28, resolve(*callback).0);
            (Op::Request0e, message)
        }
        P::Request0f { callback } => {
            let mut message = Encoded::new(4);
            message.word(0, resolve(*callback).0);
            (Op::Request0f, message)
        }
        P::Request43 => (Op::Request43, Encoded::new(0)),
    }
}

fn block_then_callback(block: &[u32; 4], callback: crate::stock::StockCallbackWord) -> Encoded {
    let mut message = Encoded::new(20);
    words(&mut message, 0, block);
    message.word(16, callback.0);
    message
}

pub const CAPTURE_REPORT_BYTES: usize = 64 + RESULTS_BYTES;

/// The record 0xf3b460..0xf3b55c passes to Command09's client with status 17 (its frame at
/// -0x460): completion 0x79's payload bytes 4..16, 16..20 and 20..60 around the capture window's
/// dimensions, then the results. The client 0xe6c578 reads words 0..5, bytes 24..64 and the
/// results. Where 0xdca798 writes no dimensions stock leaves stack there; the owned record has zero.
pub fn capture_report(report: &CaptureReport<'_>) -> [u8; CAPTURE_REPORT_BYTES] {
    let mut bytes = [0; CAPTURE_REPORT_BYTES];
    let mut put = |offset: usize, value: &[u8]| bytes[offset..offset + value.len()].copy_from_slice(value);
    let completion = |range: core::ops::Range<usize>| report.completion.get(range).unwrap_or(&[]);
    put(0, completion(4..16));
    put(12, &report.dimensions[1].wrapping_shl(1).to_le_bytes());
    put(16, &report.dimensions[0].to_le_bytes());
    put(20, completion(16..20));
    put(24, completion(20..60));
    put(64, report.results);
    bytes
}

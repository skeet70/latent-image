#![forbid(unsafe_code)]

//! X-M1 1.50 layout of saclac's imgdc requests: the S1 job block 0xf22f3c builds and its senders
//! 0xfeaa64 (0x3f42) and 0xfeaaa4 (0x3f43), and the live view's senders 0xfea438..0xfead6c.

use super::{Encoded, StockCallbackWord};
use crate::imaging::*;
use crate::{Completion, ImgDcOperation as Op};

/// 0xf22f3c's block at its frame -0x80. Bytes 6..8, 27, 82..84 and 93..96 are never written;
/// the owned block has zero there.
fn s1_job(message: &mut Encoded, job: &S1Job) {
    for (index, level) in job.levels.iter().enumerate() {
        message.halfword(2 * index, *level as u16);
    }
    for (index, word) in [job.lens_index, job.lens_word, job.selection, job.step].into_iter().enumerate() {
        message.word(8 + 4 * index, word);
    }
    message.halfword(24, job.step_limit);
    message.put(26, &[job.step_mode]);
    let words = [
        job.curves[0], job.curves[1], job.output, job.view_mode, job.descriptors[0], job.descriptors[1], job.descriptors[2],
        job.setup[0], job.setup[1], job.setup[2], job.window[0], job.window[1], job.exposure,
    ];
    for (index, word) in words.into_iter().enumerate() {
        message.word(28 + 4 * index, word);
    }
    message.halfword(80, job.exposure_offset as u16);
    message.word(84, job.view_active);
    message.word(88, job.unselected_view);
    message.put(92, &[job.view_byte]);
    let words = [job.classes[0], job.classes[1], job.ratio, job.span, job.class, job.unselected, job.scene];
    for (index, word) in words.into_iter().enumerate() {
        message.word(96 + 4 * index, word);
    }
}

/// 0xfeaa64 copies the 124-byte block ahead of its three callbacks; 0xfeaaa4 sends one word.
pub fn imaging_payload(payload: &ImagingPayload) -> (Op, Encoded) {
    match payload {
        ImagingPayload::BeginS1 { job, callbacks } => {
            let mut message = Encoded::new(136);
            s1_job(&mut message, job);
            for (index, callback) in callbacks.iter().enumerate() {
                message.word(124 + 4 * index, callback.0);
            }
            (Op::Request42, message)
        }
        ImagingPayload::EndS1 { callback } => {
            let mut message = Encoded::new(4);
            message.word(0, callback.0);
            (Op::Request43, message)
        }
    }
}

/// The live view's completion entries, 0xf2f65c..0xf2f7c8: 0xf2f65c posts 0x65, 0xf2f684 0x63, 0xf2f6ac 0x4b, 0xf2f6d8 0x4a,
/// 0xf2f700 0x44, 0xf2f728 0x43, 0xf2f750 0x42, 0xf2f778 0x41, 0xf2f7a0 0x40 and 0xf2f7c8 0x3f.
const VIEW_WINDOWED: StockCallbackWord = StockCallbackWord(0xf2f65c);
const VIEW_ADJUSTED: StockCallbackWord = StockCallbackWord(0xf2f684);
const VIEW_ACCESSOR_VALUE: StockCallbackWord = StockCallbackWord(0xf2f6ac);
const VIEW_ACCESSOR: StockCallbackWord = StockCallbackWord(0xf2f6d8);
const VIEW_RESUMED: StockCallbackWord = StockCallbackWord(0xf2f700);
const VIEW_SUSPENDED: StockCallbackWord = StockCallbackWord(0xf2f728);
const VIEW_STOPPED: StockCallbackWord = StockCallbackWord(0xf2f750);
const VIEW_CONFIGURED: StockCallbackWord = StockCallbackWord(0xf2f778);
const VIEW_RESTORED: StockCallbackWord = StockCallbackWord(0xf2f7a0);
const VIEW_STARTED: StockCallbackWord = StockCallbackWord(0xf2f7c8);

/// 0xfea438 (0x3f13, 12 bytes), 0xfea468 (0x3f16, 16), 0xfea49c (0x3f26, 232), 0xfea4d0 (0x3f27, 8), 0xfea4fc (0x3f28, 4),
/// 0xfea524 (0x3f29, 8), 0xfea7dc (0x3f34, 20) and 0xfead6c (0x3f18, 8) post the completion word first, then their words.
pub fn view_payload(payload: &ViewPayload) -> (Op, Encoded, &'static [Completion]) {
    use ViewPayload as P;
    match payload {
        P::Start { value, measured } => {
            let mut message = Encoded::new(12);
            message.word(0, VIEW_STARTED.0);
            message.word(4, *value);
            message.word(8, u32::from(*measured));
            (Op::Request13, message, &[Completion::Image3f])
        }
        P::Restore(options) => {
            let mut message = Encoded::new(16);
            message.word(0, VIEW_RESTORED.0);
            message.word(4, 0);
            message.word(8, 0);
            message.word(12, u32::from(options.first) | u32::from(options.second) << 1 | u32::from(options.negative) << 31);
            (Op::Request16, message, &[Completion::Image40])
        }
        P::Refresh { value, motion } => {
            let mut message = Encoded::new(16);
            message.word(0, VIEW_RESTORED.0);
            message.word(4, *value);
            message.word(8, motion.0);
            message.word(12, 3);
            (Op::Request16, message, &[Completion::Image40])
        }
        P::Configure(configured) => {
            let mut message = Encoded::new(232);
            configuration(&mut message, configured);
            (Op::Request26, message, &[Completion::Image41, Completion::Accessor4a, Completion::Accessor4b])
        }
        P::Stop(mode) => {
            let mut message = Encoded::new(8);
            message.word(0, VIEW_STOPPED.0);
            message.word(4, stop_word(*mode));
            (Op::Request27, message, &[Completion::Image42])
        }
        P::Suspend => {
            let mut message = Encoded::new(4);
            message.word(0, VIEW_SUSPENDED.0);
            (Op::Request28, message, &[Completion::Image43])
        }
        P::Resume => {
            let mut message = Encoded::new(8);
            message.word(0, VIEW_RESUMED.0);
            message.word(4, 0);
            (Op::Request29, message, &[Completion::Image44])
        }
        P::Adjust(adjustment) => {
            let mut message = Encoded::new(20);
            message.word(0, VIEW_ADJUSTED.0);
            message.word(4, adjustment.selection);
            message.word(8, adjustment.mode);
            message.word(12, adjustment.first);
            message.word(16, adjustment.second);
            (Op::Request34, message, &[Completion::Image63])
        }
        P::Window(window) => {
            let mut message = Encoded::new(8);
            message.word(0, VIEW_WINDOWED.0);
            message.word(4, *window);
            (Op::Request18, message, &[Completion::Image65])
        }
    }
}

/// 0xf2fe44's mode word: 0 plain, 1 secondary, 6 secondary-negative, 2..5 change code.
fn stop_word(mode: StopMode) -> u32 {
    match mode {
        StopMode::Plain => 0,
        StopMode::Secondary => 1,
        StopMode::SecondaryNegative => 6,
        StopMode::Change(code) => u32::from(code.get()),
    }
}

/// 0xf30f64's block at its frame -0xf4..-0x10; 0xfea4a4..0xfea4c4 copies it after completion
/// word 0xf2f778, so payload offset = 0xf4 minus the frame offset below r11.
fn configuration(message: &mut Encoded, configured: &Configured) {
    let values = &configured.values;
    let words = [VIEW_CONFIGURED.0, VIEW_ACCESSOR.0, VIEW_ACCESSOR_VALUE.0, values.head[0], configured.selection, values.head[1], values.head[2]];
    for (index, word) in words.into_iter().enumerate() {
        message.word(4 * index, word);
    }
    words_at(message, 0x1c, &values.windows[0]);
    words_at(message, 0x28, &values.windows[1]);
    words_at(message, 0x34, &values.block);
    message.word(0x44, match configured.selection_class {
        SelectionClass::Flagged => 4,
        SelectionClass::Unselected => 2,
        SelectionClass::Selected => 3,
    });
    for offset in [0x48, 0x6c] {
        value_group(message, offset, &values.group);
    }
    message.put(0x90, &[16]);
    message.put(0x91, &[16]);
    message.halfword(0x92, 100);
    message.halfword(0x94, 100);
    message.halfword(0x9a, values.converted[0]);
    message.halfword(0x9c, values.converted[1]);
    message.put(0x9e, &[7]);
    message.put(0x9f, &[7]);
    for (offset, property) in [0xa2, 0xa0, 0xa6, 0xa4].into_iter().zip(values.properties) {
        message.halfword(offset, property);
    }
    message.halfword(0xa8, values.converted[0]);
    message.halfword(0xaa, values.converted[1]);
    words_at(message, 0xac, &[values.tail[0], configured.selection_mode, values.tail[1], values.tail[2]]);
    message.put(0xbc, &[values.restorable]);
    message.halfword(0xc4, values.level);
    message.put(0xc8, &[values.detail]);
    message.put(0xc9, &[values.classes[0]]);
    message.put(0xca, &[values.classes[1]]);
    words_at(message, 0xd0, &[configured.prepared.word(), configured.refresh]);
    message.halfword(0xd8, configured.offset);
    message.put(0xda, &[configured.motion]);
    message.put(0xdb, &[values.scale]);
    message.put(0xdc, &[configured.active]);
    message.put(0xdd, &[configured.view_mode]);
    message.put(0xdf, &[u8::from(configured.direct_start)]);
    message.put(0xe0, &[values.capture]);
    message.put(0xe1, &[values.rate]);
    message.word(0xe4, values.restarts);
}

/// 0xf31190..0xf312c4 writes the group at 0x48 and again at 0x6c.
fn value_group(message: &mut Encoded, offset: usize, group: &ValueGroup) {
    message.word(offset, group.first);
    message.halfword(offset + 4, group.second);
    message.put(offset + 6, &[group.third]);
    message.word(offset + 8, group.fourth);
    message.word(offset + 0xc, group.fifth);
    message.halfword(offset + 0x10, group.sixth);
    message.halfword(offset + 0x12, group.seventh);
    message.halfword(offset + 0x14, group.eighth);
    message.word(offset + 0x18, group.ninth);
    message.halfword(offset + 0x1c, group.tenth);
    message.word(offset + 0x20, group.eleventh);
}

fn words_at(message: &mut Encoded, offset: usize, words: &[u32]) {
    for (index, word) in words.iter().enumerate() {
        message.word(offset + 4 * index, *word);
    }
}

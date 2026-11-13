#![forbid(unsafe_code)]

//! X-M1 1.50 layout of saclac's imgdc requests: the S1 job block 0xf22f3c builds and the senders
//! 0xfeaa64 (0x3f42) and 0xfeaaa4 (0x3f43).

use super::Encoded;
use crate::imaging::*;
use crate::ImgDcOperation as Op;

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

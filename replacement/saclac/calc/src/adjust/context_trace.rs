//! The exposure context's trace into the client's dump frame (0xf6baa8): the program steps, the
//! snapshot's focus result, subjects and limits, the span or the adjustment path, each traced
//! buffer with its regions' channel levels, and the last evaluation request's statistics. A context
//! with no frame bound writes nothing.

use xm1_saclac_interface::calc::SharedAddress;

use crate::exposure::context::{ExposureContext, ExposureStep, PROGRAM_STEPS, SUBJECTS};
use crate::exposure::ports::{Ports, StockMemory};
use crate::selection::{RowPhase, ROWS};
use crate::stock::context_trace as dump;
use crate::stock::shared;
use crate::stock::shot_dump::at;

/// The pixels of each of the four channels in a region of `pixels` pixels (0xef56cc).
pub trait ChannelPixels {
    fn channel_pixels(&mut self, pixels: u32) -> [u32; 4];
}

/// Rows the adjustment path traces after its statistics block.
const ADJUSTMENT_ROWS: usize = 7;

fn write(memory: &mut impl StockMemory, frame: SharedAddress, offset: u32, word: u32) { memory.write_u32(at(frame, offset), word); }

const fn signed(half: i16) -> u32 { half as i32 as u32 }

fn copy(memory: &mut impl StockMemory, to: SharedAddress, from: SharedAddress, bytes: usize) {
    for offset in (0..bytes as u32).step_by(4) {
        let word = memory.read_u32(at(from, offset));
        memory.write_u32(at(to, offset), word);
    }
}

/// The low word of the quotient; a zero divisor gives all ones, or 0 for a zero dividend
/// (0x89bed0).
fn quotient(dividend: u64, divisor: u32) -> u32 {
    match dividend.checked_div(u64::from(divisor)) {
        Some(quotient) => quotient as u32,
        None if dividend == 0 => 0,
        None => u32::MAX,
    }
}

fn step_words(ports: &mut impl Ports, step: &ExposureStep, target: i16) -> [u32; dump::STEP_WORDS] {
    [
        step.lens_step,
        signed(ports.base_level(step.lens_step)),
        signed(step.level),
        signed(step.converted as i16),
        signed(target),
        signed(step.offset),
        signed(ports.level_offset(step.next_profile)),
        step.profile,
        step.next_profile,
    ]
}

fn focus_words(ports: &mut impl Ports, context: &ExposureContext) -> [u32; dump::FOCUS_WORDS] {
    let snapshot = &context.snapshot;
    if snapshot.valid == 0 {
        let mut words = [0; dump::FOCUS_WORDS];
        words[1] = 58;
        return words;
    }
    let focus = &snapshot.focus;
    let record = context.binding.record_handle();
    let index = ports.calibration_index(focus.lens);
    let terms = focus.terms.map(|term| signed(term as i16));
    [
        focus.reference,
        ports.profile().selection_kind(record, false),
        u32::from(ports.profile().selection_flags(record, false)),
        signed(snapshot.offset).wrapping_add(500),
        index,
        signed(ports.base_level(index)),
        terms[0],
        terms[1],
        terms[2],
        terms[3],
        terms[5],
        terms[4],
    ]
}

fn limit_words(context: &ExposureContext) -> [u32; dump::LIMIT_WORDS] {
    let snapshot = &context.snapshot;
    if snapshot.subjects.count == 0 {
        return [u32::MAX; dump::LIMIT_WORDS];
    }
    let [first, second, third, fourth, fifth, sixth] = snapshot.limits;
    let [bound_0, bound_1, bound_2, _] = snapshot.bounds;
    [signed(second), signed(third), signed(fourth), signed(first), signed(bound_0), signed(bound_1), signed(bound_2), signed(fifth), u32::from(sixth as u16), snapshot.limit_word]
}

fn write_subjects(memory: &mut impl StockMemory, frame: SharedAddress, context: &ExposureContext) {
    let subjects = &context.snapshot.subjects;
    write(memory, frame, dump::SUBJECT_COUNT, subjects.count);
    for (index, word) in limit_words(context).into_iter().enumerate() {
        write(memory, frame, dump::LIMITS + 4 * index as u32, word);
    }
    let listed = subjects.listed();
    for index in 0..SUBJECTS {
        let words = listed.get(index).map_or([0; 4], |subject| {
            let detail = |from: usize| u32::from_le_bytes([subject.detail[from], subject.detail[from + 1], subject.detail[from + 2], subject.detail[from + 3]]);
            [subject.id, detail(0), detail(8), detail(4)]
        });
        for (word_index, word) in words.into_iter().enumerate() {
            write(memory, frame, dump::subject(index) + 4 * word_index as u32, word);
        }
    }
}

/// The regions of one traced buffer: each region's size, and each of its four 64-bit sums in
/// `sums` divided by the pixels of its channel.
fn write_regions(
    ports: &mut (impl StockMemory + ChannelPixels),
    frame: SharedAddress,
    row: usize,
    slot: usize,
    sizes: &[[u32; 2]],
    sums: SharedAddress,
) {
    for (region, &[first, second]) in sizes.iter().enumerate() {
        write(ports, frame, dump::region(row, region), first | second << 16);
        let pixels = ports.channel_pixels(first.wrapping_mul(second));
        for (channel, channel_pixels) in pixels.into_iter().enumerate() {
            let sum = at(sums, dump::SUM_BYTES * region as u32 + 8 * channel as u32);
            let low = ports.read_u32(sum);
            let high = ports.read_u32(at(sum, 4));
            let level = quotient(u64::from(low) | u64::from(high) << 32, channel_pixels);
            write(ports, frame, dump::channel_level(slot, region, channel), level);
        }
    }
}

/// The region sizes the last evaluation request names.
fn requested_sizes(memory: &mut impl StockMemory, context: &ExposureContext, sizes: &mut [[u32; 2]; SUBJECTS]) -> usize {
    let request = &context.evaluation;
    let Some(regions) = request.column_regions else { return 0 };
    let count = usize::from(request.column_count as u16).min(SUBJECTS);
    for (index, size) in sizes.iter_mut().take(count).enumerate() {
        let entry = at(regions, 8 * index as u32);
        *size = [memory.read_u32(entry), memory.read_u32(at(entry, 4))];
    }
    count
}

/// The snapshot's statistics block in slot 0, with the subjects' region sizes from its table.
fn write_statistics_block(ports: &mut (impl StockMemory + ChannelPixels), frame: SharedAddress, context: &ExposureContext) {
    let snapshot = &context.snapshot;
    let Some(block) = snapshot.focus.statistics else {
        ports.fill(at(frame, dump::slot_buffer(0)), dump::SLOT_BUFFER_BYTES, 0);
        ports.fill(at(frame, dump::slot_tail(0)), dump::SLOT_TAIL_BYTES, 0);
        return;
    };
    copy(ports, at(frame, dump::slot_buffer(0)), block, dump::SLOT_BUFFER_BYTES);
    copy(ports, at(frame, dump::slot_tail(0)), at(block, dump::STATISTICS_TAIL), dump::SLOT_TAIL_BYTES);
    let count = snapshot.subjects.listed().len();
    let mut sizes = [[0; 2]; SUBJECTS];
    for (index, size) in sizes.iter_mut().take(count).enumerate() {
        *size = [snapshot.table_block[2 * index], snapshot.table_block[2 * index + 1]];
    }
    write_regions(ports, frame, 0, 0, &sizes[..count], at(block, dump::STATISTICS_SUMS));
}

/// Each enabled row's buffers from `first_slot` on, then the unused slots cleared.
fn write_rows(ports: &mut (impl StockMemory + ChannelPixels), frame: SharedAddress, context: &ExposureContext, rows: usize, first_slot: usize) {
    let mut sizes = [[0; 2]; SUBJECTS];
    let count = requested_sizes(ports, context, &mut sizes);
    let mut slot = first_slot;
    for (index, row) in context.selection.rows.iter().enumerate().take(rows) {
        if !row.enabled {
            continue;
        }
        let [buffer, tail] = shared::row_buffers(index);
        copy(ports, at(frame, dump::slot_buffer(slot)), buffer, dump::SLOT_BUFFER_BYTES);
        copy(ports, at(frame, dump::slot_tail(slot)), tail, dump::SLOT_TAIL_BYTES);
        write_regions(ports, frame, index, slot, &sizes[..count], shared::column_buffer(index));
        slot += 1;
    }
    for unused in slot..dump::SLOTS {
        ports.fill(at(frame, dump::slot_buffer(unused)), dump::SLOT_BUFFER_BYTES, 0);
        ports.fill(at(frame, dump::slot_tail(unused)), dump::SLOT_TAIL_BYTES, 0);
        ports.fill(at(frame, dump::channel_level(unused, 0, 0)), dump::CHANNEL_LEVEL_BYTES, 0);
        ports.fill(at(frame, dump::region(unused, 0)), dump::REGION_BYTES, 0);
    }
}

/// The candidates from the first whose rows are applied, up to the bank's count.
fn applied_candidates(context: &ExposureContext) -> u32 {
    let selection = &context.selection;
    selection.rows.iter().take(usize::from(selection.count)).take_while(|row| row.phase == RowPhase::Applied).count() as u32
}

fn write_span(memory: &mut impl StockMemory, frame: SharedAddress, context: &ExposureContext) {
    let span = &context.span;
    write(memory, frame, dump::PATH, 7);
    write(memory, frame, dump::SPAN_RANGE, (i32::from(span.low) << 16).wrapping_add(i32::from(span.high)) as u32);
    let first = u32::from(span.first);
    let words = [first, first.wrapping_sub(1).wrapping_add(applied_candidates(context)), signed(span.start), signed(span.end), signed(span.first_step), signed(span.step)];
    for (index, word) in words.into_iter().enumerate() {
        write(memory, frame, dump::SPAN + 4 * index as u32, word);
    }
}

fn clear_span(memory: &mut impl StockMemory, frame: SharedAddress) {
    write(memory, frame, dump::SPAN_RANGE, 0);
    for index in 0..dump::SPAN_WORDS as u32 {
        write(memory, frame, dump::SPAN + 4 * index, 0);
    }
}

pub fn trace(ports: &mut (impl Ports + ChannelPixels), context: &ExposureContext) {
    let Some(frame) = context.binding.frame else { return };
    let request = &context.evaluation;
    for (offset, [high, low]) in dump::MEASURED.into_iter().zip([request.measured[0], request.measured[2]]) {
        write(ports, frame, offset, u32::from(high) << 16 | u32::from(low));
    }
    let steps = usize::from(context.span.last).min(PROGRAM_STEPS);
    for (index, step) in context.program.iter().enumerate() {
        let words = if index < steps { step_words(ports, step, context.levels.target) } else { [0; dump::STEP_WORDS] };
        for (word_index, word) in words.into_iter().enumerate() {
            write(ports, frame, dump::step(index) + 4 * word_index as u32, word);
        }
    }
    if context.snapshot.valid == 0 {
        ports.fill(at(frame, dump::WORK_TABLE_COPY), 4 * dump::WORK_TABLE_HALVES, 0);
    } else {
        for index in 0..dump::WORK_TABLE_HALVES as u32 {
            let half = ports.read_u16(at(dump::WORK_TABLE, 2 * index)) as i16;
            write(ports, frame, dump::WORK_TABLE_COPY + 4 * index, signed(half));
        }
    }
    for (index, word) in focus_words(ports, context).into_iter().enumerate() {
        write(ports, frame, dump::FOCUS + 4 * index as u32, word);
    }
    write_subjects(ports, frame, context);
    if context.outcome.adjusting {
        clear_span(ports, frame);
        write_statistics_block(ports, frame, context);
        write_rows(ports, frame, context, ADJUSTMENT_ROWS, 1);
        if context.adjustment.state != 0 {
            write(ports, frame, dump::PATH, u32::from(context.adjustment.code));
        }
    } else {
        write_span(ports, frame, context);
        write_rows(ports, frame, context, ROWS, 0);
    }
    for (index, statistic) in request.statistics.into_iter().enumerate() {
        write(ports, frame, dump::STATISTICS + 4 * index as u32, signed(statistic as i16));
    }
    let no_regions = if context.outcome.adjusting { context.snapshot.subjects.count == 0 } else { context.selection.columns[0].count == 0 };
    if no_regions {
        for slot in 0..dump::SLOTS {
            ports.fill(at(frame, dump::channel_level(slot, 0, 0)), dump::CHANNEL_LEVEL_BYTES, 0);
        }
    }
}

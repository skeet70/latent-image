use core::cell::RefCell;
use xm1_saclac_calc::exposure::context::ExposureContext;
use xm1_saclac_interface::stock::{record_handle_from_a2, record_handle_word};
use xm1_shot_sequence::state::{StateAddress, StateMemory};
use xm1_shot_sequence::stock::calculation::{EXPOSURE_CONTEXT, INPUT, INPUT_BYTES};
use xm1_saclac_shared::stock::Machine;

/// ShotState shares the engine's exposure fields. Its active input is the same machine-backed
/// wire buffer the external calc actor reads; no typed input copy persists in ExposureContext.
pub struct ShotMemory<'a, M>(pub M, pub &'a RefCell<ExposureContext>);

/// The exposure context bytes reads and writes go through `context_word` or are refused, except `INPUT`.
const CONTEXT_END: u32 = EXPOSURE_CONTEXT + 0x126c;

/// Machine words from word-aligned `address` to the next exposure-context word `copy` cannot move directly.
#[inline(never)]
fn plain_words(address: u32) -> usize {
    let input_end = INPUT + INPUT_BYTES as u32;
    let end = if address < EXPOSURE_CONTEXT {
        u64::from(EXPOSURE_CONTEXT)
    } else if (INPUT..input_end).contains(&address) {
        u64::from(input_end)
    } else if address >= CONTEXT_END {
        1 << 32
    } else {
        return 0;
    };
    ((end - u64::from(address)) / 4) as usize
}

fn context_word(context: &ExposureContext, address: u32) -> Option<u32> {
    Some(match address.checked_sub(EXPOSURE_CONTEXT)? {
        0x04 => context.binding.mode,
        0x08 => context.binding.exposure_mode.0,
        0x10 => context.binding.lens_reference,
        0x14 => context.binding.source,
        0x18 => context.binding.option,
        0x1c => context.binding.snapshot_record.map_or(u32::MAX, record_handle_word),
        0x20 => context.binding.record.map_or(u32::MAX, record_handle_word),
        0x2c => context.levels.curve,
        // f624a8's level block starts at context +0x24; +0x0c is unmeasured.
        0x30 => u32::from(context.levels.unmeasured),
        0x47c => u32::from(context.binding.refinement_pass),
        // f61538..f61540 stores the focus-reuse result here.
        0x1268 => u32::from(context.outcome.focus_reused),
        _ => return None,
    })
}

fn set_context_word(context: &mut ExposureContext, address: u32, value: u32) {
    match address - EXPOSURE_CONTEXT {
        0x04 => context.binding.mode = value,
        0x08 => context.binding.exposure_mode.0 = value,
        0x10 => context.binding.lens_reference = value,
        0x14 => context.binding.source = value,
        0x18 => context.binding.option = value,
        0x1c => context.binding.snapshot_record = (value != u32::MAX).then(|| record_handle_from_a2(value)),
        0x20 => context.binding.record = (value != u32::MAX).then(|| record_handle_from_a2(value)),
        0x2c => context.levels.curve = value,
        0x30 => context.levels.unmeasured = value != 0,
        0x47c => context.binding.refinement_pass = value != 0,
        0x1268 => context.outcome.focus_reused = value != 0,
        _ => unreachable!("unmapped canonical exposure write"),
    }
}

impl<M: Machine> ShotMemory<'_, M> {
    fn read_at(&mut self, address: u32, into: &mut [u8]) {
        let context = self.1.borrow();
        let mut address = address;
        let mut into = into;
        while !into.is_empty() {
            let aligned = address & !3;
            let width = if let Some(value) = context_word(&context, aligned) {
                let offset = (address & 3) as usize;
                let width = into.len().min(4 - offset);
                into[..width].copy_from_slice(&value.to_le_bytes()[offset..offset + width]);
                width
            } else {
                assert!(!(EXPOSURE_CONTEXT..CONTEXT_END).contains(&address) || (INPUT..INPUT + INPUT_BYTES as u32).contains(&address), "unmapped canonical exposure read");
                let width = if address & 3 == 0 && into.len() >= 4 { 4 } else if address & 1 == 0 && into.len() >= 2 { 2 } else { 1 };
                let value = match width {
                    4 => self.0.read_u32(address),
                    2 => u32::from(self.0.read_u16(address)),
                    _ => u32::from(self.0.read_u8(address)),
                };
                into[..width].copy_from_slice(&value.to_le_bytes()[..width]);
                width
            };
            into = &mut into[width..];
            address += width as u32;
        }
    }

    fn write_at(&mut self, address: u32, bytes: &[u8]) {
        let mut context = self.1.borrow_mut();
        let mut address = address;
        let mut bytes = bytes;
        while !bytes.is_empty() {
            let aligned = address & !3;
            let width = if let Some(value) = context_word(&context, aligned) {
                let offset = (address & 3) as usize;
                let width = bytes.len().min(4 - offset);
                let mut word = value.to_le_bytes();
                word[offset..offset + width].copy_from_slice(&bytes[..width]);
                set_context_word(&mut context, aligned, u32::from_le_bytes(word));
                width
            } else {
                assert!(!(EXPOSURE_CONTEXT..CONTEXT_END).contains(&address) || (INPUT..INPUT + INPUT_BYTES as u32).contains(&address), "unmapped canonical exposure write");
                let width = if address & 3 == 0 && bytes.len() >= 4 { 4 } else if address & 1 == 0 && bytes.len() >= 2 { 2 } else { 1 };
                match width {
                    4 => self.0.write_u32(address, u32::from_le_bytes(bytes[..4].try_into().expect("state word"))),
                    2 => self.0.write_u16(address, u16::from_le_bytes(bytes[..2].try_into().expect("state halfword"))),
                    _ => self.0.write_u8(address, bytes[0]),
                }
                width
            };
            bytes = &bytes[width..];
            address += width as u32;
        }
    }
}

impl<M: Machine> StateMemory for ShotMemory<'_, M> {
    fn read(&mut self, at: StateAddress, into: &mut [u8]) { self.read_at(at.get(), into); }

    fn write(&mut self, at: StateAddress, bytes: &[u8]) { self.write_at(at.get(), bytes); }

    fn copy(&mut self, to: StateAddress, from: StateAddress, bytes: usize) {
        let (to, from) = (to.get(), from.get());
        let overlaps_forward = to > from && u64::from(to) < u64::from(from) + bytes as u64;
        let words_move = !overlaps_forward && (to | from) & 3 == 0;
        let mut offset = 0;
        while offset < bytes {
            let (source, destination) = (from + offset as u32, to + offset as u32);
            let run = if words_move { plain_words(source).min(plain_words(destination)).min((bytes - offset) / 4) } else { 0 };
            if run != 0 {
                for word in 0..run as u32 {
                    let value = self.0.read_u32(source + 4 * word);
                    self.0.write_u32(destination + 4 * word, value);
                }
                offset += 4 * run;
                continue;
            }
            // Preserve ascending byte-copy semantics when the destination overlaps ahead.
            let width = if overlaps_forward { 1 } else { (bytes - offset).min(4) };
            let mut buffer = [0; 4];
            self.read_at(source, &mut buffer[..width]);
            self.write_at(destination, &buffer[..width]);
            offset += width;
        }
    }
}

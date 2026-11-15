#![forbid(unsafe_code)]

//! The frame dumps: the pipeline's policy block and device registers copied into a caller's
//! dump buffer.

use super::{ProfileGeometry, Tables};
use crate::memory::Memory;
use crate::ports::{DeviceRegisters, DumpBuffer, FrameDumps, PolicyDump, Ports, Register};
use crate::stock::dumps::{pipeline, result, Repeated, Run};
use crate::stock::layout::policy;

impl<M: Memory, P: Ports, T: Tables> ProfileGeometry<M, P, T> {
    /// 0xedb554.
    pub fn dump_pipeline(&mut self, buffer: Option<DumpBuffer>) {
        let Some(buffer) = buffer else {
            return;
        };
        self.ports.dump_policy(buffer, PolicyDump::Pipeline, policy::SCAN);
        for (offset, mark) in pipeline::MARKS {
            self.ports.write_dump(buffer, offset, &mark);
        }
        for bank in &pipeline::BANKS {
            if self.ports.register(Register(pipeline::STATUS)) & bank.enable == 0 {
                continue;
            }
            for run in &bank.runs {
                copy_run(&mut self.ports, buffer, run);
            }
            for (index, register) in bank.chosen.iter().enumerate() {
                let word = self.ports.register(Register(*register));
                self.ports.write_dump(buffer, bank.chosen_offset + 4 * index as u32, &word.to_le_bytes());
            }
        }
    }

    /// 0xedb564.
    pub fn dump_result(&mut self, buffer: Option<DumpBuffer>) {
        dump_result(&mut self.ports, buffer);
    }

}

/// The result dump has no controller-local state. Callbacks can use the same owned
/// algorithm through narrow ports without borrowing the active controller or router.
pub fn dump_result<P: FrameDumps + DeviceRegisters>(ports: &mut P, buffer: Option<DumpBuffer>) {
    let Some(buffer) = buffer else { return };
    ports.dump_policy(buffer, PolicyDump::Result, policy::SCAN);
    let (offset, mark) = result::MARK;
    ports.write_dump(buffer, offset, &mark);
    let [first_run, second_run] = &result::RUNS;
    let [first_repeated, rest @ ..] = &result::REPEATED;
    copy_run(ports, buffer, first_run);
    copy_repeated(ports, buffer, first_repeated);
    copy_run(ports, buffer, second_run);
    for repeated in rest { copy_repeated(ports, buffer, repeated); }
}

fn copy_run<P: FrameDumps + DeviceRegisters>(ports: &mut P, buffer: DumpBuffer, run: &Run) {
    for index in 0..run.count as u32 {
        let word = ports.register(Register(run.register + 4 * index));
        ports.write_dump(buffer, run.offset + 4 * index, &word.to_le_bytes());
    }
}

/// Each word reads the first register again and adds its byte offset to it (0xedbe90).
fn copy_repeated<P: FrameDumps + DeviceRegisters>(ports: &mut P, buffer: DumpBuffer, repeated: &Repeated) {
    for index in 0..repeated.count as u32 {
        let word = ports.register(Register(repeated.register)).wrapping_add(4 * index);
        ports.write_dump(buffer, repeated.offset + 4 * index, &word.to_le_bytes());
    }
}

//! Diagnostic ownership of the X-M1 1.50 loader's task and interrupt stacks.

use crate::modal_task::{KernelId, TaskIdentity};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct StackDescriptor { pub bytes: u32, pub base: u32 }

pub(crate) struct Sample {
    pub core: usize,
    pub pc: u32,
    pub word: u32,
    pub sp: u32,
    pub cpsr: u32,
    pub r1: u32,
    pub r2: u32,
    pub current: TaskIdentity,
    pub stack: StackDescriptor,
    pub context_depth: u32,
}

#[derive(Debug)]
pub(crate) struct Attribution {
    pub owner: Option<TaskIdentity>,
    pub scheduler_transition: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Owner { identity: TaskIdentity, stack: StackDescriptor }
impl Owner {
    fn contains(self, sp: u32) -> bool {
        let base = self.stack.base & !0x4000_0000;
        let sp = sp & !0x4000_0000;
        self.stack.bytes != 0 && base < 0x2000_0000 && sp % 4 == 0
            && base.checked_add(self.stack.bytes).is_some_and(|end| end <= 0x2000_0000 && base <= sp && sp <= end)
    }
}

#[derive(Clone, Copy)]
struct Dispatch {
    next: u32,
    selected: Option<u32>,
    incoming: Option<Owner>,
    depth: u32,
}

#[derive(Clone, Copy, Default)]
enum IrqStack { #[default] Task, Entering, Shared, Restoring }

#[derive(Clone, Copy, Default)]
struct Context {
    owner: Option<Owner>,
    dispatch: Option<Dispatch>,
    irq: IrqStack,
}

#[derive(Default)]
pub(crate) struct Attributor { cores: [Context; 2] }

impl Attributor {
    pub(crate) fn observe(&mut self, sample: Sample) -> Result<Attribution, String> {
        let kernel = match sample.core {
            0 => KernelId::Primary,
            1 => KernelId::Secondary,
            _ => return Err("unsupported stack attribution core".into()),
        };
        if sample.current.kernel != kernel {
            return Err("current task belongs to another core's kernel".into());
        }
        let mut context = self.cores[sample.core];
        let result = context.observe(&sample)?;
        self.cores[sample.core] = context;
        Ok(result)
    }
}

impl Context {
    fn observe(&mut self, sample: &Sample) -> Result<Attribution, String> {
        let mode = sample.cpsr & 0x1f;
        if sample.cpsr & ((1 << 24) | (1 << 9)) != 0 {
            return Err("unsupported instruction state or endianness".into());
        }
        if matches!(mode, 0x11 | 0x12 | 0x17 | 0x1b) {
            return Ok(Attribution { owner: None, scheduler_transition: false });
        }
        if mode != 0x13 {
            return Err("unsupported task stack bank".into());
        }
        let incoming = Owner { identity: sample.current, stack: sample.stack };
        let offset = if sample.core == 0 { 0 } else { 0x90000 };
        let pc = sample.pc.checked_sub(offset).unwrap_or(sample.pc);
        let mut transition = false;

        // loader-40000.dis and loader-d0000.dis: 40250/4032c change the SVC SP.
        match self.irq {
            IrqStack::Entering => {
                if pc != 0x40254 || sample.sp != 0x7000_2000 || sample.context_depth == 0 {
                    return Err("unproved IRQ shared-stack entry".into());
                }
                self.irq = IrqStack::Shared;
                transition = true;
            }
            IrqStack::Restoring => {
                if pc != 0x40330 { return Err("unproved IRQ task-stack restoration".into()); }
                self.irq = IrqStack::Task;
                transition = true;
            }
            _ => {}
        }
        if matches!(self.irq, IrqStack::Shared) {
            if self.dispatch.is_some() || self.owner != Some(incoming)
                || sample.sp % 4 != 0 || !(0x7000_0000..=0x7000_2000).contains(&sample.sp)
            {
                return Err("ambiguous IRQ shared-stack ownership".into());
            }
            if pc == 0x4032c {
                require_word(sample, 0xe1a0d000)?;
                self.irq = IrqStack::Restoring;
                transition = true;
            }
            return Ok(Attribution { owner: None, scheduler_transition: transition });
        }

        if self.owner.is_none() {
            if !incoming.contains(sample.sp) { return Err("initial SP is not on the current task stack".into()); }
            self.owner = Some(incoming);
        }
        if self.dispatch.is_none() && pc == 0x436ac {
            self.dispatch = Some(Dispatch { next: pc, selected: None, incoming: None, depth: sample.context_depth });
        }
        if self.dispatch.is_none()
            && ((0x436b0..=0x436e8).contains(&pc) || (0x4252c..=0x42550).contains(&pc))
        {
            return Err("scheduler phase has no observed entry".into());
        }
        if let Some(mut dispatch) = self.dispatch {
            if pc != dispatch.next || sample.context_depth != dispatch.depth || sample.cpsr & 0xa0 != 0x80 {
                return Err("unexpected scheduler phase, depth, or instruction state".into());
            }
            let (word, mut next) = dispatch_instruction(pc)?;
            require_word(sample, word)?;
            if pc == 0x436b8 {
                if sample.r1 == 0 || physical(sample.r2) != physical(sample.current.kernel.current_cell().0) {
                    return Err("scheduler selected node or current cell is invalid".into());
                }
                dispatch.selected = Some(physical(sample.r1));
            }
            if matches!(pc, 0x436e4 | 0x436e8) && dispatch.selected != Some(physical(sample.r1)) {
                return Err("SP handoff register does not name the selected task".into());
            }
            if pc >= 0x436bc && pc <= 0x436e8 || (0x4252c..=0x42550).contains(&pc) {
                if dispatch.selected != Some(physical(sample.current.node.address().0)) {
                    return Err("current task does not match the source-selected node".into());
                }
                match dispatch.incoming {
                    None => dispatch.incoming = Some(incoming),
                    Some(expected) if expected == incoming => {},
                    _ => return Err("scheduler target descriptor changed".into()),
                }
            } else if self.owner != Some(incoming) {
                return Err("current task changed before the source-bound store".into());
            }
            if pc == 0x436e8 {
                self.owner = dispatch.incoming;
                self.dispatch = None;
            } else {
                if pc == 0x4253c && sample.cpsr & (1 << 30) != 0 { next = 0x42548; }
                dispatch.next = next;
                self.dispatch = Some(dispatch);
            }
            transition = true;
        } else if self.owner != Some(incoming) {
            return Err("current task changed without a source-bound scheduler handoff".into());
        }
        let owner = self.owner.ok_or("missing task stack owner")?;
        if !owner.contains(sample.sp) { return Err("SP is outside the source-attributed task stack".into()); }
        if pc == 0x40250 {
            require_word(sample, 0xe59fd118)?;
            if sample.context_depth == 0 || self.dispatch.is_some() {
                return Err("ambiguous IRQ stack handoff".into());
            }
            self.irq = IrqStack::Entering;
            transition = true;
        }
        Ok(Attribution { owner: Some(owner.identity), scheduler_transition: transition })
    }
}

fn physical(address: u32) -> u32 { address & !0x4000_0000 }

fn require_word(sample: &Sample, expected: u32) -> Result<(), String> {
    if sample.word != expected { return Err("instruction differs from the source-bound stack transition".into()); }
    Ok(())
}

// Both loader disassemblies, lines 2358-2367 and 3473-3488, have identical words.
fn dispatch_instruction(pc: u32) -> Result<(u32, u32), String> {
    Ok(match pc {
        0x436ac => (0xe59f0150, 0x436b0),
        0x436b0 => (0xe5901000, 0x436b4),
        0x436b4 => (0xe59f2140, 0x436b8),
        0x436b8 => (0xe5821000, 0x436bc),
        0x436bc => (0xe3510000, 0x436c0),
        0x436c0 => (0x0a000009, 0x436c4),
        0x436c4 => (0xe92d100f, 0x436c8),
        0x436c8 => (0xe92d0002, 0x436cc),
        0x436cc => (0xe5911008, 0x436d0),
        0x436d0 => (0xe5911020, 0x436d4),
        0x436d4 => (0xee0d1f50, 0x436d8),
        0x436d8 => (0xe8bd0002, 0x436dc),
        0x436dc => (0xebfffb92, 0x4252c),
        0x4252c => (0xe52db004, 0x42530),
        0x42530 => (0xe28db000, 0x42534),
        0x42534 => (0xee113f50, 0x42538),
        0x42538 => (0xe313060f, 0x4253c),
        0x4253c => (0x0a000001, 0x42540),
        0x42540 => (0xe3a03000, 0x42544),
        0x42544 => (0xeee83a10, 0x42548),
        0x42548 => (0xe28bd000, 0x4254c),
        0x4254c => (0xe8bd0800, 0x42550),
        0x42550 => (0xe12fff1e, 0x436e0),
        0x436e0 => (0xe8bd100f, 0x436e4),
        0x436e4 => (0xe591d018, 0x436e8),
        0x436e8 => (0xe591f01c, 0),
        _ => return Err("unknown scheduler stack phase".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::{Attributor, Sample, StackDescriptor};
    use crate::{modal_permit::CoreId, modal_task::{self, KernelId, MemoryError, StackExtent, TaskIdentity, ValidatedTaskMemory}, provenance::Guest};
    use std::collections::BTreeMap;

    struct Memory(BTreeMap<u32, u8>);
    impl Memory {
        fn word(&mut self, address: u32, value: u32) {
            for (index, byte) in value.to_le_bytes().into_iter().enumerate() {
                self.0.insert(address + index as u32, byte);
            }
        }
    }
    impl ValidatedTaskMemory for Memory {
        fn read(&self, address: Guest, output: &mut [u8]) -> Result<(), MemoryError> {
            for (index, byte) in output.iter_mut().enumerate() {
                *byte = *self.0.get(&(address.0 + index as u32)).ok_or(MemoryError::Unmapped)?;
            }
            Ok(())
        }
        fn validate_stack(&self, _: StackExtent) -> Result<(), MemoryError> { Ok(()) }
    }
    fn task(core: usize, index: u32, base: u32) -> (TaskIdentity, [u32; 2]) {
        let (kernel, nodes, creations) = if core == 0 {
            (KernelId::Primary, 0x4005ff0c, 0x4005beac)
        } else { (KernelId::Secondary, 0x400efecc, 0x400ebe6c) };
        let mut memory = Memory(BTreeMap::new());
        memory.word(kernel.current_cell().0, nodes + 56 * index);
        memory.word(kernel.context_cell().0, 3);
        memory.word(nodes + 56 * index + 8, creations + 40 * index);
        for field in 0..10 { memory.word(creations + 40 * index + 4 * field, 0); }
        memory.word(creations + 40 * index + 16, 0x1000);
        memory.word(creations + 40 * index + 20, base);
        memory.word(creations + 40 * index + 32, index + 1);
        memory.word(creations + 40 * index + 36, 1);
        let sampled = modal_task::sample_context(&memory, CoreId(core)).unwrap();
        assert_eq!(sampled.context_depth, 3);
        (sampled.identity, [sampled.descriptor[4], sampled.descriptor[5]])
    }
    fn sample(core: usize, task: (TaskIdentity, [u32; 2]), pc: u32, word: u32) -> Sample {
        Sample {
            core, current: task.0, stack: StackDescriptor { bytes: task.1[0], base: task.1[1] }, pc: pc + if core == 0 { 0 } else { 0x90000 },
            word, sp: task.1[1] + 0x800, cpsr: 0x93, r1: task.0.node.address().0,
            r2: task.0.kernel.current_cell().0, context_depth: 3,
        }
    }

    #[test]
    fn retains_old_stack_through_helper_then_attributes_loaded_stack_on_both_cores() {
        for core in 0..2 {
            for skip_vfp in [false, true] {
                let old = task(core, 0, 0x41000000);
                let new = task(core, 1, 0x42000000);
                let mut monitor = Attributor::default();
                let prefix = [
                    (0x436ac, 0xe59f0150), (0x436b0, 0xe5901000),
                    (0x436b4, 0xe59f2140), (0x436b8, 0xe5821000),
                    (0x436bc, 0xe3510000), (0x436c0, 0x0a000009),
                    (0x436c4, 0xe92d100f), (0x436c8, 0xe92d0002),
                    (0x436cc, 0xe5911008), (0x436d0, 0xe5911020),
                    (0x436d4, 0xee0d1f50), (0x436d8, 0xe8bd0002),
                    (0x436dc, 0xebfffb92), (0x4252c, 0xe52db004),
                    (0x42530, 0xe28db000), (0x42534, 0xee113f50),
                    (0x42538, 0xe313060f), (0x4253c, 0x0a000001),
                ];
                let suffix = [
                    (0x42548, 0xe28bd000), (0x4254c, 0xe8bd0800),
                    (0x42550, 0xe12fff1e), (0x436e0, 0xe8bd100f),
                    (0x436e4, 0xe591d018), (0x436e8, 0xe591f01c),
                ];
                let vfp = [(0x42540, 0xe3a03000), (0x42544, 0xeee83a10)];
                for (pc, word) in prefix.into_iter().chain(vfp.into_iter().take(if skip_vfp { 0 } else { 2 })).chain(suffix) {
                    let current = if pc < 0x436bc && pc >= 0x436ac { old } else { new };
                    let mut observation = sample(core, current, pc, word);
                    observation.r1 = new.0.node.address().0;
                    observation.sp = if pc == 0x436e8 { new.1[1] + 0x800 } else { old.1[1] + 0x700 };
                    if pc == 0x4253c && skip_vfp { observation.cpsr |= 1 << 30; }
                    if pc == 0x42534 {
                        let mut exception = sample(core, new, 0x40184, 0);
                        exception.cpsr = 0xd1;
                        exception.sp = 0x70001000;
                        assert_eq!(monitor.observe(exception).unwrap().owner, None);
                    }
                    if pc == 0x436e4 {
                        let premature = sample(core, new, pc, word);
                        assert!(monitor.observe(premature).is_err());
                    }
                    let result = monitor.observe(observation).unwrap();
                    assert_eq!(result.owner, Some(if pc == 0x436e8 { new.0 } else { old.0 }));
                    assert!(result.scheduler_transition);
                }
                assert_eq!(monitor.observe(sample(core, new, 0x437c0, 0xe8bd0001)).unwrap().owner, Some(new.0));
            }
        }
    }

    #[test]
    fn masked_irq_and_nonzero_context_remain_task_owned_across_exception_banks() {
        let task = task(0, 0, 0x41000000);
        let mut monitor = Attributor::default();
        assert_eq!(monitor.observe(sample(0, task, 0x123400, 0)).unwrap().owner, Some(task.0));
        for mode in [0x11, 0x12, 0x17, 0x1b] {
            let mut exception = sample(0, task, 0x40204, 0);
            exception.cpsr = mode | 0xc0;
            exception.sp = 0x70002000;
            let result = monitor.observe(exception).unwrap();
            assert_eq!(result.owner, None);
        }
        let mut restored = sample(0, task, 0x123404, 0);
        restored.sp &= !0x40000000;
        assert_eq!(monitor.observe(restored).unwrap().owner, Some(task.0));
    }

    #[test]
    fn shared_irq_stack_requires_observed_handoff_and_restores_task_ownership() {
        let task = task(0, 0, 0x41000000);
        let mut monitor = Attributor::default();
        monitor.observe(sample(0, task, 0x40250, 0xe59fd118)).unwrap();
        let mut shared = sample(0, task, 0x40254, 0xe92d0004);
        shared.sp = 0x70002000;
        assert_eq!(monitor.observe(shared).unwrap().owner, None);
        let mut returning = sample(0, task, 0x4032c, 0xe1a0d000);
        returning.sp = 0x70002000;
        assert_eq!(monitor.observe(returning).unwrap().owner, None);
        assert_eq!(monitor.observe(sample(0, task, 0x40330, 0xe59f10f0)).unwrap().owner, Some(task.0));
        let mut unproved = sample(0, task, 0x123400, 0);
        unproved.sp = 0x70002000;
        assert!(monitor.observe(unproved).is_err());
    }

    #[test]
    fn refuses_wrong_core_unexplained_current_and_ambiguous_scheduler_phase() {
        let old = task(0, 0, 0x41000000);
        let new = task(0, 1, 0x42000000);
        let mut monitor = Attributor::default();
        monitor.observe(sample(0, old, 0x123400, 0)).unwrap();
        assert!(monitor.observe(sample(0, new, 0x123404, 0)).is_err());
        let mut wrong_core = sample(1, old, 0x40204, 0);
        wrong_core.cpsr = 0x12;
        assert!(monitor.observe(wrong_core).is_err());
        assert!(monitor.observe(sample(2, old, 0x123400, 0)).is_err());
        let mut fresh = Attributor::default();
        assert!(fresh.observe(sample(0, old, 0x4252c, 0xe52db004)).is_err());
        monitor.observe(sample(0, old, 0x436ac, 0xe59f0150)).unwrap();
        assert!(monitor.observe(sample(0, new, 0x4252c, 0xe52db004)).is_err());
        assert!(monitor.observe(sample(0, old, 0x436b0, 0)).is_err());
        let mut wrong_mode = sample(0, old, 0x123400, 0);
        wrong_mode.cpsr = 0x1f;
        assert!(monitor.observe(wrong_mode).is_err());
    }
}

//! Cross-core exclusive monitor probe on the X-M1 board.
//!
//! A synthetic boot ROM releases core 1 and both cores run the same worker: `Iterations`
//! passes of a spin lock on one DRAM word around a plain read-modify-write of a shared
//! counter. The lock has the RTOS lock pair's shape: LDREX, compare, STREXEQ, test,
//! branch and a CP15 DMB after acquire; CP15 DMB, store 0 and CP15 DSB on release. The
//! must-fail control is the same sequence with LDR for LDREX and STREQ for STREXEQ.
//!
//! Two detectors: lost counter updates, and per-core holder flags. Each holder stores its
//! own flag, then loads the peer's, so whenever two counter updates overlap at least one
//! holder records a violation.
//!
//! Exact policy is deterministic and the ROM fixes core 1's release phase, so each quantum
//! is one schedule.

use fujimod_xm1::machine::Machine;
use fujimod_xm1::run::{self, RunDocument, ValidatedUpdate};

const RUN: &[u8] = include_bytes!("../data/xm1-150.json");
const ROM_BYTES: usize = 16384;
/// Core 1's reset PC in the catalog SoC; a page core 1 maps at its own physical address.
const CORE1_RESET: u64 = 0x3fff_0000;
/// Software pending word of core 1's interrupt controller; bit 24 is line 120, the release pulse.
const CORE1_RELEASE_WORD: u32 = 0xfffb_40cc;
/// The shared block as both cores address it (DRAM view, MPU off) and as the host reads it.
const SHARED_VIRTUAL: u32 = 0x4010_0000;
const SHARED_PHYSICAL: u64 = 0x0010_0000;
const LOCK: u32 = 0;
const COUNTER: u32 = 4;
const CORE_BLOCK: [u32; 2] = [16, 32];
const DONE: u32 = 0;
/// Failed STREXEQ, as distinct from a busy lock: the lock holds `HELD`, a failure returns 1.
const LOSSES: u32 = 4;
const FLAG: u32 = 8;
const VIOLATIONS: u32 = 12;
const SHARED_BYTES: usize = 48;
const HELD: u32 = 2;
const WINDOW: u64 = 200_000;
const BUDGET: u64 = 40_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Acquire {
    Exclusive,
    PlainControl,
}

#[derive(Clone, Copy, Debug)]
struct Iterations(u32);

#[derive(Clone, Copy, Debug)]
struct Quantum(usize);

/// What the host reads back once both cores report done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    MutuallyExclusive { reservation_losses: [u32; 2] },
    Raced { counter: u32, violations: [u32; 2], lock: u32, flags: [u32; 2] },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Condition {
    Always = 0xe,
    NotEqual = 0x1,
}

#[derive(Clone, Copy)]
struct Label(usize);

/// A32 words with branch and literal fixups resolved by `finish`.
#[derive(Default)]
struct Program {
    words: Vec<u32>,
    branches: Vec<(usize, Condition, Label)>,
    literals: Vec<(usize, u32, u32)>,
    labels: Vec<Option<usize>>,
}

impl Program {
    fn label(&mut self) -> Label {
        self.labels.push(None);
        Label(self.labels.len() - 1)
    }
    fn bind(&mut self, label: Label) {
        self.labels[label.0] = Some(self.words.len());
    }
    fn here(&mut self) -> Label {
        let label = self.label();
        self.bind(label);
        label
    }
    fn emit(&mut self, word: u32) {
        self.words.push(word);
    }
    fn branch(&mut self, condition: Condition, target: Label) {
        self.branches.push((self.words.len(), condition, target));
        self.words.push(0);
    }
    /// `ldr rt, [pc, #literal]`.
    fn load(&mut self, rt: u32, value: u32) {
        self.literals.push((self.words.len(), rt, value));
        self.words.push(0);
    }
    fn finish(mut self) -> Vec<u32> {
        for (at, condition, target) in &self.branches {
            let target = self.labels[target.0].expect("bound label") as i64;
            let offset = (target - (*at as i64 + 2)) as u32 & 0x00ff_ffff;
            self.words[*at] = (*condition as u32) << 28 | 0x0a00_0000 | offset;
        }
        let pool = self.words.len();
        for (index, (at, rt, value)) in self.literals.clone().into_iter().enumerate() {
            let offset = 4 * (pool + index - (at + 2));
            assert!(offset < 4096);
            self.words[at] = 0xe59f_0000 | rt << 12 | offset as u32;
            self.words.push(value);
        }
        self.words
    }
}

/// r11 = shared block, r10 = this core's block, r12 = the peer's flag. Both acquire forms
/// have the same instruction count.
fn worker(program: &mut Program, acquire: Acquire, iterations: Iterations) {
    let busy = program.label();
    program.emit(0xe3a0_0000); // mov r0, #0
    program.emit(0xe3a0_2000 | HELD); // mov r2, #HELD
    program.load(4, iterations.0);
    let pass = program.here();
    match acquire {
        Acquire::Exclusive => {
            program.emit(0xe19b_6f9f); // ldrex r6, [r11]
            program.emit(0xe356_0000); // cmp r6, #0
            program.emit(0x018b_6f92); // strexeq r6, r2, [r11]
        }
        Acquire::PlainControl => {
            program.emit(0xe59b_6000); // ldr r6, [r11]
            program.emit(0xe356_0000); // cmp r6, #0
            program.emit(0x058b_2000); // streq r2, [r11]
        }
    }
    program.emit(0xe1a0_5006); // mov r5, r6
    program.emit(0xe355_0000); // cmp r5, #0
    program.branch(Condition::NotEqual, busy);
    program.emit(0xee07_0fba); // mcr p15, 0, r0, c7, c10, 5 (DMB)
    program.emit(0xe3a0_7001); // mov r7, #1
    program.emit(0xe58a_7000 | FLAG); // str r7, [r10, #FLAG]
    program.emit(0xe59c_7000); // ldr r7, [r12]
    program.emit(0xe357_0000); // cmp r7, #0
    program.emit(0x159a_9000 | VIOLATIONS); // ldrne r9, [r10, #VIOLATIONS]
    program.emit(0x1289_9001); // addne r9, r9, #1
    program.emit(0x158a_9000 | VIOLATIONS); // strne r9, [r10, #VIOLATIONS]
    program.emit(0xe59b_7000 | COUNTER); // ldr r7, [r11, #COUNTER]
    program.emit(0xe287_7001); // add r7, r7, #1
    program.emit(0xe58b_7000 | COUNTER); // str r7, [r11, #COUNTER]
    program.emit(0xe58a_0000 | FLAG); // str r0, [r10, #FLAG]
    program.emit(0xee07_0fba); // mcr p15, 0, r0, c7, c10, 5 (DMB)
    program.emit(0xe58b_0000 | LOCK); // str r0, [r11, #LOCK]
    program.emit(0xee07_0f9a); // mcr p15, 0, r0, c7, c10, 4 (DSB)
    program.emit(0xe254_4001); // subs r4, r4, #1
    program.branch(Condition::NotEqual, pass);
    program.emit(0xe3a0_7001); // mov r7, #1
    program.emit(0xe58a_7000 | DONE); // str r7, [r10, #DONE]
    let spin = program.here();
    program.branch(Condition::Always, spin);
    program.bind(busy);
    program.emit(0xe355_0001); // cmp r5, #1
    program.emit(0x059a_8000 | LOSSES); // ldreq r8, [r10, #LOSSES]
    program.emit(0x0288_8001); // addeq r8, r8, #1
    program.emit(0x058a_8000 | LOSSES); // streq r8, [r10, #LOSSES]
    program.branch(Condition::Always, pass);
}

/// Core 0 enters at 0; core 1's reset stub jumps to the returned word index.
fn rom(acquire: Acquire, iterations: Iterations) -> (Vec<u32>, usize) {
    let mut program = Program::default();
    let shared = program.label();
    program.load(11, SHARED_VIRTUAL);
    program.load(12, CORE1_RELEASE_WORD);
    program.emit(0xe3a0_3401); // mov r3, #0x01000000
    program.emit(0xe58c_3000); // str r3, [r12]
    program.emit(0xe28b_a000 | CORE_BLOCK[0]); // add r10, r11, #CORE_BLOCK0
    program.emit(0xe28b_c000 | (CORE_BLOCK[1] + FLAG)); // add r12, r11, #CORE_BLOCK1+FLAG
    program.branch(Condition::Always, shared);
    let core1 = program.words.len();
    program.load(11, SHARED_VIRTUAL);
    program.emit(0xe28b_a000 | CORE_BLOCK[1]); // add r10, r11, #CORE_BLOCK1
    program.emit(0xe28b_c000 | (CORE_BLOCK[0] + FLAG)); // add r12, r11, #CORE_BLOCK0+FLAG
    program.branch(Condition::Always, shared);
    program.bind(shared);
    worker(&mut program, acquire, iterations);
    (program.finish(), core1)
}

fn machine(acquire: Acquire, iterations: Iterations, quantum: Quantum) -> Machine {
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let mut document = RunDocument::parse(RUN).unwrap();
    document.execution.quantum = quantum.0;
    let (words, core1) = rom(acquire, iterations);
    let mut image = vec![0u8; ROM_BYTES];
    for (index, word) in words.iter().enumerate() {
        image[4 * index..4 * index + 4].copy_from_slice(&word.to_le_bytes());
    }
    let update = ValidatedUpdate(vec![0xff; document.update_flash_offset + 2048]);
    let mut session = run::open(&document, &update, &image).unwrap();
    session.board.set_execution_policy(document.execution_policy.into()).unwrap();
    // ldr pc, [pc, #-4]; .word core-1 entry in the ROM's low view.
    let stub = [0xe51f_f004u32, 4 * core1 as u32];
    session.board.write_physical(CORE1_RESET, &stub.map(u32::to_le_bytes).concat()).unwrap();
    session.board.write_physical(SHARED_PHYSICAL, &[0; SHARED_BYTES]).unwrap();
    Machine::new(document, session).unwrap()
}

fn probe(acquire: Acquire, iterations: Iterations, quantum: Quantum) -> Outcome {
    let mut machine = machine(acquire, iterations, quantum);
    let word = |machine: &Machine, offset: u32| machine.word(SHARED_PHYSICAL + u64::from(offset)).unwrap();
    let per_core = |machine: &Machine, field: u32| CORE_BLOCK.map(|block| word(machine, block + field));
    while per_core(&machine, DONE).contains(&0) {
        assert!(machine.instructions() < BUDGET, "{acquire:?} at {quantum:?} did not finish: {:?}", machine.session.board.instruction_counts());
        machine.run(WINDOW);
        machine.require_running().unwrap();
    }
    let counts = machine.session.board.instruction_counts();
    assert!(counts.iter().all(|&count| count > 0), "both cores execute: {counts:?}");
    let (counter, violations, lock, flags) =
        (word(&machine, COUNTER), per_core(&machine, VIOLATIONS), word(&machine, LOCK), per_core(&machine, FLAG));
    if counter == 2 * iterations.0 && violations == [0, 0] && lock == 0 && flags == [0, 0] {
        Outcome::MutuallyExclusive { reservation_losses: per_core(&machine, LOSSES) }
    } else {
        Outcome::Raced { counter, violations, lock, flags }
    }
}

fn quanta() -> [Quantum; 4] {
    let document = RunDocument::parse(RUN).unwrap();
    [Quantum(1), Quantum(3), Quantum(7), Quantum(document.execution.quantum)]
}

const ITERATIONS: Iterations = Iterations(2000);

#[test]
fn exclusive_spin_lock_excludes_the_other_core_at_the_probed_quanta() {
    for quantum in quanta() {
        let outcome = probe(Acquire::Exclusive, ITERATIONS, quantum);
        eprintln!("exclusive {quantum:?}: {outcome:?}");
        let Outcome::MutuallyExclusive { reservation_losses } = outcome else {
            panic!("{quantum:?}: {outcome:?}");
        };
        // In lockstep each core reaches STREXEQ after the peer's successful one at least once.
        if quantum.0 == 1 {
            assert!(reservation_losses.iter().all(|&losses| losses > 0), "{outcome:?}");
        }
    }
}

#[test]
fn plain_test_and_set_control_admits_two_holders_at_the_probed_quanta() {
    for quantum in quanta() {
        let outcome = probe(Acquire::PlainControl, ITERATIONS, quantum);
        eprintln!("plain control {quantum:?}: {outcome:?}");
        assert!(
            matches!(outcome, Outcome::Raced { violations, .. } if violations.iter().sum::<u32>() > 0),
            "{quantum:?}: {outcome:?}"
        );
    }
}

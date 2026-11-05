//! Cross-core exclusive monitor probe on the X-M1 board.
//!
//! A synthetic boot ROM releases core 1 and both cores run the same worker: `Iterations`
//! passes of a spin lock on one DRAM word around a plain read-modify-write of a shared
//! counter. Inside the critical section each holder counts the holders present and records
//! a violation when it is not alone. The lock is taken with LDREX/STREX, or, as the
//! must-fail control, with a plain LDR/STR test-and-set of the same instruction count.

use fujimod_xm1::machine::Machine;
use fujimod_xm1::run::{self, RunDocument, ValidatedUpdate};

const RUN: &[u8] = include_bytes!("../data/xm1-150.json");
const ROM_BYTES: usize = 16384;
/// Core 1's reset PC in the catalog SoC; a page core 1 maps at its own physical address.
const CORE1_RESET: u64 = 0x3fff_0000;
/// Software pending word of core 1's interrupt controller; bit 24 is line 120, the release pulse.
const CORE1_RELEASE_WORD: u32 = 0xfffb_40cc;
/// The shared block as both cores address it (DRAM view) and as the host reads it.
const SHARED_VIRTUAL: u32 = 0x4010_0000;
const SHARED_PHYSICAL: u64 = 0x0010_0000;
const LOCK: u32 = 0;
const COUNTER: u32 = 4;
const HOLDERS: u32 = 8;
const VIOLATIONS: u32 = 12;
const DONE: [u32; 2] = [16, 20];
/// Per-core count of failed STREX, at `DONE[core] + LOSSES_FROM_DONE`.
const LOSSES_FROM_DONE: u32 = 8;
const SHARED_BYTES: usize = 32;
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
    Raced { counter: u32, violations: u32, holders: u32, lock: u32 },
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

/// r11 = shared block, r10 = this core's done word, r2 = remaining passes. Both acquire forms
/// have the same instruction count.
fn worker(program: &mut Program, acquire: Acquire, iterations: Iterations) {
    program.load(2, iterations.0);
    let pass = program.here();
    match acquire {
        Acquire::Exclusive => {
            program.emit(0xe19b_3f9f); // ldrex r3, [r11]
            program.emit(0xe353_0000); // cmp r3, #0
            program.branch(Condition::NotEqual, pass);
            program.emit(0xe3a0_3001); // mov r3, #1
            program.emit(0xe18b_6f93); // strex r6, r3, [r11]
            program.emit(0xe356_0000); // cmp r6, #0
        }
        Acquire::PlainControl => {
            program.emit(0xe59b_3000); // ldr r3, [r11]
            program.emit(0xe353_0000); // cmp r3, #0
            program.branch(Condition::NotEqual, pass);
            program.emit(0xe3a0_3001); // mov r3, #1
            program.emit(0xe58b_3000); // str r3, [r11]
            program.emit(0xe3b0_6000); // movs r6, #0
        }
    }
    program.emit(0x159a_8000 | LOSSES_FROM_DONE); // ldrne r8, [r10, #LOSSES_FROM_DONE]
    program.emit(0x1288_8001); // addne r8, r8, #1
    program.emit(0x158a_8000 | LOSSES_FROM_DONE); // strne r8, [r10, #LOSSES_FROM_DONE]
    program.branch(Condition::NotEqual, pass);
    program.emit(0xe59b_7000 | HOLDERS); // ldr r7, [r11, #HOLDERS]
    program.emit(0xe287_7001); // add r7, r7, #1
    program.emit(0xe58b_7000 | HOLDERS); // str r7, [r11, #HOLDERS]
    program.emit(0xe357_0001); // cmp r7, #1
    program.emit(0x159b_9000 | VIOLATIONS); // ldrne r9, [r11, #VIOLATIONS]
    program.emit(0x1289_9001); // addne r9, r9, #1
    program.emit(0x158b_9000 | VIOLATIONS); // strne r9, [r11, #VIOLATIONS]
    program.emit(0xe59b_7000 | COUNTER); // ldr r7, [r11, #COUNTER]
    program.emit(0xe287_7001); // add r7, r7, #1
    program.emit(0xe58b_7000 | COUNTER); // str r7, [r11, #COUNTER]
    program.emit(0xe59b_7000 | HOLDERS); // ldr r7, [r11, #HOLDERS]
    program.emit(0xe247_7001); // sub r7, r7, #1
    program.emit(0xe58b_7000 | HOLDERS); // str r7, [r11, #HOLDERS]
    program.emit(0xe3a0_3000); // mov r3, #0
    program.emit(0xe58b_3000 | LOCK); // str r3, [r11, #LOCK]
    program.emit(0xe252_2001); // subs r2, r2, #1
    program.branch(Condition::NotEqual, pass);
    program.emit(0xe3a0_3001); // mov r3, #1
    program.emit(0xe58a_3000); // str r3, [r10]
    let spin = program.here();
    program.branch(Condition::Always, spin);
}

/// Core 0 enters at 0; core 1's reset stub jumps to the returned word index.
fn rom(acquire: Acquire, iterations: Iterations) -> (Vec<u32>, usize) {
    let mut program = Program::default();
    let shared = program.label();
    program.load(11, SHARED_VIRTUAL);
    program.load(12, CORE1_RELEASE_WORD);
    program.emit(0xe3a0_3401); // mov r3, #0x01000000
    program.emit(0xe58c_3000); // str r3, [r12]
    program.emit(0xe28b_a000 | DONE[0]); // add r10, r11, #DONE0
    program.branch(Condition::Always, shared);
    let core1 = program.words.len();
    program.load(11, SHARED_VIRTUAL);
    program.emit(0xe28b_a000 | DONE[1]); // add r10, r11, #DONE1
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
    while DONE.iter().any(|&done| word(&machine, done) == 0) {
        assert!(machine.instructions() < BUDGET, "{acquire:?} at {quantum:?} did not finish: {:?}", machine.session.board.instruction_counts());
        machine.run(WINDOW);
        machine.require_running().unwrap();
    }
    let counts = machine.session.board.instruction_counts();
    assert!(counts.iter().all(|&count| count > 0), "both cores execute: {counts:?}");
    let (counter, violations, holders, lock) =
        (word(&machine, COUNTER), word(&machine, VIOLATIONS), word(&machine, HOLDERS), word(&machine, LOCK));
    if counter == 2 * iterations.0 && violations == 0 && holders == 0 && lock == 0 {
        Outcome::MutuallyExclusive {
            reservation_losses: DONE.map(|done| word(&machine, done + LOSSES_FROM_DONE)),
        }
    } else {
        Outcome::Raced { counter, violations, holders, lock }
    }
}

fn quanta() -> [Quantum; 4] {
    let document = RunDocument::parse(RUN).unwrap();
    [Quantum(1), Quantum(3), Quantum(7), Quantum(document.execution.quantum)]
}

const ITERATIONS: Iterations = Iterations(2000);

#[test]
fn exclusive_spin_lock_excludes_the_other_core_at_every_quantum() {
    for quantum in quanta() {
        let outcome = probe(Acquire::Exclusive, ITERATIONS, quantum);
        eprintln!("exclusive {quantum:?}: {outcome:?}");
        let Outcome::MutuallyExclusive { reservation_losses } = outcome else {
            panic!("{quantum:?}: {outcome:?}");
        };
        // In lockstep both cores reach STREX holding a reservation; one must lose it.
        if quantum.0 == 1 {
            assert!(reservation_losses.iter().sum::<u32>() > 0, "{outcome:?}");
        }
    }
}

#[test]
fn plain_test_and_set_control_races_at_every_quantum() {
    for quantum in quanta() {
        let outcome = probe(Acquire::PlainControl, ITERATIONS, quantum);
        eprintln!("plain control {quantum:?}: {outcome:?}");
        assert!(matches!(outcome, Outcome::Raced { .. }), "{quantum:?}: {outcome:?}");
    }
}

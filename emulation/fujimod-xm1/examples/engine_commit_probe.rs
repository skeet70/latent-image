//! Throwaway probe of pinned FujiMod raw-core hook, stop and commit timing for
//! the native-case admission decision. Prints one JSON object per scenario.
//!
//! Pages: code 0x1000 rx, 0x4000 rw (0xaa), 0x5000 r (0xbb), 0x6000 no access
//! (0xcc written before protection), 0x8000 unmapped.

use fujimod_emulator_cpu::{Access, Cpu, ExecutionError, Exit, Fault, Hint, Hook, HookRange, Model, Register, Run, arm};
use fujimod_emulator_cpu_unicorn::Library;
use serde::Serialize;
use std::ffi::c_void;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "action", content = "address")]
enum OnAccess {
    Continue,
    Stop(u64),
    RestartAndStop(u64),
}

struct Scenario {
    name: &'static str,
    code: &'static [u32],
    registers: &'static [(u8, u32)],
    on_access: OnAccess,
    fault_handled: bool,
    stop_at_instruction: Option<usize>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum AccessKind {
    Read,
    Write,
    Fetch,
}

impl From<Access> for AccessKind {
    fn from(access: Access) -> Self {
        match access {
            Access::Read => Self::Read,
            Access::Write => Self::Write,
            Access::Fetch => Self::Fetch,
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum FaultKind {
    ReadUnmapped,
    WriteUnmapped,
    FetchUnmapped,
    ReadProtected,
    WriteProtected,
    FetchProtected,
}

impl From<Fault> for FaultKind {
    fn from(fault: Fault) -> Self {
        match fault {
            Fault::ReadUnmapped => Self::ReadUnmapped,
            Fault::WriteUnmapped => Self::WriteUnmapped,
            Fault::FetchUnmapped => Self::FetchUnmapped,
            Fault::ReadProtected => Self::ReadProtected,
            Fault::WriteProtected => Self::WriteProtected,
            Fault::FetchProtected => Self::FetchProtected,
        }
    }
}

#[derive(Serialize)]
struct AccessRecord {
    access: AccessKind,
    address: u64,
    size: i32,
    value: i64,
}

#[derive(Serialize)]
struct FaultRecord {
    fault: FaultKind,
    address: u64,
    size: i32,
    value: i64,
    handled: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum HintKind {
    WaitForInterrupt,
    WaitForEvent,
    SendEvent,
    SendEventLocal,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum ErrorKind {
    InvalidInstruction,
    Memory(FaultKind),
    Backend,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum RunResult {
    Stopped,
    Hint { hint: HintKind },
    Trap { syndrome: u64, virtual_address: u64, physical_address: u64 },
    Timer,
    Error { error: ErrorKind, message: String },
}

impl From<Result<Exit, ExecutionError>> for RunResult {
    fn from(result: Result<Exit, ExecutionError>) -> Self {
        match result {
            Ok(Exit::Stopped) => Self::Stopped,
            Ok(Exit::Hint(hint)) => Self::Hint {
                hint: match hint {
                    Hint::WaitForInterrupt => HintKind::WaitForInterrupt,
                    Hint::WaitForEvent => HintKind::WaitForEvent,
                    Hint::SendEvent => HintKind::SendEvent,
                    Hint::SendEventLocal => HintKind::SendEventLocal,
                },
            },
            Ok(Exit::Trap(trap)) => Self::Trap {
                syndrome: trap.syndrome,
                virtual_address: trap.virtual_address,
                physical_address: trap.physical_address,
            },
            Ok(Exit::Timer) => Self::Timer,
            Err(error) => {
                let kind = match &error {
                    ExecutionError::InvalidInstruction(_) => ErrorKind::InvalidInstruction,
                    ExecutionError::Memory { fault, .. } => ErrorKind::Memory((*fault).into()),
                    ExecutionError::Backend(_) => ErrorKind::Backend,
                };
                Self::Error { error: kind, message: error.to_string() }
            }
        }
    }
}

#[derive(Serialize)]
struct Value {
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl From<Result<u32, String>> for Value {
    fn from(result: Result<u32, String>) -> Self {
        match result {
            Ok(value) => Self { value: Some(value), error: None },
            Err(error) => Self { value: None, error: Some(error) },
        }
    }
}

#[derive(Serialize)]
struct MemoryWord {
    address: u64,
    #[serde(flatten)]
    word: Value,
}

#[derive(Serialize)]
struct Observed {
    result: RunResult,
    pc: Value,
    r0: Value,
    r1: Value,
    r2: Value,
    r3: Value,
    memory: Vec<MemoryWord>,
    instructions: Vec<u64>,
    accesses: Vec<AccessRecord>,
    faults: Vec<FaultRecord>,
    hook_errors: Vec<String>,
}

#[derive(Serialize)]
struct Line {
    scenario: &'static str,
    on_access: OnAccess,
    fault_handled: bool,
    stop_at_instruction: Option<usize>,
    #[serde(flatten)]
    outcome: Outcome,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    SetupError(String),
    Observed(Observed),
}

struct Probe<'a> {
    cpu: &'a dyn Cpu,
    on_access: OnAccess,
    fault_handled: bool,
    stop_at_instruction: Option<usize>,
    instructions: Vec<u64>,
    accesses: Vec<AccessRecord>,
    faults: Vec<FaultRecord>,
    errors: Vec<String>,
}

unsafe fn on_instruction(address: u64, _: u32, data: *mut c_void) {
    let probe = unsafe { &mut *data.cast::<Probe<'_>>() };
    probe.instructions.push(address);
    if probe.stop_at_instruction == Some(probe.instructions.len()) {
        if let Err(error) = probe.cpu.stop() {
            probe.errors.push(error);
        }
    }
}

unsafe fn on_access(access: Access, address: u64, size: i32, value: i64, data: *mut c_void) {
    let probe = unsafe { &mut *data.cast::<Probe<'_>>() };
    probe.accesses.push(AccessRecord { access: access.into(), address, size, value });
    let result = match probe.on_access {
        OnAccess::Stop(at) if at == address => probe.cpu.stop(),
        OnAccess::RestartAndStop(at) if at == address => {
            probe.cpu.request_restart().and_then(|()| probe.cpu.stop())
        }
        _ => Ok(()),
    };
    if let Err(error) = result {
        probe.errors.push(error);
    }
}

unsafe fn on_fault(fault: Fault, address: u64, size: i32, value: i64, data: *mut c_void) -> bool {
    let probe = unsafe { &mut *data.cast::<Probe<'_>>() };
    let handled = probe.fault_handled;
    probe.faults.push(FaultRecord { fault: fault.into(), address, size, value, handled });
    handled
}

fn read_word(cpu: &dyn Cpu, address: u64) -> Result<u32, String> {
    let mut bytes = [0u8; 4];
    cpu.read_memory(address, &mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_register(cpu: &dyn Cpu, register: Register) -> Result<u32, String> {
    let mut bytes = [0u8; 4];
    match cpu.read_register(register, &mut bytes)? {
        4 => Ok(u32::from_le_bytes(bytes)),
        size => Err(format!("register read returned {size} bytes")),
    }
}

fn setup(cpu: &dyn Cpu, scenario: &Scenario) -> Result<(), String> {
    cpu.map_zeroed(0x1000, 0x1000, 5)?;
    for (page, fill) in [(0x4000u64, 0xaau8), (0x5000, 0xbb), (0x6000, 0xcc)] {
        cpu.map_zeroed(page, 0x1000, 3)?;
        cpu.write_memory(page, &[fill; 0x1000])?;
    }
    cpu.protect(0x5000, 0x1000, 1).map_err(|error| format!("read-only protect: {error}"))?;
    cpu.protect(0x6000, 0x1000, 0).map_err(|error| format!("no-access protect: {error}"))?;
    let permissions: Vec<(u64, u32)> = cpu.memory_regions()?.iter().map(|region| (region.begin, region.permissions)).collect();
    for (page, expected) in [(0x1000u64, 5u32), (0x4000, 3), (0x5000, 1), (0x6000, 0)] {
        if !permissions.contains(&(page, expected)) {
            return Err(format!("page {page:#x} lacks permissions {expected}: {permissions:?}"));
        }
    }
    let code: Vec<u8> = scenario.code.iter().flat_map(|word| word.to_le_bytes()).collect();
    cpu.write_memory(0x1000, &code)?;
    cpu.write_register(arm::CPSR, &0x10u32.to_le_bytes())?;
    for &(index, value) in scenario.registers {
        cpu.write_register(arm::CORE[usize::from(index)], &value.to_le_bytes())?;
    }
    Ok(())
}

fn run(scenario: &Scenario) -> Outcome {
    let cpu = match Library::linked().and_then(|library| library.open(Model::CortexR5F, 4 * 1024 * 1024)) {
        Ok(cpu) => cpu,
        Err(error) => return Outcome::SetupError(format!("open: {error}")),
    };
    if let Err(error) = setup(&cpu, scenario) {
        return Outcome::SetupError(error);
    }
    let mut probe = Box::new(Probe {
        cpu: &cpu,
        on_access: scenario.on_access,
        fault_handled: scenario.fault_handled,
        stop_at_instruction: scenario.stop_at_instruction,
        instructions: Vec::new(),
        accesses: Vec::new(),
        faults: Vec::new(),
        errors: Vec::new(),
    });
    let data = (&mut *probe as *mut Probe<'_>).cast::<c_void>();
    // SAFETY: `probe` outlives the run; the hooks are removed before it drops.
    let hooks = match unsafe {
        cpu.install_hooks(&[
            Hook::Instruction { callback: on_instruction, data, range: HookRange::All },
            Hook::Memory { callback: on_access, data, read: true, write: true, range: HookRange::All },
            Hook::Fault { callback: on_fault, data },
        ])
    } {
        Ok(hooks) => hooks,
        Err(error) => return Outcome::SetupError(format!("hooks: {error}")),
    };
    let end = 0x1000 + 4 * scenario.code.len() as u64;
    let result = cpu.run(Run { begin: 0x1000, until: Some(end), watchdog_us: 1_000_000, instructions: Some(16) });
    if let Err(error) = cpu.remove_hooks(&hooks) {
        probe.errors.push(format!("hook removal: {error}"));
    }
    let register = |index: usize| Value::from(read_register(&cpu, arm::CORE[index]));
    let probe = *probe;
    Outcome::Observed(Observed {
        result: result.into(),
        pc: read_register(&cpu, arm::PC).into(),
        r0: register(0),
        r1: register(1),
        r2: register(2),
        r3: register(3),
        memory: [0x4ff8u64, 0x4ffc, 0x5000, 0x5004, 0x5ffc, 0x6000]
            .into_iter()
            .map(|address| MemoryWord { address, word: read_word(&cpu, address).into() })
            .collect(),
        instructions: probe.instructions,
        accesses: probe.accesses,
        faults: probe.faults,
        hook_errors: probe.errors,
    })
}

const STR_POST: u32 = 0xe481_0004; // str r0, [r1], #4
const LDR_POST: u32 = 0xe491_0004; // ldr r0, [r1], #4
const STR: u32 = 0xe581_0000; // str r0, [r1]
const MOV_R3_1: u32 = 0xe3a0_3001; // mov r3, #1
const ADD_R0_1: u32 = 0xe280_0001; // add r0, r0, #1
const STMIA_WB: u32 = 0xe8a2_0003; // stmia r2!, {r0, r1}
const STMDB_WB: u32 = 0xe922_0003; // stmdb r2!, {r0, r1}
const LDMIA_WB: u32 = 0xe8b2_0003; // ldmia r2!, {r0, r1}
const STRD: u32 = 0xe1c2_00f0; // strd r0, r1, [r2]
const LDRD: u32 = 0xe1c2_00d0; // ldrd r0, r1, [r2]
const SWP: u32 = 0xe102_0091; // swp r0, r1, [r2]
const STORED: (u8, u32) = (0, 0x1111_1111);
const SECOND: (u8, u32) = (1, 0x2222_2222);
const LOADED: [(u8, u32); 2] = [(0, 0x7777_7777), (1, 0x8888_8888)];

fn main() {
    let scenario = |name, code, registers, on_access, fault_handled| Scenario {
        name,
        code,
        registers,
        on_access,
        fault_handled,
        stop_at_instruction: None,
    };
    let scenarios = [
        scenario("str-post rw, memory-hook stop", &[STR_POST, MOV_R3_1], &[STORED, (1, 0x4ffc)], OnAccess::Stop(0x4ffc), false),
        scenario("str-post r page, fault refused", &[STR_POST, MOV_R3_1], &[STORED, (1, 0x5000)], OnAccess::Continue, false),
        scenario("str-post r page, fault handled", &[STR_POST, MOV_R3_1], &[STORED, (1, 0x5000)], OnAccess::Continue, true),
        scenario("str-post unmapped, fault refused", &[STR_POST, MOV_R3_1], &[STORED, (1, 0x8000)], OnAccess::Continue, false),
        scenario("ldr-post no-access, fault refused", &[LDR_POST, MOV_R3_1], &[LOADED[0], (1, 0x6000)], OnAccess::Continue, false),
        scenario("ldr-post rw, memory-hook stop", &[LDR_POST, MOV_R3_1], &[LOADED[0], (1, 0x4ffc)], OnAccess::Stop(0x4ffc), false),
        scenario("stmia! rw then r, fault refused", &[STMIA_WB, MOV_R3_1], &[STORED, SECOND, (2, 0x4ffc)], OnAccess::Continue, false),
        scenario("stmdb! rw then r, fault refused", &[STMDB_WB, MOV_R3_1], &[STORED, SECOND, (2, 0x5004)], OnAccess::Continue, false),
        scenario("stmia! rw, memory-hook stop on second word", &[STMIA_WB, MOV_R3_1], &[STORED, SECOND, (2, 0x4ff8)], OnAccess::Stop(0x4ffc), false),
        scenario("ldmia! r then no-access, fault refused", &[LDMIA_WB, MOV_R3_1], &[LOADED[0], LOADED[1], (2, 0x5ffc)], OnAccess::Continue, false),
        scenario("strd rw then r, fault refused", &[STRD, MOV_R3_1], &[STORED, SECOND, (2, 0x4ffc)], OnAccess::Continue, false),
        scenario("ldrd r then no-access, fault refused", &[LDRD, MOV_R3_1], &[LOADED[0], LOADED[1], (2, 0x5ffc)], OnAccess::Continue, false),
        scenario("swp r page, fault refused", &[SWP, MOV_R3_1], &[LOADED[0], SECOND, (2, 0x5000)], OnAccess::Continue, false),
        scenario("str mid-block, restart and stop", &[STR, MOV_R3_1], &[STORED, (1, 0x4ffc)], OnAccess::RestartAndStop(0x4ffc), false),
        scenario("str last-in-block, restart and stop", &[STR], &[STORED, (1, 0x4ffc)], OnAccess::RestartAndStop(0x4ffc), false),
        Scenario {
            stop_at_instruction: Some(2),
            ..scenario("instruction-hook stop at second entry", &[ADD_R0_1, ADD_R0_1, ADD_R0_1], &[(0, 0)], OnAccess::Continue, false)
        },
    ];
    for scenario in &scenarios {
        let line = Line {
            scenario: scenario.name,
            on_access: scenario.on_access,
            fault_handled: scenario.fault_handled,
            stop_at_instruction: scenario.stop_at_instruction,
            outcome: run(scenario),
        };
        match serde_json::to_string(&line) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("{}: encoding failed: {error}", scenario.name);
                std::process::exit(2);
            }
        }
    }
}

//! Throwaway probe of pinned FujiMod raw-core hook, stop and commit timing for
//! the native-case admission decision. Prints one JSON line per scenario.
//!
//! Pages: code 0x1000 rx, 0x4000 rw (0xaa), 0x5000 r (0xbb), 0x6000 no access
//! (0xcc written before protection), 0x8000 unmapped.

use fujimod_emulator_cpu::{Access, Cpu, Fault, Hook, HookRange, Model, Run, arm};
use fujimod_emulator_cpu_unicorn::Library;
use std::ffi::c_void;

#[derive(Clone, Copy, Debug, PartialEq)]
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

struct Probe<'a> {
    cpu: &'a dyn Cpu,
    on_access: OnAccess,
    fault_handled: bool,
    stop_at_instruction: Option<usize>,
    instructions: Vec<u64>,
    accesses: Vec<(Access, u64, i32, i64)>,
    faults: Vec<(Fault, u64, i32, i64)>,
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
    probe.accesses.push((access, address, size, value));
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
    probe.faults.push((fault, address, size, value));
    probe.fault_handled
}

fn word(cpu: &dyn Cpu, address: u64) -> String {
    let mut bytes = [0u8; 4];
    match cpu.read_memory(address, &mut bytes) {
        Ok(()) => format!("{:08x}", u32::from_le_bytes(bytes)),
        Err(error) => format!("unreadable: {error}"),
    }
}

fn register(cpu: &dyn Cpu, register: fujimod_emulator_cpu::Register) -> String {
    let mut bytes = [0u8; 4];
    match cpu.read_register(register, &mut bytes) {
        Ok(_) => format!("{:08x}", u32::from_le_bytes(bytes)),
        Err(error) => format!("unreadable: {error}"),
    }
}

fn run(scenario: &Scenario) -> String {
    let library = Library::linked().expect("pinned Unicorn");
    let cpu = library.open(Model::CortexR5F, 4 * 1024 * 1024).expect("Cortex-R5F core");
    cpu.map_zeroed(0x1000, 0x1000, 5).unwrap();
    for (page, fill) in [(0x4000u64, 0xaau8), (0x5000, 0xbb), (0x6000, 0xcc)] {
        cpu.map_zeroed(page, 0x1000, 3).unwrap();
        cpu.write_memory(page, &[fill; 0x1000]).unwrap();
    }
    cpu.protect(0x5000, 0x1000, 1).unwrap();
    let no_access = cpu.protect(0x6000, 0x1000, 0).map_err(|error| error.to_string());
    let code: Vec<u8> = scenario.code.iter().flat_map(|word| word.to_le_bytes()).collect();
    cpu.write_memory(0x1000, &code).unwrap();
    cpu.write_register(arm::CPSR, &0x10u32.to_le_bytes()).unwrap();
    for &(index, value) in scenario.registers {
        cpu.write_register(arm::CORE[usize::from(index)], &value.to_le_bytes()).unwrap();
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
    let hooks = unsafe {
        cpu.install_hooks(&[
            Hook::Instruction { callback: on_instruction, data, range: HookRange::All },
            Hook::Memory { callback: on_access, data, read: true, write: true, range: HookRange::All },
            Hook::Fault { callback: on_fault, data },
        ])
    }
    .unwrap();
    let end = 0x1000 + 4 * scenario.code.len() as u64;
    let result = cpu.run(Run { begin: 0x1000, until: Some(end), watchdog_us: 1_000_000, instructions: Some(16) });
    cpu.remove_hooks(&hooks).unwrap();
    let registers: Vec<String> = (0..4)
        .map(|index| format!("\"r{index}\":\"{}\"", register(&cpu, arm::CORE[index])))
        .collect();
    format!(
        "{{\"scenario\":\"{}\",\"no_access_protect\":{:?},\"result\":{:?},\"pc\":\"{}\",{},\
         \"m4ff8\":\"{}\",\"m4ffc\":\"{}\",\"m5000\":\"{}\",\"m5004\":\"{}\",\"m6000\":\"{}\",\
         \"instructions\":{:?},\"accesses\":{:?},\"faults\":{:?},\"hook_errors\":{:?}}}",
        scenario.name,
        no_access,
        result.map(|exit| format!("{exit:?}")).map_err(|error| format!("{}: {error}", error.kind())),
        register(&cpu, arm::PC),
        registers.join(","),
        word(&cpu, 0x4ff8),
        word(&cpu, 0x4ffc),
        word(&cpu, 0x5000),
        word(&cpu, 0x5004),
        word(&cpu, 0x6000),
        probe.instructions,
        probe.accesses,
        probe.faults,
        probe.errors,
    )
}

const STR_POST: u32 = 0xe481_0004; // str r0, [r1], #4
const LDR_POST: u32 = 0xe491_0004; // ldr r0, [r1], #4
const STR: u32 = 0xe581_0000; // str r0, [r1]
const MOV_R3_1: u32 = 0xe3a0_3001; // mov r3, #1
const ADD_R0_1: u32 = 0xe280_0001; // add r0, r0, #1
const STORED: (u8, u32) = (0, 0x1111_1111);
const SECOND: (u8, u32) = (1, 0x2222_2222);

fn main() {
    let scenarios = [
        Scenario { name: "str-post rw, memory-hook stop", code: &[STR_POST, MOV_R3_1], registers: &[STORED, (1, 0x4ffc)], on_access: OnAccess::Stop(0x4ffc), fault_handled: false, stop_at_instruction: None },
        Scenario { name: "str-post r page, fault refused", code: &[STR_POST, MOV_R3_1], registers: &[STORED, (1, 0x5000)], on_access: OnAccess::Continue, fault_handled: false, stop_at_instruction: None },
        Scenario { name: "str-post r page, fault handled", code: &[STR_POST, MOV_R3_1], registers: &[STORED, (1, 0x5000)], on_access: OnAccess::Continue, fault_handled: true, stop_at_instruction: None },
        Scenario { name: "str-post unmapped, fault refused", code: &[STR_POST, MOV_R3_1], registers: &[STORED, (1, 0x8000)], on_access: OnAccess::Continue, fault_handled: false, stop_at_instruction: None },
        Scenario { name: "ldr-post no-access, fault refused", code: &[LDR_POST, MOV_R3_1], registers: &[(0, 0x7777_7777), (1, 0x6000)], on_access: OnAccess::Continue, fault_handled: false, stop_at_instruction: None },
        Scenario { name: "ldr-post rw, memory-hook stop", code: &[LDR_POST, MOV_R3_1], registers: &[(0, 0x7777_7777), (1, 0x4ffc)], on_access: OnAccess::Stop(0x4ffc), fault_handled: false, stop_at_instruction: None },
        Scenario { name: "stmia! rw then r, fault refused", code: &[0xe8a2_0003, MOV_R3_1], registers: &[STORED, SECOND, (2, 0x4ffc)], on_access: OnAccess::Continue, fault_handled: false, stop_at_instruction: None },
        Scenario { name: "stmdb! rw then r, fault refused", code: &[0xe922_0003, MOV_R3_1], registers: &[STORED, SECOND, (2, 0x5004)], on_access: OnAccess::Continue, fault_handled: false, stop_at_instruction: None },
        Scenario { name: "stmia! rw, memory-hook stop on second word", code: &[0xe8a2_0003, MOV_R3_1], registers: &[STORED, SECOND, (2, 0x4ff8)], on_access: OnAccess::Stop(0x4ffc), fault_handled: false, stop_at_instruction: None },
        Scenario { name: "ldmia! r then no-access, fault refused", code: &[0xe8b2_0003, MOV_R3_1], registers: &[(0, 0x7777_7777), (1, 0x8888_8888), (2, 0x5ffc)], on_access: OnAccess::Continue, fault_handled: false, stop_at_instruction: None },
        Scenario { name: "strd rw then r, fault refused", code: &[0xe1c2_00f0, MOV_R3_1], registers: &[STORED, SECOND, (2, 0x4ffc)], on_access: OnAccess::Continue, fault_handled: false, stop_at_instruction: None },
        Scenario { name: "swp r page, fault refused", code: &[0xe102_0091, MOV_R3_1], registers: &[(0, 0x7777_7777), SECOND, (2, 0x5000)], on_access: OnAccess::Continue, fault_handled: false, stop_at_instruction: None },
        Scenario { name: "str mid-block, restart and stop", code: &[STR, MOV_R3_1], registers: &[STORED, (1, 0x4ffc)], on_access: OnAccess::RestartAndStop(0x4ffc), fault_handled: false, stop_at_instruction: None },
        Scenario { name: "str last-in-block, restart and stop", code: &[STR], registers: &[STORED, (1, 0x4ffc)], on_access: OnAccess::RestartAndStop(0x4ffc), fault_handled: false, stop_at_instruction: None },
        Scenario { name: "instruction-hook stop at second entry", code: &[ADD_R0_1, ADD_R0_1, ADD_R0_1], registers: &[(0, 0)], on_access: OnAccess::Continue, fault_handled: false, stop_at_instruction: Some(2) },
    ];
    for scenario in &scenarios {
        println!("{}", run(scenario));
    }
}

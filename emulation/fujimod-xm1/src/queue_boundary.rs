use fujimod_emulator_cpu::{Cpu, Register, arm};
use fujimod_xm1::{
    machine::Machine,
    run::{self, RunDocument, ValidatedUpdate},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    process::ExitCode,
    time::{Duration, Instant},
};

const CAPTURE_HASH: &str = "fa933fcf723ff46d435161cfecf48edc4a29cc4dc4c0ff5232fd0c70e7a6fee2";
const IMAGE_HASH: &str = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16";
const QUEUE: u32 = 0x4005a36c;
const POOL: u32 = 0x4005f7a0;
const ENGINE: u32 = 0x400545cc;
const FIXTURE: u32 = 0x01010000;
const BLOCK: u32 = FIXTURE + 0x200;
const EXIT: u32 = 0x01011000;
const STACK: u32 = 0x01002000;
const FUNCTIONS: &[(Address, Address)] = &[
    (Address(0x411cc), Address(0x412cc)),
    (Address(0x42574), Address(0x42588)),
    (Address(0x4259c), Address(0x425c8)),
    (Address(0x427b8), Address(0x427d4)),
    (Address(0x439d8), Address(0x43a3c)),
    (Address(0x48edc), Address(0x48f44)),
    (Address(0x48fa4), Address(0x48ff8)),
    (Address(0x49308), Address(0x49718)),
    (Address(0x4a2f0), Address(0x4a56c)),
    (Address(0x4b41c), Address(0x4b480)),
    (Address(0x4bab8), Address(0x4bd4c)),
    (Address(0x4cbd8), Address(0x4cc00)),
    (Address(0x4d978), Address(0x4da60)),
    (Address(0x4da6c), Address(0x4da8c)),
    (Address(0x41a34), Address(0x41a5c)),
    (Address(0x722054), Address(0x722178)),
    (Address(0x723398), Address(0x7233b4)),
    (Address(0x72503c), Address(0x725214)),
    (Address(0x726478), Address(0x726498)),
    (Address(0x41140), Address(0x4118c)),
    (Address(0x422d8), Address(0x423d4)),
    (Address(0x726a38), Address(0x726a80)),
    (Address(0x726b30), Address(0x726b88)),
    (Address(0x726c74), Address(0x726cf8)),
    (Address(0x72835c), Address(0x728374)),
    (Address(0x72845c), Address(0x7284ec)),
];

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
struct Address(u32);
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
struct Word(u32);
#[derive(Serialize)]
#[serde(transparent)]
struct Cpu0InstructionCount(u64);
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Context {
    UnmaskedSvcSelectorClear,
    MaskedSvcSelectorClear,
    MaskedSvcSelectorSet,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Occupancy {
    Full,
    OneFree,
}
#[derive(Clone, Copy)]
struct Domain {
    start: Address,
    end: Address,
    writable: bool,
}
const DOMAINS: &[Domain] = &[
    Domain {
        start: Address(0x40000),
        end: Address(0x50000),
        writable: false,
    },
    Domain {
        start: Address(0x722000),
        end: Address(0x729000),
        writable: false,
    },
    Domain {
        start: Address(0x7cad00),
        end: Address(0x7cb000),
        writable: false,
    },
    Domain {
        start: Address(0x4004d000),
        end: Address(0x4004f100),
        writable: false,
    },
    Domain {
        start: Address(0x4004f100),
        end: Address(0x40050000),
        writable: true,
    },
    Domain {
        start: Address(0x40054000),
        end: Address(0x40070000),
        writable: true,
    },
    Domain {
        start: Address(0x4089e000),
        end: Address(0x4089f000),
        writable: false,
    },
    Domain {
        start: Address(0x0096b000),
        end: Address(0x0096c000),
        writable: false,
    },
    Domain {
        start: Address(0x40a0f000),
        end: Address(0x40a10000),
        writable: true,
    },
    Domain {
        start: Address(0x40a5b000),
        end: Address(0x40a5c000),
        writable: true,
    },
    Domain {
        start: Address(0x01000000),
        end: Address(0x01002000),
        writable: true,
    },
    Domain {
        start: Address(FIXTURE),
        end: Address(FIXTURE + 0x1000),
        writable: true,
    },
];

fn register(cpu: &dyn Cpu, r: u8) -> Result<Word, String> {
    let mut bytes = [0; 4];
    cpu.read_register(Register::Arm32(arm::Register::R(r)), &mut bytes)?;
    Ok(Word(u32::from_le_bytes(bytes)))
}
fn word(cpu: &dyn Cpu, at: Address) -> Result<Word, String> {
    let mut bytes = [0; 4];
    cpu.read_memory(u64::from(at.0), &mut bytes)?;
    Ok(Word(u32::from_le_bytes(bytes)))
}
fn put(cpu: &dyn Cpu, at: Address, value: Word) -> Result<(), String> {
    cpu.write_memory(u64::from(at.0), &value.0.to_le_bytes())
}
fn executable(at: Address) -> bool {
    at.0.is_multiple_of(4)
        && (at.0 == EXIT
            || FUNCTIONS
                .iter()
                .any(|&(lo, hi)| lo.0 <= at.0 && at.0 < hi.0))
}
fn memory(at: Address, size: u32, write: bool) -> Result<(), String> {
    if at.0.checked_add(size).is_some_and(|end| {
        DOMAINS
            .iter()
            .any(|d| d.start.0 <= at.0 && end <= d.end.0 && (!write || d.writable))
    }) {
        Ok(())
    } else {
        Err(format!(
            "outside memory domain: {:#010x}+{size} write={write}",
            at.0
        ))
    }
}
fn condition(ins: u32, cpsr: u32) -> bool {
    let (n, z, c, v) = (
        cpsr >> 31 != 0,
        cpsr & (1 << 30) != 0,
        cpsr & (1 << 29) != 0,
        cpsr & (1 << 28) != 0,
    );
    match ins >> 28 {
        0 => z,
        1 => !z,
        2 => c,
        3 => !c,
        4 => n,
        5 => !n,
        6 => v,
        7 => !v,
        8 => c && !z,
        9 => !c || z,
        10 => n == v,
        11 => n != v,
        12 => !z && n == v,
        13 => z || n != v,
        14 => true,
        _ => false,
    }
}
fn target(at: Address) -> Result<(), String> {
    if executable(at) {
        Ok(())
    } else {
        Err(format!("outside executable domain: {:#010x}", at.0))
    }
}
fn preflight(cpu: &dyn Cpu, pc: Address, ins: Word, cpsr: Word) -> Result<(), String> {
    let (pc, ins, cpsr) = (pc.0, ins.0, cpsr.0);
    if ins >> 28 == 15 {
        return Err(format!("unadmitted unconditional instruction {ins:#010x}"));
    }
    if !condition(ins, cpsr) {
        return Ok(());
    }
    if let (Some(refusal), _) = fujimod_xm1::r4f::classify_a32(ins) {
        return Err(format!("R4F guard: {refusal}"));
    }
    if ins & 0x0ffffff0 == 0x012fff10 || ins & 0x0ffffff0 == 0x012fff30 {
        return target(Address(register(cpu, (ins & 15) as u8)?.0));
    }
    if ins & 0x0e000000 == 0x0a000000 {
        let displacement = (((ins & 0xffffff) << 8) as i32) >> 6;
        return target(Address(
            pc.wrapping_add(8).wrapping_add_signed(displacement),
        ));
    }
    let rn = ((ins >> 16) & 15) as u8;
    let rd = ((ins >> 12) & 15) as u8;
    let base = if rn == 15 {
        pc + 8
    } else {
        register(cpu, rn)?.0
    };
    if ins & 0x0ff00fff == 0x01900f9f || ins & 0x0ff00ff0 == 0x01800f90 {
        if !matches!((pc, ins), (0x439e8, 0xe1901f9f) | (0x439f0, 0x01801f92)) {
            return Err("exclusive outside pinned lock sites".into());
        }
        if !base.is_multiple_of(4) {
            return Err("unaligned exclusive".into());
        }
        return memory(Address(base), 4, true);
    }
    let top = (ins >> 25) & 7;
    if ins & 0x0fb00000 == 0x03000000 {
        return if rd != 15 {
            Ok(())
        } else {
            Err("MOVW/MOVT to PC".into())
        };
    }
    if ins & 0x0fe00070 == 0x07e00050 || ins & 0x0fe00070 == 0x07a00050 {
        let lsb = (ins >> 7) & 31;
        let width = ((ins >> 16) & 31) + 1;
        return if rd != 15 && ins & 15 != 15 && lsb + width <= 32 {
            Ok(())
        } else {
            Err("invalid bitfield operand".into())
        };
    }
    if matches!(ins & 0x0fff0ff0, 0x06ef0070 | 0x06ff0070 | 0x06bf0070) {
        return if rd != 15 && ins & 15 != 15 {
            Ok(())
        } else {
            Err("invalid extend operand".into())
        };
    }
    if top == 2 || top == 3 {
        let load = ins & (1 << 20) != 0;
        let writeback = ins & (1 << 24) == 0 || ins & (1 << 21) != 0;
        if (ins & (1 << 24) == 0 && ins & (1 << 21) != 0)
            || (rn == 15 && writeback)
            || (!load && rd == 15)
            || (load && writeback && rn == rd)
        {
            return Err("unadmitted single transfer operands".into());
        }
        let offset = if top == 2 {
            ins & 0xfff
        } else {
            if ins & 0x10 != 0 {
                return Err("register-specified memory shift".into());
            }
            if ins & 15 == 15 {
                return Err("PC memory offset".into());
            }
            let value = register(cpu, (ins & 15) as u8)?.0;
            let shift = (ins >> 7) & 31;
            match (ins >> 5) & 3 {
                0 => value.wrapping_shl(shift),
                1 => {
                    if shift == 0 {
                        0
                    } else {
                        value >> shift
                    }
                }
                2 => ((value as i32) >> if shift == 0 { 31 } else { shift }) as u32,
                _ => return Err("ROR memory offset".into()),
            }
        };
        let effective = if ins & (1 << 24) == 0 {
            base
        } else if ins & (1 << 23) != 0 {
            base.wrapping_add(offset)
        } else {
            base.wrapping_sub(offset)
        };
        let size = if ins & (1 << 22) != 0 { 1 } else { 4 };
        if size == 4 && !effective.is_multiple_of(4) {
            return Err("unaligned word".into());
        }
        memory(Address(effective), size, ins & (1 << 20) == 0)?;
        if rd == 15 {
            if size != 4 {
                return Err("byte load to PC".into());
            }
            target(Address(word(cpu, Address(effective))?.0))?;
        }
        return Ok(());
    }
    if top == 4 {
        let list = ins & 0xffff;
        if ins & (1 << 22) != 0
            || rn == 15
            || (ins & (1 << 21) != 0 && list & (1 << rn) != 0)
            || (ins & (1 << 20) == 0 && list & (1 << 15) != 0)
        {
            return Err("unadmitted multiple transfer operands".into());
        }
        let size = list.count_ones() * 4;
        let up = ins & (1 << 23) != 0;
        let pre = ins & (1 << 24) != 0;
        let at = if up {
            base.wrapping_add(if pre { 4 } else { 0 })
        } else {
            base.wrapping_sub(size)
                .wrapping_add(if pre { 0 } else { 4 })
        };
        if !at.is_multiple_of(4) || size == 0 {
            return Err("invalid multiple transfer".into());
        }
        memory(Address(at), size, ins & (1 << 20) == 0)?;
        if list & (1 << 15) != 0 && ins & (1 << 20) != 0 {
            target(Address(word(cpu, Address(at + size - 4))?.0))?;
        }
        return Ok(());
    }
    if ins & 0x0e000090 == 0x00000090 && ins & 0x60 != 0 {
        if ins & 0x60 != 0x20
            || rd == 15
            || (rn == 15 && (ins & (1 << 24) == 0 || ins & (1 << 21) != 0))
            || (ins & (1 << 22) == 0 && ins & 15 == 15)
        {
            return Err("unadmitted extra transfer".into());
        }
        let offset = if ins & (1 << 22) != 0 {
            ((ins >> 4) & 0xf0) | (ins & 15)
        } else {
            register(cpu, (ins & 15) as u8)?.0
        };
        let at = if ins & (1 << 24) == 0 {
            base
        } else if ins & (1 << 23) != 0 {
            base.wrapping_add(offset)
        } else {
            base.wrapping_sub(offset)
        };
        if !at.is_multiple_of(2) {
            return Err("unaligned halfword".into());
        }
        return memory(Address(at), 2, ins & (1 << 20) == 0);
    }
    if top >= 6 {
        return if matches!(
            (pc, ins),
            (0x427b8, 0xee121e10)
                | (0x43a00, 0xee073fba)
                | (0x43a20, 0xee073fba)
                | (0x43a2c, 0xee073f9a)
        ) {
            Ok(())
        } else {
            Err(format!("unknown peripheral/coprocessor {ins:#010x}"))
        };
    }
    if ins & 0x0fb0fff0 == 0x0120f000 {
        let control = matches!(
            (pc, ins),
            (0x41150, 0xe121f001)
                | (0x4117c, 0xe121f000)
                | (0x425a4, 0xe121f002)
                | (0x425c0, 0xe121f001)
        );
        let flags_control = ins == 0xe129f003
            && matches!(
                pc,
                0x49448
                    | 0x49668
                    | 0x4a3d4
                    | 0x4a3f8
                    | 0x4a524
                    | 0x4bbf0
                    | 0x4bc14
                    | 0x4bc78
                    | 0x4bd18
            );
        if !control && !flags_control {
            return Err("state write outside pinned CPSR sites".into());
        }
        let value = register(cpu, (ins & 15) as u8)?.0;
        let (written, allowed) = if flags_control {
            (0xff0000ff, 0xf8000080)
        } else {
            (0xff, 0x80)
        };
        return if (value ^ cpsr) & written & !allowed == 0 {
            Ok(())
        } else {
            Err("CPSR write changes mode, T, F, J, or reserved state".into())
        };
    }
    if ins & 0x0fff0fff == 0x010f0000 {
        return if rd != 15 {
            Ok(())
        } else {
            Err("MRS to PC".into())
        };
    }
    if ins & 0x0fe0f0f0 == 0x00000090 {
        return if rn != 15 && ins & 15 != 15 && (ins >> 8) & 15 != 15 {
            Ok(())
        } else {
            Err("multiply PC operand".into())
        };
    }
    if top <= 1 && (ins & (1 << 25) != 0 || ins & 0x10 == 0 || ins & 0x80 == 0) {
        if ins & (1 << 25) == 0 && ins & 0x10 != 0 && ((ins >> 8) & 15 == 15 || ins & 15 == 15) {
            return Err("PC register-shift operand".into());
        }
        let opcode = (ins >> 21) & 15;
        let comparison = (8..=11).contains(&opcode);
        if comparison {
            return if ins & (1 << 20) != 0 && rd == 0 {
                Ok(())
            } else {
                Err("unadmitted test/state form".into())
            };
        }
        return if rd != 15 {
            Ok(())
        } else {
            Err("data processing writes PC".into())
        };
    }
    Err(format!("unadmitted instruction {ins:#010x} at {pc:#010x}"))
}

#[derive(Clone, Serialize)]
struct Hit {
    pc: Address,
    r0: Word,
    r1: Word,
    cpsr: Word,
}
#[derive(Default)]
struct Observations {
    hits: Vec<Hit>,
}

fn pinned(path: &Path, expected: &str) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if format!("{:x}", Sha256::digest(&bytes)) != expected {
        return Err(format!("{}: source digest differs", path.display()));
    }
    Ok(bytes)
}
fn idle_peer(machine: &Machine) -> Result<(), String> {
    let cpu = machine.session.board.cpu(1)?;
    let mut bytes = [0; 4];
    cpu.read_register(Register::Arm32(arm::Register::Cpsr), &mut bytes)?;
    if register(cpu, 15)?.0 != 0x80000000
        || u32::from_le_bytes(bytes) & 0xff != 0xd3
        || word(cpu, Address(0x80000000))?.0 != 0xeafffffe
    {
        return Err("CPU1 left masked SVC idle fixture".into());
    }
    Ok(())
}
fn step(machine: &mut Machine) -> Result<(), String> {
    idle_peer(machine)?;
    let cpu = machine.session.board.cpu(0)?;
    let pc = register(cpu, 15)?.0;
    target(Address(pc))?;
    let mut bytes = [0; 4];
    cpu.read_register(Register::Arm32(arm::Register::Cpsr), &mut bytes)?;
    let cpsr = u32::from_le_bytes(bytes);
    if cpsr & 0x7f != 0x13 {
        return Err("left SVC ARM state".into());
    }
    preflight(cpu, Address(pc), word(cpu, Address(pc))?, Word(cpsr))?;
    let before = machine.instructions();
    machine.run(1);
    if let Some(error) = &machine.failure {
        return Err(error.clone());
    }
    if machine.instructions() != before + 1 {
        return Err("exact aggregate instruction step did not advance once".into());
    }
    idle_peer(machine)
}

fn invoke(
    machine: &mut Machine,
    observations: &mut Observations,
    entry: Address,
    r0: Word,
    r1: Word,
    cpsr: Word,
) -> Result<Word, String> {
    let cpu = machine.session.board.cpu(0)?;
    cpu.write_register(Register::Arm32(arm::Register::Cpsr), &cpsr.0.to_le_bytes())?;
    for (r, value) in [
        (0, r0.0),
        (1, r1.0),
        (2, 0),
        (3, 0),
        (13, STACK),
        (14, EXIT),
        (15, entry.0),
    ] {
        cpu.write_register(Register::Arm32(arm::Register::R(r)), &value.to_le_bytes())?;
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    let initial = machine.instructions();
    loop {
        if Instant::now() >= deadline || machine.instructions() - initial >= 100_000 {
            return Err("bounded call exhausted".into());
        }
        let cpu = machine.session.board.cpu(0)?;
        let pc = register(cpu, 15)?.0;
        if pc == EXIT {
            break;
        }
        target(Address(pc))?;
        let mut bytes = [0; 4];
        cpu.read_register(Register::Arm32(arm::Register::Cpsr), &mut bytes)?;
        let cpsr = u32::from_le_bytes(bytes);
        if cpsr & 0x3f != 0x13 {
            return Err("left SVC ARM state".into());
        }
        if matches!(
            pc,
            0x411cc
                | 0x412b4
                | 0x49308
                | 0x49528
                | 0x494fc
                | 0x4970c
                | 0x48edc
                | 0x4bab8
                | 0x4bd28
                | 0x439e8
                | 0x439f0
                | 0x7251f4
        ) {
            observations.hits.push(Hit {
                pc: Address(pc),
                r0: Word(register(cpu, 0)?.0),
                r1: Word(register(cpu, 1)?.0),
                cpsr: Word(cpsr),
            });
        }
        step(machine)?;
    }
    register(machine.session.board.cpu(0)?, 0)
}

#[derive(Serialize)]
struct Report {
    context: Context,
    occupancy: Occupancy,
    capture_sha256: &'static str,
    image_sha256: &'static str,
    cp14_identity_fixture: Word,
    caller_return: Word,
    status_cell: Word,
    drained: Vec<Address>,
    next_allocation_return: Word,
    next_allocation: Address,
    pool_free_head: Address,
    initial_cpsr: Word,
    initial_context_selector: Word,
    caller_exit_cpsr: Word,
    caller_exit_context_selector: Word,
    query_initial_cpsr: Word,
    query_context_selector: Word,
    drain_empty_return: Word,
    post_allocation_receive_return: Word,
    cpu0_instructions: Cpu0InstructionCount,
    hits: Vec<Hit>,
    assumptions: [&'static str; 5],
}
fn session() -> Result<(RunDocument, fujimod_emulator::session::Session), String> {
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let mut document = RunDocument::parse(include_bytes!("../data/xm1-150.json"))?;
    document.execution.quantum = 1;
    let mut rom = vec![0; document.boot_rom.bytes];
    for chunk in rom.chunks_exact_mut(4) {
        chunk.copy_from_slice(&0xeafffffeu32.to_le_bytes());
    }
    let update = ValidatedUpdate(vec![0xff; document.update_flash_offset + 2048]);
    let mut session = run::open(&document, &update, &rom)?;
    session
        .board
        .set_execution_policy(document.execution_policy.into())?;
    if session.board.execution_policy() != fujimod_emulator_cpu::ExecutionPolicy::Exact {
        return Err("bounded fixture requires Exact execution".into());
    }
    let peer = session.board.cpu(1)?;
    peer.write_register(Register::Arm32(arm::Register::Cpsr), &0xd3u32.to_le_bytes())?;
    peer.write_register(
        Register::Arm32(arm::Register::R(15)),
        &0x80000000u32.to_le_bytes(),
    )?;
    Ok((document, session))
}

fn experiment(
    capture: &[u8],
    image: &[u8],
    context: Context,
    occupancy: Occupancy,
) -> Result<Report, String> {
    let (document, session) = session()?;
    let cpu = session.board.cpu(0)?;
    for &(lo, hi) in &[
        (0x40000u32, 0x50000u32),
        (0x722000, 0x729000),
        (0x7cad00, 0x7cb000),
    ] {
        let bytes = if lo >= 0x600000 {
            &image[(lo - 0x61c400) as usize..(hi - 0x61c400) as usize]
        } else {
            &capture[lo as usize..hi as usize]
        };
        cpu.write_memory(u64::from(lo), bytes)?;
    }
    for &(lo, hi) in &[
        (0x4004d000u32, 0x40050000u32),
        (0x40054000, 0x40070000),
        (0x4089e000, 0x4089f000),
        (0x40a0f000, 0x40a10000),
        (0x40a5b000, 0x40a5c000),
    ] {
        cpu.write_memory(
            u64::from(lo),
            &capture[(lo & 0x3fffffff) as usize..(hi & 0x3fffffff) as usize],
        )?;
    }
    cpu.write_memory(0x0096b000, &capture[0x96b000..0x96c000])?;
    cpu.write_memory(u64::from(FIXTURE), &[0; 0x1000])?;
    cpu.write_memory(0x01000000, &[0; 0x2000])?;
    cpu.write_memory(u64::from(EXIT), &0xeafffffeu32.to_le_bytes())?;
    cpu.write_memory(0x4089eaa8, &[0; 0x134])?;
    let cpsr = match context {
        Context::UnmaskedSvcSelectorClear => 0x13,
        _ => 0x93,
    };
    let count = match occupancy {
        Occupancy::Full => 2,
        Occupancy::OneFree => 1,
    };
    for (at, value) in [
        (
            0x400545a4,
            if matches!(context, Context::MaskedSvcSelectorSet) {
                1
            } else {
                0
            },
        ),
        (0x0096bd20, 0x4004dae0),
        (0x4089ea94, 0x726478),
        (0x4089ea8c, 0x72835c),
        (0x40a5b5ec, 0),
        (0x4004f600, 0),
        (ENGINE + 0x14, 0x400545c4),
        (0x400545c4, 0),
        (0x4004f25c, 0),
        (0x4089ea9c, 0),
        (0x4089eaa0, 0),
        (0x40a5b664, 0),
        (QUEUE, QUEUE),
        (QUEUE + 4, QUEUE),
        (QUEUE + 8, FIXTURE + 0x40),
        (QUEUE + 12, QUEUE + 12),
        (QUEUE + 16, QUEUE + 12),
        (QUEUE + 20, count),
        (QUEUE + 24, 0),
        (QUEUE + 28, if count == 2 { 0 } else { 1 }),
        (FIXTURE + 0x44, 2),
        (FIXTURE + 0x48, FIXTURE + 0x80),
        (FIXTURE + 0x4c, 1),
        (FIXTURE + 0x80, FIXTURE + 0x100),
        (FIXTURE + 0x84, FIXTURE + 0x180),
        (POOL, POOL),
        (POOL + 4, POOL),
        (POOL + 8, FIXTURE + 0x60),
        (POOL + 12, BLOCK + 0x80),
        (POOL + 16, BLOCK),
        (FIXTURE + 0x64, 0x80),
        (FIXTURE + 0x68, BLOCK),
        (FIXTURE + 0x6c, BLOCK + 0x80),
        (FIXTURE + 0x70, 1),
        (BLOCK, 0),
        (FIXTURE, 0x00003c09),
        (FIXTURE + 4, 8),
        (FIXTURE + 8, FIXTURE + 0x20),
        (FIXTURE + 0x20, 0xdeadbeef),
        (FIXTURE + 0x24, 0x11223344),
        (FIXTURE + 0x104, 0x11111111),
        (FIXTURE + 0x184, 0x22222222),
    ] {
        put(cpu, Address(at), Word(value))?;
    }
    cpu.rprofile_identity(Some(0x4003))?;
    let mut machine = Machine::new(document, session)?;
    let mut observations = Observations::default();
    let result = (|| {
        let caller = invoke(
            &mut machine,
            &mut observations,
            Address(0x72503c),
            Word(28),
            Word(FIXTURE),
            Word(cpsr),
        )?
        .0;
        let cpu = machine.session.board.cpu(0)?;
        let mut exit_cpsr = [0; 4];
        cpu.read_register(Register::Arm32(arm::Register::Cpsr), &mut exit_cpsr)?;
        let exit_selector = word(cpu, Address(0x400545a4))?;
        let status = word(machine.session.board.cpu(0)?, Address(0x40a0f82c))?.0;
        let free = word(machine.session.board.cpu(0)?, Address(POOL + 16))?.0;
        put(machine.session.board.cpu(0)?, Address(0x400545a4), Word(0))?;
        let mut drained = Vec::with_capacity(2);
        let mut drain_empty_return = None;
        for _ in 0..=2 {
            let received = invoke(
                &mut machine,
                &mut observations,
                Address(0x4a2f0),
                Word(0x1001c),
                Word(FIXTURE + 0x300),
                Word(0x13),
            )?;
            match received.0 {
                0 => drained.push(Address(
                    word(machine.session.board.cpu(0)?, Address(FIXTURE + 0x300))?.0,
                )),
                0xffffffce => {
                    drain_empty_return = Some(received);
                    break;
                }
                _ => return Err(format!("nonblocking receive returned {:#010x}", received.0)),
            }
        }
        let drain_empty_return =
            drain_empty_return.ok_or("queue did not become empty within capacity")?;
        let allocated = invoke(
            &mut machine,
            &mut observations,
            Address(0x411cc),
            Word(0x1001c),
            Word(FIXTURE + 0x304),
            Word(0x13),
        )?
        .0;
        let next = word(machine.session.board.cpu(0)?, Address(FIXTURE + 0x304))?.0;
        let post_allocation_receive_return = invoke(
            &mut machine,
            &mut observations,
            Address(0x4a2f0),
            Word(0x1001c),
            Word(FIXTURE + 0x300),
            Word(0x13),
        )?;
        Ok(Report {
            context,
            occupancy,
            capture_sha256: CAPTURE_HASH,
            image_sha256: IMAGE_HASH,
            cp14_identity_fixture: Word(0x4003),
            caller_return: Word(caller),
            status_cell: Word(status),
            drained,
            next_allocation_return: Word(allocated),
            next_allocation: Address(next),
            pool_free_head: Address(free),
            initial_cpsr: Word(cpsr),
            initial_context_selector: Word(u32::from(matches!(
                context,
                Context::MaskedSvcSelectorSet
            ))),
            caller_exit_cpsr: Word(u32::from_le_bytes(exit_cpsr)),
            caller_exit_context_selector: exit_selector,
            query_initial_cpsr: Word(0x13),
            query_context_selector: word(machine.session.board.cpu(0)?, Address(0x400545a4))?,
            drain_empty_return,
            post_allocation_receive_return,
            cpu0_instructions: Cpu0InstructionCount(machine.session.board.instruction_counts()[0]),
            hits: observations.hits,
            assumptions: [
                "CP14 identity 0x4003 is a fixture input, not an observed X-M1 identity",
                "CPU1 stays at masked SVC B . ROM, checked at every aggregate step; no interrupt delivery or competing writer",
                "Synthetic capacity-two ring and one-block pool; no waiting receiver or pool waiter; selected locks start uncontended",
                "Original diagnostics execute with logging disabled and no matched errno table entry",
                "Finite emulation budgets do not bound stock spinlocks or establish scheduler/lifetime acceptance",
            ],
        })
    })();
    result
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SafetyCase {
    Store,
    Load,
    Halfword,
    Multiple,
    MovWide,
    SignedExtend,
    MovWidePcRefusal,
    SwapRefusal,
    OutsideWrite,
    ReadOnlyWrite,
    UnalignedWrite,
    ComputedTarget,
    DirectTarget,
    StateControl,
    FlagsControl,
    FlagsModeRefusal,
    ModeRefusal,
    ExclusiveControl,
    ExclusiveUnaligned,
    PeripheralRefusal,
    IdlePeerRefusal,
}
#[derive(Serialize, PartialEq, Eq)]
struct SafetyState {
    pc: Address,
    cpsr: Word,
    r0: Word,
    r1: Word,
    r2: Word,
    first: Word,
    second: Word,
}
#[derive(Serialize)]
struct SafetyObservation {
    case: SafetyCase,
    refused: bool,
    before: SafetyState,
    after: SafetyState,
}
fn safety_state(machine: &Machine) -> Result<SafetyState, String> {
    let cpu = machine.session.board.cpu(0)?;
    let mut bytes = [0; 4];
    cpu.read_register(Register::Arm32(arm::Register::Cpsr), &mut bytes)?;
    Ok(SafetyState {
        pc: Address(register(cpu, 15)?.0),
        cpsr: Word(u32::from_le_bytes(bytes)),
        r0: Word(register(cpu, 0)?.0),
        r1: Word(register(cpu, 1)?.0),
        r2: Word(register(cpu, 2)?.0),
        first: Word(word(cpu, Address(FIXTURE))?.0),
        second: Word(word(cpu, Address(FIXTURE + 4))?.0),
    })
}
fn cpu0_step(machine: &mut Machine) -> Result<(), String> {
    let before = machine.session.board.instruction_counts()[0];
    for _ in 0..4 {
        step(machine)?;
        if machine.session.board.instruction_counts()[0] == before + 1 {
            return Ok(());
        }
    }
    Err("CPU0 did not receive an exact step".into())
}
fn safety() -> Result<Vec<SafetyObservation>, String> {
    let (document, session) = session()?;
    let cpu = session.board.cpu(0)?;
    for (pc, ins) in [
        (0x72503c, 0xe5801000u32),
        (0x725040, 0xe5902000),
        (0x725044, 0xe1002091),
        (0x725048, 0xe12fff10),
        (0x72504c, 0xeaf00000),
        (0x725050, 0xe1c010b0),
        (0x725054, 0xe8800006),
        (0x725058, 0xee111f10),
        (0x425a4, 0xe121f002),
        (0x439e8, 0xe1901f9f),
        (0x439ec, 0xe3510000),
        (0x439f0, 0x01801f92),
        (0x72505c, 0xe30a2bcd),
        (0x725060, 0xe3412234),
        (0x725064, 0xe6bf2071),
        (0x725068, 0xe300f001),
        (0x4bbf0, 0xe129f003),
    ] {
        put(cpu, Address(pc), Word(ins))?;
    }
    let mut machine = Machine::new(document, session)?;
    let mut observations = Vec::new();
    for case in [
        SafetyCase::Store,
        SafetyCase::Load,
        SafetyCase::Halfword,
        SafetyCase::Multiple,
        SafetyCase::MovWide,
        SafetyCase::SignedExtend,
        SafetyCase::MovWidePcRefusal,
        SafetyCase::SwapRefusal,
        SafetyCase::OutsideWrite,
        SafetyCase::ReadOnlyWrite,
        SafetyCase::UnalignedWrite,
        SafetyCase::ComputedTarget,
        SafetyCase::DirectTarget,
        SafetyCase::StateControl,
        SafetyCase::FlagsControl,
        SafetyCase::FlagsModeRefusal,
        SafetyCase::ModeRefusal,
        SafetyCase::ExclusiveControl,
        SafetyCase::ExclusiveUnaligned,
        SafetyCase::PeripheralRefusal,
        SafetyCase::IdlePeerRefusal,
    ] {
        let (pc, address, value, third, refusal): (u32, u32, u32, u32, bool) = match case {
            SafetyCase::Store => (0x72503c, FIXTURE, 0x12345678, 0, false),
            SafetyCase::Load => (0x725040, FIXTURE, 0, 0, false),
            SafetyCase::Halfword => (0x725050, FIXTURE, 0x12345678, 0, false),
            SafetyCase::Multiple => (0x725054, FIXTURE, 0x12345678, 0xabcdef01, false),
            SafetyCase::MovWide => (0x72505c, FIXTURE, 0, 0, false),
            SafetyCase::SignedExtend => (0x725064, FIXTURE, 0x8001, 0, false),
            SafetyCase::MovWidePcRefusal => (0x725068, FIXTURE, 0, 0, true),
            SafetyCase::SwapRefusal => (0x725044, FIXTURE, 0x12345678, 0, true),
            SafetyCase::OutsideWrite => (0x72503c, 0x90000000, 0x12345678, 0, true),
            SafetyCase::ReadOnlyWrite => (0x72503c, 0x40000, 0x12345678, 0, true),
            SafetyCase::UnalignedWrite => (0x72503c, FIXTURE + 1, 0x12345678, 0, true),
            SafetyCase::ComputedTarget => (0x725048, 0x90000000, 0, 0, true),
            SafetyCase::DirectTarget => (0x72504c, FIXTURE, 0, 0, true),
            SafetyCase::StateControl => (0x425a4, FIXTURE, 0, 0x93, false),
            SafetyCase::FlagsControl => (0x4bbf0, FIXTURE, 0, 0, false),
            SafetyCase::FlagsModeRefusal => (0x4bbf0, FIXTURE, 0, 0, true),
            SafetyCase::ModeRefusal => (0x425a4, FIXTURE, 0, 0x12, true),
            SafetyCase::ExclusiveControl => (0x439e8, FIXTURE, 0, 1, false),
            SafetyCase::ExclusiveUnaligned => (0x439e8, FIXTURE + 1, 0, 1, true),
            SafetyCase::PeripheralRefusal => (0x725058, FIXTURE, 0, 0, true),
            SafetyCase::IdlePeerRefusal => (0x72503c, FIXTURE, 0x12345678, 0, true),
        };
        let cpu = machine.session.board.cpu(0)?;
        cpu.write_register(Register::Arm32(arm::Register::Cpsr), &0x13u32.to_le_bytes())?;
        for (r, value) in [(0, address), (1, value), (2, third), (15, pc)] {
            cpu.write_register(Register::Arm32(arm::Register::R(r)), &value.to_le_bytes())?;
        }
        let flags = if matches!(case, SafetyCase::FlagsModeRefusal) {
            0xa0000092u32
        } else {
            0xa0000093
        };
        cpu.write_register(Register::Arm32(arm::Register::R(3)), &flags.to_le_bytes())?;
        put(
            cpu,
            Address(FIXTURE),
            Word(if matches!(case, SafetyCase::ExclusiveControl) {
                0
            } else {
                0xaabbccdd
            }),
        )?;
        put(cpu, Address(FIXTURE + 4), Word(0x55555555))?;
        if matches!(case, SafetyCase::IdlePeerRefusal) {
            machine.session.board.cpu(1)?.write_register(
                Register::Arm32(arm::Register::R(15)),
                &0x90000000u32.to_le_bytes(),
            )?;
        }
        let before = safety_state(&machine)?;
        let count = machine.instructions();
        let result = cpu0_step(&mut machine);
        if matches!(case, SafetyCase::ExclusiveControl) && result.is_ok() {
            cpu0_step(&mut machine)?;
            cpu0_step(&mut machine)?;
        }
        if matches!(case, SafetyCase::MovWide) && result.is_ok() {
            cpu0_step(&mut machine)?;
        }
        let refused = result.is_err();
        let after = safety_state(&machine)?;
        if refused != refusal || (refusal && (before != after || machine.instructions() != count)) {
            return Err("safety refusal changed guest state or had the wrong result".into());
        }
        let control = match case {
            SafetyCase::Store => after.first == Word(0x12345678),
            SafetyCase::Load => after.r2 == Word(0xaabbccdd),
            SafetyCase::Halfword => after.first == Word(0xaabb5678),
            SafetyCase::Multiple => {
                after.first == Word(0x12345678) && after.second == Word(0xabcdef01)
            }
            SafetyCase::MovWide => after.r2 == Word(0x1234abcd),
            SafetyCase::SignedExtend => after.r2 == Word(0xffff8001),
            SafetyCase::FlagsControl => after.cpsr == Word(0xa0000093),
            SafetyCase::StateControl => after.cpsr == Word(0x93),
            SafetyCase::ExclusiveControl => after.first == Word(1) && after.r1 == Word(0),
            _ => true,
        };
        if !control {
            return Err("safety control produced the wrong public memory/register effect".into());
        }
        observations.push(SafetyObservation {
            case,
            refused,
            before,
            after,
        });
        if matches!(case, SafetyCase::IdlePeerRefusal) {
            machine.session.board.cpu(1)?.write_register(
                Register::Arm32(arm::Register::R(15)),
                &0x80000000u32.to_le_bytes(),
            )?;
        }
    }
    Ok(observations)
}

fn drive() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 2 && args[0] == "--safety" {
        let file = std::fs::File::create(&args[1]).map_err(|e| e.to_string())?;
        return serde_json::to_writer_pretty(file, &safety()?).map_err(|e| e.to_string());
    }
    if args.len() != 3 {
        return Err("usage: fujimod-xm1-queue-boundary CAPTURE IMAGE OUTPUT".into());
    }
    let capture = pinned(Path::new(&args[0]), CAPTURE_HASH)?;
    let image = pinned(Path::new(&args[1]), IMAGE_HASH)?;
    let reports = [
        (Context::UnmaskedSvcSelectorClear, Occupancy::Full),
        (Context::UnmaskedSvcSelectorClear, Occupancy::OneFree),
        (Context::MaskedSvcSelectorClear, Occupancy::Full),
        (Context::MaskedSvcSelectorSet, Occupancy::Full),
    ]
    .into_iter()
    .map(|(context, occupancy)| experiment(&capture, &image, context, occupancy))
    .collect::<Result<Vec<_>, _>>()?;
    let file = std::fs::File::create(&args[2]).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(file, &reports).map_err(|e| e.to_string())
}
fn main() -> ExitCode {
    match drive() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("queue-boundary: {error}");
            ExitCode::FAILURE
        }
    }
}

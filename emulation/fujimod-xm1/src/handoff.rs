//! The `ram-rescue-engine` raw handoff: CPU0's terminal state at native entry
//! `0x1800` with 32 MiB of DRAM and the 4 KiB SRAM, imported into an
//! unexecuted session as the retained `load_recovery_cpu0` does
//! (`tools/continue_rec_scanout.py`). CPU1 stays held at reset.

use crate::machine::Machine;
use crate::provenance::{Guest, plan_windows};
use fujimod_emulator_cpu::Register;
use fujimod_emulator_cpu::arm::{self, Coprocessor};
use fujimod_emulator_devices::Device;
use fujimod_emulator_devices::exr2::{clock::Clocks, gpio::Gpio};
use fujimod_emulator_exr2_system::boot::Block98000;
use serde::Deserialize;
use std::path::Path;

const RAM_BYTES: usize = 0x200_0000;
const SRAM: u64 = 0xfff0_0000;
const SRAM_BYTES: usize = 4096;
const NATIVE_ENTRY: u32 = 0x1800;
const ENTRY_STACK: u32 = 0xfff0_0f00;
const SUPERVISOR: u32 = 0x13;
const MODES: [u32; 6] = [0x10, 0x11, 0x12, 0x13, 0x17, 0x1b];
const CP15: [(u8, u8, u8); 6] = [(1, 0, 0), (1, 0, 1), (1, 0, 2), (3, 0, 0), (9, 1, 0), (9, 1, 1)];
/// ACTLR, compared with the cold core.
const RESET_ONLY: (u8, u8, u8) = (1, 0, 1);
const CLOCK_REQUEST: u32 = 0xfff8_8084;
const MEMORY_SELECTION: u32 = 0xfff9_8008;
const RAM_SELECTION: u64 = 1;
const GPIO_WRITES: [u32; 2] = [0xfff8_93c4, 0xfff8_93c0];

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Route {
    Fat,
    Raw,
}

impl Route {
    /// The ROM return address the bootstrap saved at SRAM `+0xefc`.
    fn witness(self) -> u32 {
        match self {
            Route::Fat => 0x8000_015c,
            Route::Raw => 0x8000_0174,
        }
    }
}

#[derive(Deserialize)]
enum Kind {
    #[serde(rename = "ram-rescue-terminal-cpu0")]
    TerminalCpu0,
}

#[derive(Deserialize)]
enum Status {
    #[serde(rename = "handoff")]
    Handoff,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Banked {
    mode: u32,
    sp: u32,
    lr: u32,
    spsr: u32,
    r8_r12: [u32; 5],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Vfp {
    fpexc: u32,
    fpscr: u32,
    d0_d15: [u64; 16],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct State {
    schema: u32,
    kind: Kind,
    status: Status,
    route: Route,
    ram_bytes: usize,
    ram_crc32: u32,
    registers: [u32; 17],
    banked: Vec<Banked>,
    vfp: Vfp,
    cp15: Vec<(u8, u8, u8, u32)>,
    mpu: [[u32; 3]; 8],
    mpu_selector: u32,
    sram_hex: String,
    device_writes: Vec<(u32, u32)>,
    provenance: serde_json::Value,
}

/// A checked handoff: its RAM and SRAM bytes and CPU0 frame.
pub struct Handoff {
    state: State,
    ram: Vec<u8>,
    sram: Vec<u8>,
}

fn hex(text: &str) -> Result<Vec<u8>, String> {
    if text.len() % 2 != 0 {
        return Err("handoff SRAM hex has odd length".into());
    }
    (0..text.len() / 2)
        .map(|index| u8::from_str_radix(&text[2 * index..2 * index + 2], 16).map_err(|_| "handoff SRAM is not hex".into()))
        .collect()
}

fn coprocessor(crn: u8, crm: u8, opc2: u8) -> Coprocessor {
    Coprocessor { cp: 15, opc1: 0, crn: crn.into(), crm: crm.into(), opc2: opc2.into(), secure: 0, wide: false }
}

impl Handoff {
    /// Reads `ram` and its `.state.json` companion and refuses anything but the
    /// A32 supervisor native-entry frame with R4F-compatible MPU state and the cache off.
    pub fn load(ram: &Path) -> Result<Self, String> {
        let companion = format!("{}.state.json", ram.display());
        let text = std::fs::read(&companion).map_err(|e| format!("{companion}: {e}"))?;
        let state: State = serde_json::from_slice(&text).map_err(|e| format!("{companion}: {e}"))?;
        let bytes = std::fs::read(ram).map_err(|e| format!("{}: {e}", ram.display()))?;
        if state.schema != 1 || state.ram_bytes != RAM_BYTES || bytes.len() != RAM_BYTES {
            return Err("handoff RAM extent differs from the 32 MiB export".into());
        }
        let mut crc = flate2::Crc::new();
        crc.update(&bytes);
        if crc.sum() != state.ram_crc32 {
            return Err("handoff RAM checksum differs from its companion".into());
        }
        let registers = &state.registers;
        if registers[15] != NATIVE_ENTRY || registers[13] != ENTRY_STACK || registers[16] & 0x3f != SUPERVISOR {
            return Err("handoff CPU0 must be the A32 supervisor native-entry frame".into());
        }
        let sram = hex(&state.sram_hex)?;
        if sram.len() != SRAM_BYTES {
            return Err("handoff SRAM must be 4 KiB".into());
        }
        let witness = u32::from_le_bytes([sram[0xefc], sram[0xefd], sram[0xefe], sram[0xeff]]);
        if witness != state.route.witness() || registers[14] != witness {
            return Err("handoff SRAM and saved ROM return witness disagree".into());
        }
        if state.banked.len() != MODES.len() || state.banked.iter().zip(MODES).any(|(row, mode)| row.mode != mode) {
            return Err("handoff banked register frame is incomplete".into());
        }
        if state.cp15.len() != CP15.len()
            || state.cp15.iter().zip(CP15).any(|(row, key)| (row.0, row.1, row.2) != key)
            || state.cp15[0].3 & 4 != 0
            || state.mpu_selector >= 8
        {
            return Err("handoff needs complete R4F-compatible MPU/control state with the cache disabled".into());
        }
        if state.device_writes.iter().any(|(address, _)| *address != CLOCK_REQUEST && !GPIO_WRITES.contains(address)) {
            return Err("handoff contains an unsupported bootstrap device transaction".into());
        }
        Ok(Self { state, ram: bytes, sram })
    }

    pub fn route(&self) -> Route {
        self.state.route
    }

    /// Imports the handoff into `machine`, whose session must be unexecuted.
    /// Selection word 1 at `0xfff98008` decodes DRAM at address 0, as the
    /// retained `remap_low_ram`.
    pub fn apply(&self, machine: &mut Machine) -> Result<(), String> {
        let windows = plan_windows(machine.document.devices.get())?;
        let node_at = |address: u32| {
            windows
                .iter()
                .find(|(_, window)| address.wrapping_sub(window.base.0) < window.size)
                .map(|(node, _)| node.clone())
                .ok_or_else(|| format!("no plan window holds {}", Guest(address)))
        };
        let session = &mut machine.session;
        if session.board.instruction_counts().iter().any(|count| *count != 0) {
            return Err("the handoff requires an unexecuted board".into());
        }
        let cpu = session.board.cpu(0)?;
        let (crn, crm, opc2) = RESET_ONLY;
        let exported = self.state.cp15.iter().find(|row| (row.0, row.1, row.2) == RESET_ONLY).map(|row| row.3);
        if exported != Some(cpu.coprocessor(coprocessor(crn, crm, opc2), None)? as u32) {
            return Err("handoff exported a reset-only CP15 field differing from the cold core".into());
        }
        let selection = session.node(&node_at(MEMORY_SELECTION)?)?;
        session.board.device_input::<Block98000, _>(selection, |device, cx| {
            device.write(cx, MEMORY_SELECTION.into(), 4, RAM_SELECTION);
            Ok(())
        })?;
        session.board.write_physical(0, &self.ram)?;
        session.board.write_physical(SRAM, &self.sram)?;
        let mut low = [0; 16];
        session.board.read_memory(0, 0, &mut low)?;
        if low[..] != self.ram[..16] {
            return Err("address 0 does not decode the handoff DRAM after the memory selection".into());
        }
        let write = |register: arm::Register, value: u64| session.board.write_register(0, Register::Arm32(register), value);
        let cpsr = u64::from(self.state.registers[16]);
        for row in &self.state.banked {
            write(arm::Register::Cpsr, (cpsr & !0x1f) | u64::from(row.mode))?;
            write(arm::Register::R(13), row.sp.into())?;
            write(arm::Register::R(14), row.lr.into())?;
            if row.mode != 0x10 {
                write(arm::Register::Spsr, row.spsr.into())?;
            }
            for (number, value) in (8..13).zip(row.r8_r12) {
                write(arm::Register::R(number), value.into())?;
            }
        }
        write(arm::Register::Cpsr, cpsr)?;
        for (number, value) in self.state.registers.iter().take(16).enumerate() {
            write(arm::Register::R(number as u8), (*value).into())?;
        }
        let cpu = session.board.cpu(0)?;
        for (index, row) in self.state.mpu.iter().enumerate() {
            cpu.coprocessor(coprocessor(6, 2, 0), Some(index as u64))?;
            for (opc2, value) in [0, 2, 4].into_iter().zip(row) {
                cpu.coprocessor(coprocessor(6, 1, opc2), Some((*value).into()))?;
            }
        }
        cpu.coprocessor(coprocessor(6, 2, 0), Some(self.state.mpu_selector.into()))?;
        for (crn, crm, opc2, value) in &self.state.cp15 {
            if (*crn, *crm, *opc2) != RESET_ONLY {
                cpu.coprocessor(coprocessor(*crn, *crm, *opc2), Some((*value).into()))?;
            }
        }
        write(arm::Register::Fpexc, self.state.vfp.fpexc.into())?;
        write(arm::Register::Fpscr, self.state.vfp.fpscr.into())?;
        for (number, value) in self.state.vfp.d0_d15.iter().enumerate() {
            write(arm::Register::D(number as u8), *value)?;
        }
        for (address, value) in &self.state.device_writes {
            let node = session.node(&node_at(*address)?)?;
            let (address, value) = (u64::from(*address), u64::from(*value));
            let store = |device: &mut dyn Device, cx: &mut fujimod_emulator_devices::Cx<'_>| -> Result<(), String> {
                device.write(cx, address, 4, value);
                Ok(())
            };
            if address == u64::from(CLOCK_REQUEST) {
                session.board.device_input::<Clocks, _>(node, |device, cx| store(device, cx))?;
            } else {
                session.board.device_input::<Gpio, _>(node, |device, cx| store(device, cx))?;
            }
        }
        Ok(())
    }
}

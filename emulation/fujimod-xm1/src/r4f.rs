//! Refusal of behaviour a Cortex-R4F lacks or reports differently from the
//! Cortex-R5F that FujiMod opens (docs/principles.md, scope and evidence).
//!
//! A block hook on every core records each newly entered block; after each
//! run window the guard decodes those blocks and refuses A32 SDIV/UDIV,
//! PLDW, reads of the CP15 c0 ID registers, CP15 c15 other than the stock
//! data-cache invalidate `c15,c5,0`, and c9 TCM selectors other than the
//! `c9,c1,{0,1}` region registers. After any block that writes CP15 c6, it
//! refuses an enabled MPU region 12-15. A block rewritten in place with the
//! same address and length is decoded once.

use fujimod_emulator::board::Board;
use fujimod_emulator_cpu::arm::Coprocessor;
use fujimod_emulator_cpu::{Block, Hook, HookId, HookRange};
use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::c_void;
use std::fmt;

/// Cortex-R4 implements at most 12 MPU regions.
const R4_MPU_REGIONS: u32 = 12;
const R5_MPU_REGIONS: u32 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    A32Divide,
    PreloadForWrite,
    IdRegisterRead { crm: u32, opc2: u32 },
    ImplementationRegister { opc1: u32, crm: u32, opc2: u32 },
    TcmSelector { crm: u32, opc2: u32 },
    MpuRegion(u32),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::A32Divide => write!(f, "A32 SDIV/UDIV"),
            Refusal::PreloadForWrite => write!(f, "PLDW (multiprocessing extensions)"),
            Refusal::IdRegisterRead { crm, opc2 } => write!(f, "CP15 ID register read c0,c{crm},{opc2}"),
            Refusal::ImplementationRegister { opc1, crm, opc2 } => write!(f, "CP15 c15 access {opc1},c{crm},{opc2}"),
            Refusal::TcmSelector { crm, opc2 } => write!(f, "CP15 TCM register c9,c{crm},{opc2}"),
            Refusal::MpuRegion(region) => write!(f, "enabled MPU region {region}"),
        }
    }
}

/// What one coprocessor transfer touches, from its A32 bits [27:0] layout.
fn coprocessor(word: u32) -> (Option<Refusal>, bool) {
    if word & 0x0f00_0010 != 0x0e00_0010 || (word >> 8) & 15 != 15 {
        return (None, false);
    }
    let (opc1, read, crn, crm, opc2) = ((word >> 21) & 7, word & (1 << 20) != 0, (word >> 16) & 15, word & 15, (word >> 5) & 7);
    let refusal = match crn {
        0 if opc1 == 0 && read => Some(Refusal::IdRegisterRead { crm, opc2 }),
        15 if !(opc1 == 0 && crm == 5 && opc2 == 0 && !read) => Some(Refusal::ImplementationRegister { opc1, crm, opc2 }),
        9 if opc1 == 0 && (crm == 2 || crm == 1 && opc2 > 1) => Some(Refusal::TcmSelector { crm, opc2 }),
        _ => None,
    };
    (refusal, crn == 6 && !read)
}

/// Decodes one A32 word: a refusal, and whether it writes an MPU register.
pub fn classify_a32(word: u32) -> (Option<Refusal>, bool) {
    if word >> 28 != 15 && word & 0x0fd0_f0f0 == 0x0710_f010 {
        return (Some(Refusal::A32Divide), false);
    }
    if word & 0xff70_f000 == 0xf510_f000 || word & 0xff70_f010 == 0xf710_f000 {
        return (Some(Refusal::PreloadForWrite), false);
    }
    coprocessor(word)
}

/// Decodes one 32-bit T32 instruction, first halfword in the high bits.
/// T32 SDIV/UDIV exist on Cortex-R4 and are admitted.
pub fn classify_t32(word: u32) -> (Option<Refusal>, bool) {
    if word & 0xef00_0010 == 0xee00_0010 {
        return coprocessor(word);
    }
    (None, false)
}

#[derive(Default)]
struct Entered {
    seen: HashSet<(u64, u32, Option<bool>)>,
    pending: Vec<Block>,
}

struct CoreHook {
    core: usize,
    entered: Box<RefCell<Entered>>,
    hook: Option<HookId>,
}

unsafe fn entered(block: Block, data: *mut c_void) {
    let entered = unsafe { &*data.cast::<RefCell<Entered>>() };
    let mut entered = entered.borrow_mut();
    if entered.seen.insert((block.address, block.bytes, block.thumb)) {
        entered.pending.push(block);
    }
}

#[derive(Default)]
pub struct Guard {
    cores: Vec<CoreHook>,
    mpu_written: bool,
}

impl Guard {
    /// Installs the block hooks; `seen` blocks survive a disarm.
    pub fn arm(&mut self, board: &Board) -> Result<(), String> {
        if self.cores.is_empty() {
            self.cores = (0..board.instruction_counts().len())
                .map(|core| CoreHook { core, entered: Box::default(), hook: None })
                .collect();
        }
        for core in &mut self.cores {
            if core.hook.is_some() {
                continue;
            }
            let data = (&*core.entered as *const RefCell<Entered>).cast_mut().cast();
            let hook = unsafe { board.cpu(core.core)?.install_hook(Hook::Block { callback: entered, data, range: HookRange::All }) }?;
            core.hook = Some(hook);
        }
        Ok(())
    }

    pub fn disarm(&mut self, board: &Board) -> Result<(), String> {
        for core in &mut self.cores {
            if let Some(hook) = core.hook.take() {
                board.cpu(core.core)?.remove_hook(hook)?;
            }
        }
        Ok(())
    }

    /// Decodes the blocks entered since the last scan.
    pub fn scan(&mut self, board: &Board) -> Result<(), String> {
        for core in &self.cores {
            let pending = std::mem::take(&mut core.entered.borrow_mut().pending);
            for block in pending {
                let mut bytes = vec![0; block.bytes as usize];
                board.read_memory(core.core, block.address, &mut bytes)?;
                let refuse = |refusal: Refusal, at: u64, word: u32| {
                    Err(format!("R4F guard: {refusal} at {at:#010x} (word {word:#010x}, core {})", core.core))
                };
                if block.thumb == Some(true) {
                    let halves: Vec<u16> = bytes.chunks_exact(2).map(|h| u16::from_le_bytes([h[0], h[1]])).collect();
                    let mut index = 0;
                    while index < halves.len() {
                        let first = halves[index];
                        if first >> 11 >= 0b11101 && index + 1 < halves.len() {
                            let word = u32::from(first) << 16 | u32::from(halves[index + 1]);
                            let (refusal, mpu) = classify_t32(word);
                            if let Some(refusal) = refusal {
                                return refuse(refusal, block.address + 2 * index as u64, word);
                            }
                            self.mpu_written |= mpu;
                            index += 2;
                        } else {
                            index += 1;
                        }
                    }
                } else {
                    for (index, chunk) in bytes.chunks_exact(4).enumerate() {
                        let word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                        let (refusal, mpu) = classify_a32(word);
                        if let Some(refusal) = refusal {
                            return refuse(refusal, block.address + 4 * index as u64, word);
                        }
                        self.mpu_written |= mpu;
                    }
                }
            }
        }
        if std::mem::take(&mut self.mpu_written) {
            self.check_regions(board)?;
        }
        Ok(())
    }

    /// Refuses an enabled region above the Cortex-R4 maximum on any core.
    fn check_regions(&self, board: &Board) -> Result<(), String> {
        let register = |crm, opc2| Coprocessor { cp: 15, opc1: 0, crn: 6, crm, opc2, secure: 0, wide: false };
        let (number, size) = (register(2, 0), register(1, 2));
        for core in &self.cores {
            let cpu = board.cpu(core.core)?;
            let saved = cpu.coprocessor(number, None)?;
            let mut enabled = None;
            for region in R4_MPU_REGIONS..R5_MPU_REGIONS {
                cpu.coprocessor(number, Some(u64::from(region)))?;
                if cpu.coprocessor(size, None)? & 1 != 0 {
                    enabled = Some(region);
                    break;
                }
            }
            cpu.coprocessor(number, Some(saved))?;
            if let Some(region) = enabled {
                return Err(format!("R4F guard: {} on core {}", Refusal::MpuRegion(region), core.core));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(word: u32) -> Option<Refusal> {
        classify_a32(word).0
    }

    #[test]
    fn a32_divide_and_preload_for_write_are_refused() {
        assert_eq!(refused(0xe710_f312), Some(Refusal::A32Divide)); // sdiv r0, r2, r3
        assert_eq!(refused(0x1730_f112), Some(Refusal::A32Divide)); // udivne r0, r2, r1
        assert_eq!(refused(0xf590_f000), Some(Refusal::PreloadForWrite)); // pldw [r0]
        assert_eq!(refused(0xf5d0_f000), None); // pld [r0]
        assert_eq!(refused(0xe000_0291), None); // mul r0, r1, r2
    }

    #[test]
    fn id_register_reads_are_refused_and_cache_identification_is_admitted() {
        assert_eq!(refused(0xee10_0fb0), Some(Refusal::IdRegisterRead { crm: 0, opc2: 5 })); // mrc p15,0,r0,c0,c0,5 (MPIDR)
        assert_eq!(refused(0xee10_0f10), Some(Refusal::IdRegisterRead { crm: 0, opc2: 0 })); // mrc p15,0,r0,c0,c0,0 (MIDR)
        assert_eq!(refused(0xee30_0f30), None); // mrc p15,1,r0,c0,c0,1 (CLIDR)
        assert_eq!(refused(0xee40_0f10), None); // mcr p15,2,r0,c0,c0,0 (CSSELR)
    }

    #[test]
    fn only_the_stock_implementation_and_tcm_registers_are_admitted() {
        assert_eq!(refused(0xee0f_0f15), None); // mcr p15,0,r0,c15,c5,0
        assert_eq!(refused(0xee1f_0f10), Some(Refusal::ImplementationRegister { opc1: 0, crm: 0, opc2: 0 }));
        assert_eq!(refused(0xee09_0f31), None); // mcr p15,0,r0,c9,c1,1 (BTCM region)
        assert_eq!(refused(0xee09_0f12), Some(Refusal::TcmSelector { crm: 2, opc2: 0 }));
        assert_eq!(classify_a32(0xee06_0f12), (None, true)); // mcr p15,0,r0,c6,c2,0 (RGNR)
    }

    #[test]
    fn t32_coprocessor_reads_are_classified_and_t32_divide_is_admitted() {
        assert_eq!(classify_t32(0xee10_0fb0).0, Some(Refusal::IdRegisterRead { crm: 0, opc2: 5 }));
        assert_eq!(classify_t32(0xfb90_f0f1).0, None); // sdiv r0, r0, r1
    }
}

//! Stock `0x42560..0x425c4`: save and restore of the CPSR I and F bits.

use super::SavedInterrupts;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MaskBits {
    IrqFiq,
    Irq,
    Fiq,
}

impl MaskBits {
    pub const fn bits(self) -> u32 {
        match self {
            MaskBits::IrqFiq => 0xc0,
            MaskBits::Irq => 0x80,
            MaskBits::Fiq => 0x40,
        }
    }
}

/// `mrs`, `cpsid` and `cpsie`.
pub trait Cpsr {
    fn read_cpsr(&self) -> u32;
    fn mask(&self, bits: MaskBits);
    fn unmask(&self, bits: MaskBits);
}

#[inline(always)]
pub fn save<C: Cpsr>(c: &C, bits: MaskBits) -> SavedInterrupts {
    let cpsr = c.read_cpsr();
    c.mask(bits);
    SavedInterrupts(cpsr & bits.bits())
}

/// Sets each of `bits` to its value in `saved`, masking before unmasking. Returns
/// `bits`, stock's r0.
#[inline(always)]
pub fn restore<C: Cpsr>(c: &C, bits: MaskBits, saved: SavedInterrupts) -> u32 {
    let irq = saved.0 & 0x80 != 0;
    let fiq = saved.0 & 0x40 != 0;
    match bits {
        MaskBits::Irq if irq => c.mask(MaskBits::Irq),
        MaskBits::Irq => c.unmask(MaskBits::Irq),
        MaskBits::Fiq if fiq => c.mask(MaskBits::Fiq),
        MaskBits::Fiq => c.unmask(MaskBits::Fiq),
        MaskBits::IrqFiq => {
            if irq && fiq {
                c.mask(MaskBits::IrqFiq);
            } else if irq {
                c.mask(MaskBits::Irq);
                c.unmask(MaskBits::Fiq);
            } else if fiq {
                c.mask(MaskBits::Fiq);
                c.unmask(MaskBits::Irq);
            } else {
                c.unmask(MaskBits::IrqFiq);
            }
        }
    }
    bits.bits()
}

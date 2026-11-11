//! Kernel0 `__aeabi_uidiv` at 0x0004_d978, one body under both stock spellings.
//!
//! The body is the loader instruction sequence at 0x0004_d978.

/// What the stock body leaves in `r0` when it raises the division fault.
///
/// 0x0004_da60 sets it to 0 for a zero dividend and 0xffff_ffff otherwise
/// before 0x0004_3614 raises `svc #0x1010`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DivisionFault {
    pub operand: u32,
}

/// Quotient of `dividend / divisor`, the image-0 body at 0x0004_d978.
pub fn unsigned_divide(dividend: u32, divisor: u32) -> Result<u32, DivisionFault> {
    if divisor == 0 {
        return Err(DivisionFault { operand: if dividend == 0 { 0 } else { u32::MAX } });
    }
    Ok(dividend / divisor)
}

#[cfg(target_arch = "arm")]
mod native {
    use super::unsigned_divide;

    fn quotient(dividend: u32, divisor: u32) -> u32 {
        match unsigned_divide(dividend, divisor) {
            Ok(value) => value,
            Err(fault) => unsafe {
                core::arch::asm!("svc #0x1010", "2: b 2b", in("r0") fault.operand, in("r1") 0u32, options(noreturn));
            },
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_record_helper_04d978(dividend: u32, divisor: u32) -> u32 {
        quotient(dividend, divisor)
    }

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_record_helper_4d978(dividend: u32, divisor: u32) -> u32 {
        quotient(dividend, divisor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotient_matches_the_shift_subtract_body() {
        assert_eq!(unsigned_divide(0, 7), Ok(0));
        assert_eq!(unsigned_divide(6, 7), Ok(0));
        assert_eq!(unsigned_divide(7, 7), Ok(1));
        assert_eq!(unsigned_divide(8, 7), Ok(1));
        assert_eq!(unsigned_divide(0xffff_ffff, 1), Ok(0xffff_ffff));
        assert_eq!(unsigned_divide(0xffff_ffff, 3), Ok(0x5555_5555));
        assert_eq!(unsigned_divide(1_000_000, 0x1_0000), Ok(15));
        assert_eq!(unsigned_divide(0x8000_0000, 0x8000_0000), Ok(1));
        assert_eq!(unsigned_divide(0x8000_0001, 0x8000_0000), Ok(1));
        assert_eq!(unsigned_divide(0xffff_ffff, 0x8000_0000), Ok(1));
    }

    #[test]
    fn zero_divisor_reports_the_operand_the_trap_carries() {
        assert_eq!(unsigned_divide(0, 0), Err(DivisionFault { operand: 0 }));
        assert_eq!(unsigned_divide(1, 0), Err(DivisionFault { operand: 0xffff_ffff }));
        assert_eq!(unsigned_divide(0xffff_ffff, 0), Err(DivisionFault { operand: 0xffff_ffff }));
    }
}

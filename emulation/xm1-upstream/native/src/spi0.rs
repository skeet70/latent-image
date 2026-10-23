//! SPI0 status word 0xfff8400c under the declared endpoint-10 polling profile.

use pyo3::prelude::*;

/// Bits 8 and 9: 0x00918a70 reports them, 0x00944568 waits for bit 9.
const DEVICE_STATUS: u32 = 0x300;

/// Written device-status bits clear; other written bits are discarded.
#[pyfunction]
pub(crate) fn spi0_status_acknowledged(status: u32, written: u32) -> u32 {
    status & !(written & DEVICE_STATUS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acknowledgement_clears_only_selected_device_status() {
        let mut status = DEVICE_STATUS;
        for (written, expected) in [
            (0x2000_0100, 0x200),
            (0xffff_0000, 0x200),
            (0xffff_0200, 0),
            (0xffff_ffff, 0),
            (0x300, 0),
        ] {
            status = spi0_status_acknowledged(status, written);
            assert_eq!(status, expected, "after writing {written:#x}");
        }
    }

    #[test]
    fn upper_acknowledgement_data_never_latches() {
        for upper in [0, 0x2000_0000, 0xffff_0000, 0x1357_0000, 0xaaaa_0000] {
            assert_eq!(spi0_status_acknowledged(0, upper | 0x3311), 0);
            assert_eq!(spi0_status_acknowledged(DEVICE_STATUS, upper), DEVICE_STATUS);
        }
    }
}

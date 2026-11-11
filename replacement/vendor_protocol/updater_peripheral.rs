//! X-M1 1.50 nonserial updater image and block consumers.
//!
//! Evidence: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3,
//! authenticated dc843c/dc85a0 extents and 6c0b48 instructions.

#[path = "updater_peripheral_gpio.rs"]
pub(crate) mod gpio;
#[path = "updater_peripheral_transaction.rs"]
pub(crate) mod transaction;
#[path = "updater_peripheral_io.rs"]
pub mod io;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImageError {
    Truncated,
    IdentityMismatch,
    InvalidExtent,
    EmptySelection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PeripheralIdentity([u8; 6]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PeripheralSnapshot([u8; 128]);

impl PeripheralSnapshot {
    pub(crate) const fn from_driver_bytes(bytes: [u8; 128]) -> Self {
        Self(bytes)
    }

    pub(crate) fn requires_handshake(&self) -> bool {
        self.0[8..16].iter().any(|byte| *byte != 0xff)
    }

    pub(crate) fn identity(&self) -> PeripheralIdentity {
        let mut identity = [0; 6];
        identity.copy_from_slice(&self.0[..6]);
        PeripheralIdentity(identity)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BlockIndex(u16);

impl BlockIndex {
    pub(crate) const fn from_driver_word(index: u16) -> Self {
        Self(index)
    }

    pub(crate) const fn driver_word(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BlockCount(u16);

impl BlockCount {
    pub(crate) const fn driver_word(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TransferAddress(u32);

impl TransferAddress {
    pub(crate) const fn driver_word(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BlockFormat(u8);

impl BlockFormat {
    pub(crate) const fn driver_byte(self) -> u8 {
        self.0
    }

    const fn byte_count(self) -> usize {
        1usize << self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Section {
    address: TransferAddress,
    offset: usize,
    count: BlockCount,
    format: BlockFormat,
}

impl Section {
    pub(crate) const fn address(self) -> TransferAddress {
        self.address
    }

    pub(crate) const fn count(self) -> BlockCount {
        self.count
    }

    pub(crate) const fn format(self) -> BlockFormat {
        self.format
    }
}

pub(crate) struct UpdateImage<'a> {
    bytes: &'a [u8],
    sections: [Option<Section>; 8],
}

impl<'a> UpdateImage<'a> {
    pub(crate) fn parse(bytes: &'a [u8], peripheral: PeripheralSnapshot) -> Result<Self, ImageError> {
        if bytes.len() < 0x200 {
            return Err(ImageError::Truncated);
        }
        if bytes[4..10] != peripheral.identity().0 && peripheral.requires_handshake() {
            return Err(ImageError::IdentityMismatch);
        }
        let mut image = Self { bytes, sections: [None; 8] };
        if word(bytes, 0x78) == 2 {
            let mut first = None;
            let mut length = 0u32;
            for bit in 0..8 {
                if bytes[0] & (1 << bit) == 0 {
                    if first.is_some() {
                        break;
                    }
                } else {
                    if first.is_none() {
                        first = Some(word(bytes, 0x1c + bit * 8));
                    }
                    length = length.checked_add(word(bytes, 0x20 + bit * 8)).ok_or(ImageError::InvalidExtent)?;
                }
            }
            let address = first.ok_or(ImageError::EmptySelection)?;
            if length == 0 {
                return Err(ImageError::EmptySelection);
            }
            let format = BlockFormat(if length <= 2048 { (32 - (length - 1).leading_zeros()) as u8 } else { 11 });
            let count = length.div_ceil(format.byte_count() as u32);
            let count = u16::try_from(count).map_err(|_| ImageError::InvalidExtent)?;
            let offset = address.checked_sub(word(bytes, 0x70)).ok_or(ImageError::InvalidExtent)?;
            image.add(0, address, offset, count, format)?;
        } else {
            match word(bytes, 0) {
                1 => {
                    let address = word(bytes, 0x1c);
                    image.add(0, address, address.checked_add(0x60000).ok_or(ImageError::InvalidExtent)?, 0x18, BlockFormat(11))?;
                    let address = word(bytes, 0x54);
                    image.add(7, address, address.checked_add(0x60000).ok_or(ImageError::InvalidExtent)?, 1, BlockFormat(11))?;
                }
                2 | 3 => {
                    let address = word(bytes, 0x24);
                    image.add(1, address, address.checked_add(0x60000).ok_or(ImageError::InvalidExtent)?, 0xa6, BlockFormat(11))?;
                }
                _ => {
                    let address = word(bytes, 0x1c);
                    image.add(5, address, address.checked_add(0x60000).ok_or(ImageError::InvalidExtent)?, 0xc0, BlockFormat(11))?;
                }
            }
        }
        Ok(image)
    }

    fn add(&mut self, slot: usize, address: u32, offset: u32, count: u16, format: BlockFormat) -> Result<(), ImageError> {
        let offset = (offset as usize).checked_add(0x200).ok_or(ImageError::InvalidExtent)?;
        let end = offset.checked_add(usize::from(count) * format.byte_count()).ok_or(ImageError::InvalidExtent)?;
        if end > self.bytes.len() {
            return Err(ImageError::Truncated);
        }
        self.sections[slot] = Some(Section { address: TransferAddress(address), offset, count: BlockCount(count), format });
        Ok(())
    }

    pub(crate) fn sections(&self) -> impl Iterator<Item = Section> + '_ {
        self.sections.iter().flatten().copied()
    }

    pub(crate) fn block(&self, section: Section, index: BlockIndex) -> Option<&'a [u8]> {
        let index = match index.0 {
            0xffff => 0,
            index if index < section.count.0 => usize::from(index),
            _ => return None,
        };
        let start = section.offset.checked_add(index * section.format.byte_count())?;
        self.bytes.get(start..start.checked_add(section.format.byte_count())?)
    }
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ObserverResult {
    Accepted,
    Rejected,
}

impl ObserverResult {
    pub(crate) const fn from_driver_status(status: i32) -> Self {
        match status {
            0 | -40 => Self::Accepted,
            _ => Self::Rejected,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BlockResult {
    Accepted,
    Retry,
    Rejected,
}

impl BlockResult {
    pub(crate) const fn from_driver_status(status: i32) -> Self {
        match status {
            0 => Self::Accepted,
            2 | 3 => Self::Retry,
            _ => Self::Rejected,
        }
    }

    pub(crate) const fn driver_stop(self, next: BlockIndex) -> bool {
        !matches!(self, Self::Rejected) && next.0 == 0xff00
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PassLimit(u8);

impl PassLimit {
    pub const fn from_configuration_byte(value: u8) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Advance {
    Block(BlockIndex),
    AwaitInterrupt,
    Setup(Section),
    Complete,
    Rejected,
}

pub(crate) struct TransferCursor {
    sections: [Option<Section>; 8],
    slot: usize,
    passes: u32,
    limit: PassLimit,
    retry: bool,
    rejected: bool,
    next: BlockIndex,
    awaiting_interrupt: bool,
}

impl TransferCursor {
    pub(crate) fn new(image: &UpdateImage<'_>, limit: PassLimit) -> Self {
        let slot = image.sections.iter().position(Option::is_some).unwrap_or(8);
        Self { sections: image.sections, slot, passes: 0, limit, retry: false, rejected: false, next: BlockIndex(0), awaiting_interrupt: false }
    }

    pub(crate) fn section(&self) -> Option<Section> {
        self.sections.get(self.slot).copied().flatten()
    }

    pub(crate) fn completed_block(&mut self, result: BlockResult, next: BlockIndex) -> Advance {
        if self.rejected || matches!(result, BlockResult::Rejected) {
            self.rejected = true;
            return Advance::Rejected;
        }
        if self.section().is_none() {
            return Advance::Complete;
        }
        self.retry |= matches!(result, BlockResult::Retry);
        self.next = next;
        self.awaiting_interrupt = next.0 != 0xff00;
        if self.awaiting_interrupt {
            return Advance::AwaitInterrupt;
        }
        self.advance()
    }

    pub(crate) fn interrupt(&mut self) -> Advance {
        if self.rejected {
            return Advance::Rejected;
        }
        if !self.awaiting_interrupt {
            return self.section().map_or(Advance::Complete, |_| Advance::Block(self.next));
        }
        self.awaiting_interrupt = false;
        self.advance()
    }

    fn advance(&mut self) -> Advance {
        let Some(section) = self.section() else {
            return Advance::Complete;
        };
        if self.next.0 < section.count.0 || self.next.0 == 0xffff {
            return Advance::Block(self.next);
        }
        self.passes += 1;
        if self.retry && self.passes < u32::from(self.limit.0) {
            self.retry = false;
            return Advance::Setup(section);
        }
        self.retry = false;
        self.slot += 1;
        while self.slot < self.sections.len() && self.sections[self.slot].is_none() {
            self.slot += 1;
        }
        self.section().map_or(Advance::Complete, Advance::Setup)
    }
}

#[cfg(test)]
mod tests {
    use super::{Advance, BlockIndex, BlockResult, ImageError, ObserverResult, PassLimit, PeripheralSnapshot, TransferCursor, UpdateImage};

    fn snapshot() -> PeripheralSnapshot {
        let mut bytes = [0xff; 128];
        bytes[..6].copy_from_slice(b"XM1CPU");
        PeripheralSnapshot::from_driver_bytes(bytes)
    }

    fn image() -> [u8; 0x210] {
        let mut bytes = [0; 0x210];
        bytes[0] = 0b00011010;
        bytes[4..10].copy_from_slice(b"XM1CPU");
        bytes[0x24..0x28].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[0x28..0x2c].copy_from_slice(&9u32.to_le_bytes());
        bytes[0x70..0x74].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[0x78..0x7c].copy_from_slice(&2u32.to_le_bytes());
        bytes[0x200..].copy_from_slice(b"0123456789abcdef");
        bytes
    }

    #[test]
    fn contiguous_selection_stops_at_first_gap_and_rounds_block_size() {
        let bytes = image();
        let parsed = UpdateImage::parse(&bytes, snapshot()).unwrap();
        let section = parsed.sections().next().unwrap();
        assert_eq!(section.address().driver_word(), 0x1000);
        assert_eq!(section.count().driver_word(), 1);
        assert_eq!(section.format().driver_byte(), 4);
        assert_eq!(parsed.block(section, BlockIndex::from_driver_word(0)), Some(&b"0123456789abcdef"[..]));
        assert_eq!(parsed.block(section, BlockIndex::from_driver_word(0xffff)), Some(&b"0123456789abcdef"[..]));
        assert_eq!(parsed.block(section, BlockIndex::from_driver_word(1)), None);
    }

    #[test]
    fn erased_peripheral_accepts_image_identity_mismatch() {
        let mut bytes = image();
        bytes[4] = 0;
        assert!(UpdateImage::parse(&bytes, snapshot()).is_ok());
        let mut identity = [0xff; 128];
        identity[8] = 0;
        assert!(matches!(UpdateImage::parse(&bytes, PeripheralSnapshot::from_driver_bytes(identity)), Err(ImageError::IdentityMismatch)));
    }

    #[test]
    fn truncated_payload_is_not_exposed_to_driver() {
        let bytes = image();
        assert!(matches!(UpdateImage::parse(&bytes[..0x20f], snapshot()), Err(ImageError::Truncated)));
    }

    #[test]
    fn observer_accepts_only_success_and_minus_forty() {
        assert_eq!(ObserverResult::from_driver_status(-40), ObserverResult::Accepted);
        assert_eq!(ObserverResult::from_driver_status(0), ObserverResult::Accepted);
        assert_eq!(ObserverResult::from_driver_status(1), ObserverResult::Rejected);
    }

    #[test]
    fn block_retry_stops_at_ff00_but_rejection_does_not() {
        let terminal = BlockIndex::from_driver_word(0xff00);
        assert!(BlockResult::from_driver_status(2).driver_stop(terminal));
        assert!(BlockResult::from_driver_status(3).driver_stop(terminal));
        assert!(!BlockResult::from_driver_status(1).driver_stop(terminal));
    }

    #[test]
    fn terminal_block_requires_physical_interrupt_before_completion() {
        let bytes = image();
        let parsed = UpdateImage::parse(&bytes, snapshot()).unwrap();
        let mut transfer = TransferCursor::new(&parsed, PassLimit::from_configuration_byte(1));
        assert_eq!(transfer.completed_block(BlockResult::Accepted, BlockIndex::from_driver_word(1)), Advance::AwaitInterrupt);
        assert_eq!(transfer.interrupt(), Advance::Complete);
        assert_eq!(transfer.interrupt(), Advance::Complete);
    }

    #[test]
    fn retry_pass_is_bounded_and_ff00_skips_interrupt_wait() {
        let bytes = image();
        let parsed = UpdateImage::parse(&bytes, snapshot()).unwrap();
        let section = parsed.sections().next().unwrap();
        let mut transfer = TransferCursor::new(&parsed, PassLimit::from_configuration_byte(2));
        assert_eq!(transfer.completed_block(BlockResult::Retry, BlockIndex::from_driver_word(0xff00)), Advance::Setup(section));
        assert_eq!(transfer.completed_block(BlockResult::Retry, BlockIndex::from_driver_word(0xff00)), Advance::Complete);
    }

    #[test]
    fn rejected_transfer_cannot_be_completed_by_late_interrupt() {
        let bytes = image();
        let parsed = UpdateImage::parse(&bytes, snapshot()).unwrap();
        let mut transfer = TransferCursor::new(&parsed, PassLimit::from_configuration_byte(2));
        assert_eq!(transfer.completed_block(BlockResult::Rejected, BlockIndex::from_driver_word(0)), Advance::Rejected);
        assert_eq!(transfer.interrupt(), Advance::Rejected);
    }
}

use crate::{backend::BackendError, configuration_storage::UploadStatus};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadTarget { Configuration, SerialPeripheral, PacketPeripheral }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Admission { Accepted(UploadTarget), Rejected(UploadStatus) }

fn word(image: &[u8], offset: usize) -> Result<u32, BackendError> {
    Ok(u32::from_le_bytes(image.get(offset..offset + 4).ok_or(BackendError::Capacity)?.try_into().unwrap()))
}

pub fn admit(image: &[u8]) -> Result<Admission, BackendError> {
    if word(image, 0x5c)? != 1 { return Ok(Admission::Accepted(UploadTarget::Configuration)); }
    let payload_bytes = word(image, 0x14)?;
    if word(image, 0x78)? == 2 {
        let flags = word(image, 0)?;
        for shift in 0..8 {
            if flags.wrapping_shl(shift) & 0x50 != 0 {
                let offset = (10 - shift as usize) * 8;
                let section_end = word(image, offset + 4)?.wrapping_add(word(image, offset + 8)?);
                let image_end = word(image, 0x70)?.wrapping_add(word(image, 0x74)?);
                if section_end > image_end { return Ok(Admission::Rejected(UploadStatus::InitializationFailure)); }
                break;
            }
        }
    } else if payload_bytes > 0x60000 {
        return Ok(Admission::Rejected(UploadStatus::InitializationFailure));
    }
    let start = 0x200usize;
    let end = start.checked_add(payload_bytes as usize).ok_or(BackendError::Capacity)?;
    let payload = image.get(start..end).ok_or(BackendError::Capacity)?;
    let sum = payload.iter().fold(0u32, |sum, value| sum.wrapping_add(u32::from(*value)));
    if word(image, 0x18)? != !sum { return Ok(Admission::Rejected(UploadStatus::ChecksumMismatch)); }
    Ok(Admission::Accepted(if word(image, 0x60)? == 1 { UploadTarget::SerialPeripheral } else { UploadTarget::PacketPeripheral }))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UploadResult { pub status: UploadStatus }
impl UploadResult {
    pub const fn phase(self) -> u32 { if matches!(self.status, UploadStatus::Complete) { 0 } else { 3 } }
    pub const fn flags(self) -> u32 { if matches!(self.status, UploadStatus::Complete) { 0 } else { 0x80000000 } }
    pub const fn return_code(self) -> i32 { if matches!(self.status, UploadStatus::Complete) { 0 } else { -255 } }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nonperipheral_images_bypass_peripheral_header_and_checksum_checks() {
        let mut image = [0xff; 0x200];
        image[0x5c..0x60].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(admit(&image), Ok(Admission::Accepted(UploadTarget::Configuration)));
    }
    #[test]
    fn peripheral_checksum_failure_precedes_transport_selection() {
        let mut image = [0; 0x201];
        image[0x5c..0x60].copy_from_slice(&1u32.to_le_bytes());
        image[0x14..0x18].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(admit(&image), Ok(Admission::Rejected(UploadStatus::ChecksumMismatch)));
        image[0x18..0x1c].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(admit(&image), Ok(Admission::Accepted(UploadTarget::PacketPeripheral)));
        image[0x60..0x64].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(admit(&image), Ok(Admission::Accepted(UploadTarget::SerialPeripheral)));
    }
}

use crate::{backend::{BackendError, ConfigImages}, configuration_actions_low::{ActionCompletion, ConfigurationKey}};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlaneDimension(pub u32);
#[derive(Clone, Copy, Debug)]
pub struct PlaneSamples<'a> { pub accumulators: &'a [u32; 1024], pub width: PlaneDimension, pub height: PlaneDimension }
#[derive(Clone, Copy, Debug, Default)]
pub struct RecordPlanes<'a> { pub primary: Option<PlaneSamples<'a>>, pub secondary: Option<PlaneSamples<'a>> }

impl PlaneSamples<'_> {
    fn average(self, index: u8) -> [u16; 4] {
        let divisor = self.width.0.wrapping_mul(self.height.0) / 4;
        if divisor == 0 { return [0; 4]; }
        core::array::from_fn(|plane| (self.accumulators[usize::from(index) + plane * 256] / divisor) as u16)
    }
}

pub fn query_plane_samples(configuration: &mut ConfigImages<'_>, planes: RecordPlanes<'_>) -> Result<ActionCompletion, BackendError> {
    let mut samples = [0; 8];
    for index in 0..8 {
        let high = ConfigurationKey::from_abi(0x83 + index as u16 * 2).read(configuration)?;
        let low = ConfigurationKey::from_abi(0x84 + index as u16 * 2).read(configuration)?;
        samples[index ^ 1] = u16::from_be_bytes([high, low]);
    }
    let selected = ConfigurationKey::from_abi(0x81).read(configuration)?;
    if let Some(primary) = planes.primary { samples[..4].copy_from_slice(&primary.average(selected)); }
    if let Some(secondary) = planes.secondary { samples[4..].copy_from_slice(&secondary.average(selected)); }
    for index in 0..8 {
        let [high, low] = samples[index ^ 1].to_be_bytes();
        ConfigurationKey::from_abi(0x83 + index as u16 * 2).write(configuration, high)?;
        ConfigurationKey::from_abi(0x84 + index as u16 * 2).write(configuration, low)?;
    }
    Ok(ActionCompletion::service_success())
}

#[cfg(target_arch = "arm")]
pub unsafe fn query_plane_samples_native(configuration: &mut ConfigImages<'_>) -> Result<ActionCompletion, BackendError> {
    unsafe fn plane(pointer: usize, width: usize, height: usize) -> Option<PlaneSamples<'static>> {
        let address = unsafe { core::ptr::read(pointer as *const u32) };
        if address == 0 { return None; }
        Some(PlaneSamples {
            accumulators: unsafe { &*(address as *const [u32; 1024]) },
            width: PlaneDimension(unsafe { core::ptr::read(width as *const u32) }),
            height: PlaneDimension(unsafe { core::ptr::read(height as *const u32) }),
        })
    }
    query_plane_samples(configuration, RecordPlanes {
        primary: unsafe { plane(0x016e_f6c0, 0x016e_f6c8, 0x016e_f6c4) },
        secondary: unsafe { plane(0x016e_22dc, 0x016e_22d4, 0x016e_22d8) },
    })
}

#[cfg(test)]
mod tests {
    use super::{query_plane_samples, PlaneDimension, PlaneSamples, RecordPlanes};
    use crate::backend::ConfigImages;
    #[test]
    fn primary_and_secondary_statistics_publish_big_endian_configuration_words() {
        let mut primary = [0; 1024]; let mut secondary = [0; 1024];
        for (index, value) in [0x1234, 0x2345, 0x3456, 0x4567].into_iter().enumerate() { primary[index * 256 + 7] = value * 4; }
        for (index, value) in [0x5678, 0x6789, 0x789a, 0x89ab].into_iter().enumerate() { secondary[index * 256 + 7] = value * 2; }
        let mut bytes = [0; 256]; bytes[0x81] = 7;
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        query_plane_samples(&mut configuration, RecordPlanes {
            primary: Some(PlaneSamples { accumulators: &primary, width: PlaneDimension(8), height: PlaneDimension(2) }),
            secondary: Some(PlaneSamples { accumulators: &secondary, width: PlaneDimension(4), height: PlaneDimension(2) }),
        }).unwrap();
        let expected = [0x23, 0x45, 0x12, 0x34, 0x45, 0x67, 0x34, 0x56, 0x67, 0x89, 0x56, 0x78, 0x89, 0xab, 0x78, 0x9a];
        for (index, byte) in expected.into_iter().enumerate() { assert_eq!(configuration.byte(0x83 + index as u32), Some(byte)); }
    }
    #[test]
    fn absent_plane_sources_preserve_existing_sample_words() {
        let mut bytes = [0; 256]; bytes[0x83..0x93].copy_from_slice(&[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16]);
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        query_plane_samples(&mut configuration, RecordPlanes::default()).unwrap();
        for index in 0..16 { assert_eq!(configuration.byte(0x83 + index), Some(index as u8 + 1)); }
    }
    #[test]
    fn zero_area_present_plane_clears_only_its_four_samples() {
        let source = [u32::MAX; 1024];
        let mut bytes = [0xff; 256];
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        query_plane_samples(&mut configuration, RecordPlanes { primary: Some(PlaneSamples { accumulators: &source, width: PlaneDimension(0), height: PlaneDimension(9) }), secondary: None }).unwrap();
        for index in 0..8 { assert_eq!(configuration.byte(0x83 + index), Some(0)); }
        for index in 8..16 { assert_eq!(configuration.byte(0x83 + index), Some(0xff)); }
    }
}

use crate::{backend::{BackendError, ConfigImages}, configuration_actions_low::{ActionCompletion, ConfigurationKey}};
use super::profiles::{FrameGeometry, PixelFormat, ProfileId, ProfileTables};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SampleDimensions { pub height: u32, pub width: u32 }
#[derive(Clone, Copy, Debug)]
pub struct CaptureStatistics<'a> { pub accumulators: &'a [u32; 1032], pub primary: SampleDimensions, pub auxiliary: SampleDimensions }
#[derive(Clone, Copy, Debug)]
pub struct FramePixels<'a> { pub bytes: &'a [u8], pub format: PixelFormat, pub geometry: FrameGeometry }
impl FramePixels<'_> {
    fn sample(self, x: u32, y: u32) -> Result<u32, BackendError> {
        let row = y.checked_mul(self.geometry.pitch).ok_or(BackendError::Capacity)?;
        let column = match self.format { PixelFormat::Unpacked16 => x.checked_mul(2), PixelFormat::Packed12 => x.checked_mul(3).map(|value| value / 2) }.ok_or(BackendError::Capacity)?;
        let offset = row.checked_add(column).ok_or(BackendError::Capacity)? as usize;
        let bytes = self.bytes.get(offset..offset + 2).ok_or(BackendError::Capacity)?;
        let word = u32::from(u16::from_le_bytes([bytes[0], bytes[1]]));
        Ok(match self.format { PixelFormat::Unpacked16 => word, PixelFormat::Packed12 => (if column % 3 == 0 { word & 0xfff } else { word >> 4 }).wrapping_mul(4) })
    }
}
fn write_words(configuration: &mut ConfigImages<'_>, values: &[u16]) -> Result<(), BackendError> {
    for (index, value) in values.iter().enumerate() {
        let [high, low] = value.to_be_bytes();
        ConfigurationKey::from_abi(0x83 + index as u16 * 2).write(configuration, high)?;
        ConfigurationKey::from_abi(0x84 + index as u16 * 2).write(configuration, low)?;
    }
    Ok(())
}
fn divide(value: u32, divisor: u32) -> u32 { if divisor == 0 { 0 } else { value / divisor } }
fn window(statistics: CaptureStatistics<'_>, index: u8) -> [u16; 4] {
    let area = statistics.primary.width.wrapping_mul(statistics.primary.height);
    if area == 0 { return [0; 4]; }
    let divisors = [(u64::from(area) * 2 / 9) as u32, (u64::from(area) * 5 / 9) as u32, (u64::from(area) * 2 / 9) as u32, (u64::from(area) * 5 / 9) as u32];
    core::array::from_fn(|plane| divide(statistics.accumulators[plane * 256 + usize::from(index)], divisors[plane]) as u16)
}
fn total(statistics: CaptureStatistics<'_>) -> [u16; 4] {
    let area = statistics.auxiliary.width.wrapping_mul(statistics.auxiliary.height);
    let sum = statistics.accumulators[0x404..0x408].iter().fold(0u32, |sum, value| sum.wrapping_add(*value));
    [divide(sum, area) as u16; 4]
}

pub fn select_samples(configuration: &mut ConfigImages<'_>, statistics: CaptureStatistics<'_>, raster: FramePixels<'_>, tables: &ProfileTables<'_>, profile: ProfileId) -> Result<ActionCompletion, BackendError> {
    let mode = ConfigurationKey::from_abi(0x82).read(configuration)?;
    write_words(configuration, &[0; 9])?;
    let high = mode >> 4; let low = mode & 15;
    if (high == 10 && low < 4) || (high == 11 && low == 0) {
        let cells = grid(configuration, raster, tables, profile)?;
        if high == 10 {
            let origin = usize::from(low / 2) * 18 + usize::from(low % 2) * 3;
            let values: [u16; 9] = core::array::from_fn(|index| cells[origin + index / 3 * 6 + index % 3]);
            write_words(configuration, &values)?;
        } else {
            let groups: [&[usize]; 6] = [&[0,2,7,12,14,21,23,28,33,35], &[9,11,24,26], &[4,16,19,31], &[3,5,10,15,17,18,20,25,30,32], &[1,13,22,34], &[6,8,27,29]];
            let values: [u16; 6] = core::array::from_fn(|index| (groups[index].iter().map(|cell| u32::from(cells[*cell])).sum::<u32>() / groups[index].len() as u32) as u16);
            write_words(configuration, &values)?;
        }
    } else {
        let selected = ConfigurationKey::from_abi(0x81).read(configuration)?;
        let (first, second) = if high == 0 && low == 1 { let values = total(statistics); (values, values) } else { (window(statistics, selected), window(statistics, selected.wrapping_add(1))) };
        let values = [first[1], first[0], first[3], first[2], second[1], second[0], second[3], second[2]];
        write_words(configuration, &values)?;
    }
    Ok(ActionCompletion::service_success())
}

fn grid(configuration: &ConfigImages<'_>, raster: FramePixels<'_>, tables: &ProfileTables<'_>, profile: ProfileId) -> Result<[u16; 36], BackendError> {
    let region = ConfigurationKey::from_abi(0x81).read(configuration)?;
    let bounds = tables.relative(profile, false, configuration)?;
    let width = (((bounds.width & 0xfffff) >> 4) / 6 * 6) as u16;
    let height = (((bounds.height & 0xfffff) >> 4) / 6 * 6) as u16;
    let x = u32::from(region & 15).wrapping_mul(u32::from(width)).wrapping_add(bounds.top).wrapping_add(bounds.width.wrapping_sub(u32::from(width) * 16) >> 1) as u16;
    let y = u32::from(region >> 4).wrapping_mul(u32::from(height)).wrapping_add(bounds.left).wrapping_add(bounds.height.wrapping_sub(u32::from(height) * 16) >> 1) as u16;
    let attributes = tables.attributes(profile);
    let phase_x = ((u32::from(x) % 6 + u32::from(attributes[0])) as u8) % 6;
    let phase_y = ((u32::from(y) % 6 + u32::from(attributes[1])) as u8) % 6;
    let mut sums = [0u32; 36]; let mut counts = [0u16; 36];
    for row in 0..height {
        for column in 0..width {
            let cell_x = ((u32::from(column) % 6 + u32::from(phase_x)) as u8) % 6;
            let cell_y = ((u32::from(row) % 6 + u32::from(phase_y)) as u8) % 6;
            let cell = usize::from(cell_y) * 6 + usize::from(cell_x);
            sums[cell] = sums[cell].wrapping_add(raster.sample(u32::from(x) + u32::from(column), u32::from(y) + u32::from(row))?);
            counts[cell] = counts[cell].wrapping_add(1);
        }
    }
    Ok(core::array::from_fn(|index| divide(sums[index], u32::from(counts[index])) as u16))
}

#[cfg(target_arch = "arm")]
pub unsafe fn select_samples_native(configuration: &mut ConfigImages<'_>) -> Result<ActionCompletion, BackendError> {
    let tables = unsafe { super::profiles::native_tables()? };
    let profile = ProfileId::parse(unsafe { core::ptr::read(0x014e_5b2c as *const u32) })?;
    let raw_format = ConfigurationKey::from_abi(0xec).read(configuration)?;
    let format = PixelFormat::from_configuration(raw_format);
    let mode = ConfigurationKey::from_abi(0x82).read(configuration)?;
    let geometry = if (mode >> 4 == 10 && mode & 15 < 4) || mode == 0xb0 { tables.geometry(profile, format, configuration)? } else { FrameGeometry::default() };
    let dimensions = unsafe { &*(0x014e_5b30 as *const [u32; 5]) };
    let statistics = CaptureStatistics {
        accumulators: unsafe { &*(0x5ffb_6120 as *const [u32; 1032]) },
        primary: SampleDimensions { height: dimensions[1], width: dimensions[2] },
        auxiliary: SampleDimensions { height: dimensions[3], width: dimensions[4] },
    };
    let raster = FramePixels { bytes: unsafe { core::slice::from_raw_parts(0x437c_ee00 as *const u8, geometry.size as usize) }, format: if raw_format == 0 { PixelFormat::Unpacked16 } else { PixelFormat::Packed12 }, geometry };
    select_samples(configuration, statistics, raster, &tables, profile)
}

#[cfg(test)]
#[path = "configuration_imaging_statistics_tests.rs"]
mod tests;

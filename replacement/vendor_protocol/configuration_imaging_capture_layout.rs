use crate::{backend::{BackendError, ConfigImages}, configuration_actions_low::ConfigurationKey};
use super::profiles::{FrameGeometry, PixelFormat, ProfileId, ProfileTables};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(transparent)]
pub struct FrameAddress(pub u32);
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct OutputDescriptor { pub buffer: FrameAddress, pub row_origin: u16, pub row_count: u16, pub column_origin: u16, pub column_count: u16, pub pitch: u32, pub size: u32 }
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u32)]
pub enum OutputFormat { #[default] Unpacked = 0, Packed = 1, Profile = 2 }
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct CaptureOutputLayout { pub first: OutputDescriptor, pub second: OutputDescriptor, pub reserved28: [u32; 5], pub format: OutputFormat, pub reserved40: [u32; 2], pub status: i32, pub reserved4c: u32, pub completed: u32, pub reserved54: u32 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputProfile { WholeFrame, Single(ProfileId) }

pub fn prepare_output(buffer: FrameAddress, selection: OutputProfile, current: ProfileId, configuration: &ConfigImages<'_>, tables: &ProfileTables<'_>, output: &mut CaptureOutputLayout) -> Result<(), BackendError> {
    if buffer.0 == 0 { return Err(BackendError::Range); }
    let (geometry, bounds, format) = match selection {
        OutputProfile::WholeFrame => {
            let packing = ConfigurationKey::from_abi(0xec).read(configuration)?;
            let bounds = tables.full(current);
            (FrameGeometry::from_bounds(bounds, PixelFormat::from_configuration(packing)), bounds, if packing == 0 { OutputFormat::Unpacked } else { OutputFormat::Packed })
        }
        OutputProfile::Single(profile) => (tables.geometry(profile, PixelFormat::Unpacked16, configuration)?, tables.relative(profile, true, configuration)?, OutputFormat::Profile),
    };
    let row = bounds.left.wrapping_add(bounds.height >> 1) as u16;
    let row_count = geometry.height.wrapping_sub(u32::from(row)) as u16;
    let column = bounds.top.wrapping_add(bounds.width >> 1) as u16;
    let column_count = geometry.width.wrapping_sub(u32::from(column)) as u16;
    output.first = OutputDescriptor { buffer, row_origin: row, row_count, column_origin: column, column_count, pitch: geometry.pitch, size: u32::from(row).wrapping_add(u32::from(row_count)).wrapping_mul(geometry.pitch) };
    output.second = OutputDescriptor::default();
    output.format = format;
    output.status = 0;
    output.completed = 0;
    Ok(())
}

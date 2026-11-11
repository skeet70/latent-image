use crate::record_address_space::{GuestAddressSpace, MemoryError};
use super::profiles::{GeometryKind, ProfileGeometry, ProfileId};
use super::div;

pub fn transform<M: GuestAddressSpace, E>(memory: &mut crate::record_address_space::RecordContext<M,E>, profiles: &mut ProfileGeometry, input: [u32; 6]) -> Result<[u32; 8], MemoryError> { let profile = ProfileId(input[0]);
let kind = GeometryKind(input[5]);
let outer = profiles.rectangle(memory, profile, kind)?;
let ratio_height = (input[4] >> 16).max(1);
let ratio_width = (input[4] & 0xffff).max(1);
let horizontal = profiles.horizontal_scale(memory, profile, kind)?;
let vertical = profiles.vertical_scale(memory, profile, kind)?;
let width = outer[3];
let height = outer[2];
let denominator = ratio_height.wrapping_mul(vertical);
let numerator = ratio_width.wrapping_mul(horizontal);
let candidate = div(width.wrapping_mul(numerator), denominator);
let limited = if ratio_height == 4 { candidate.min(div(width.wrapping_mul(3).wrapping_mul(horizontal), vertical.wrapping_mul(4))) } else { candidate.min(height) };
let adjusted_width = div(denominator.wrapping_mul(limited), numerator);
let removed = div(input[3].wrapping_mul(input[1]).wrapping_mul(adjusted_width), width.wrapping_mul(horizontal));
let inner_width = adjusted_width.saturating_sub(removed);
let inner_height = div(numerator.wrapping_mul(inner_width), denominator).min(height);
Ok([outer[0], outer[1], height, width, outer[0].wrapping_add(width.wrapping_sub(inner_width) >> 1), outer[1].wrapping_add(height.wrapping_sub(inner_height) >> 1), inner_height, inner_width]) }

pub fn rescale<M: GuestAddressSpace, E>(memory: &mut crate::record_address_space::RecordContext<M,E>, profiles: &mut ProfileGeometry, scale: u32, rectangle: &mut [u32; 5]) -> Result<(), MemoryError> { if rectangle[2] == 0 { return Ok(()); }
let reference = profiles.rectangle(memory, ProfileId(1), GeometryKind(1))?;
let horizontal = profiles.horizontal_scale(memory, ProfileId(1), GeometryKind(1))?;
let limit = div(reference[3].wrapping_mul(100u32.wrapping_sub(scale)).wrapping_mul(horizontal), 100u32.wrapping_mul(rectangle[2]));
if limit < rectangle[0] { rectangle[0] = limit; }
Ok(()) }

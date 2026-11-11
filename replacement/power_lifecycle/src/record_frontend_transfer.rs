use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_hardware_deep_calibration::{minimum_dimensions, weights};
use crate::record_configuration::{helper_69f8fc, helper_69fec8, read_byte, ConfigurationKey};

fn area<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, dimensions: Option<GuestAddress>) -> Result<u32, MemoryError> {
    let Some(dimensions) = dimensions else { return Ok(0); };
    let width = context.read_u32(dimensions)?;
    Ok(width.wrapping_mul(context.read_u32(dimensions.checked_add(4)?)?))
}
fn read_wide<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, address: GuestAddress) -> Result<u64, MemoryError> {
    let low = context.read_u32(address)?;
    let high = context.read_u32(address.checked_add(4)?)?;
    Ok(u64::from(low) | (u64::from(high) << 32))
}
fn write_wide<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, address: GuestAddress, value: u64) -> Result<(), MemoryError> {
    context.write_u32(address, value as u32)?;
    context.write_u32(address.checked_add(4)?, (value >> 32) as u32)
}

pub fn helper_ffc3fc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, grid: Option<GuestAddress>, grid_dimensions: Option<GuestAddress>, regions: Option<GuestAddress>, region_dimensions: Option<GuestAddress>) -> Result<(), MemoryError> {
    let minimum = minimum_dimensions(context)?;
    let minimum_area = minimum.width.wrapping_mul(minimum.height);
    let mut black = [0; 4];
    for (index, key) in [0x1378, 0x1376, 0x137c, 0x137a].into_iter().enumerate() { black[index] = u32::from(helper_69fec8(context, key)?); }
    if let Some(grid) = grid {
        let count = area(context, grid_dimensions)?.max(minimum_area);
        let scale = weights(context, count)?.channels;
        for cell in 0..256 { for channel in 0..4 {
            let address = grid.checked_add(channel as u32 * 1024 + cell * 4)?;
            let value = context.read_u32(address)?.saturating_sub(scale[channel].wrapping_mul(black[channel]));
            context.write_u32(address, value)?;
        } }
    }
    if let Some(regions) = regions { for region in 0..10 {
        let dimensions = match region_dimensions { Some(base) => Some(base.checked_add(region * 8)?), None => None };
        let measured = area(context, dimensions)?.max(minimum_area);
        let scale = weights(context, measured)?.channels;
        for channel in 0..4 {
            let address = regions.checked_add(region * 32 + channel as u32 * 8)?;
            let value = read_wide(context, address)?.saturating_sub(u64::from(scale[channel]) * u64::from(black[channel]));
            write_wide(context, address, value)?;
        }
    } }
    Ok(())
}

pub fn helper_ffc97c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, grid: Option<GuestAddress>, grid_dimensions: Option<GuestAddress>, regions: Option<GuestAddress>, region_dimensions: Option<GuestAddress>, black: Option<GuestAddress>, black_dimensions: Option<GuestAddress>, coordinate: i16) -> Result<(), MemoryError> {
    let minimum = minimum_dimensions(context)?;
    let threshold = i32::from(read_byte(context, ConfigurationKey(0x1386))?).wrapping_mul(10);
    if i32::from(coordinate) < threshold || black.is_none() { return Ok(()); }
    let black = black.unwrap();
    let minimum_area = minimum.width.wrapping_mul(minimum.height);
    let grid_area = area(context, grid_dimensions)?.max(minimum_area);
    let grid_scale = weights(context, grid_area)?.channels;
    let black_area = area(context, black_dimensions)?.max(1);
    let mut offsets = [0u32; 4];
    for (channel, (first, second)) in [(0x1378, 0x1380), (0x1376, 0x137e), (0x137c, 0x1384), (0x137a, 0x1382)].into_iter().enumerate() {
        let first = u32::from(helper_69fec8(context, first)?);
        offsets[channel] = first.wrapping_add(u32::from(helper_69fec8(context, second)?));
    }
    let mut total = 0u64;
    for channel in 0..4 { total += u64::from(context.read_u32(black.checked_add(channel * 4)?)?); }
    if let Some(grid) = grid {
        let subtraction = grid_scale.map(|scale| (u64::from(scale).wrapping_mul(total) / u64::from(black_area)) as u32);
        for cell in 0..256 { for channel in 0..4 {
            let address = grid.checked_add(channel as u32 * 1024 + cell * 4)?;
            let value = context.read_u32(address)?.wrapping_add(grid_scale[channel].wrapping_mul(offsets[channel])).saturating_sub(subtraction[channel]).min(grid_scale[channel].wrapping_mul(0x3fff));
            context.write_u32(address, value)?;
        } }
    }
    if let Some(regions) = regions { for region in 0..10 {
        let dimensions = match region_dimensions { Some(base) => Some(base.checked_add(region * 8)?), None => None };
        let count = area(context, dimensions)?.max(minimum_area);
        let scale = weights(context, count)?.channels;
        for channel in 0..4 {
            let address = regions.checked_add(region * 32 + channel as u32 * 8)?;
            let subtraction = (u64::from(scale[channel]).wrapping_mul(total) / u64::from(black_area)) as u32;
            let addition = scale[channel].wrapping_mul(offsets[channel]);
            let value = read_wide(context, address)?.wrapping_add(u64::from(addition)).saturating_sub(u64::from(subtraction)).min(u64::from(scale[channel]) * 0x3fff);
            write_wide(context, address, value)?;
        }
    } }
    Ok(())
}

pub fn helper_ffc668<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, grid: Option<GuestAddress>, regions: Option<GuestAddress>, parameters: GuestAddress) -> Result<(), MemoryError> {
    let profile = context.read_u32(parameters.checked_add(116)?)?;
    let dimensions = crate::record_image_geometry_query::profiles::helper_ef5b5c(context, profile)?;
    let minimum_area = dimensions[0].wrapping_mul(dimensions[1]);
    let output_width = weights(context, minimum_area)?.total;
    if let Some(grid) = grid {
        let grid_area = area(context, Some(parameters))?;
        let scale = weights(context, grid_area)?;
        let target = (scale.total >> 2).max(minimum_area);
        let limit = target.wrapping_mul(0x3fff);
        for cell in 0..256 { for channel in 0..4 {
            let address = grid.checked_add(channel as u32 * 1024 + cell * 4)?;
            let value = (u64::from(target) * u64::from(context.read_u32(address)?) / u64::from(scale.channels[channel])) as u32;
            context.write_u32(address, value.min(limit))?;
        } }
        context.write_u32(parameters.checked_add(4)?, scale.total / output_width)?;
        context.write_u32(parameters, output_width)?;
    }
    if let Some(regions) = regions { for region in 0..10 {
        let dimensions = parameters.checked_add(16 + region * 8)?;
        let region_area = area(context, Some(dimensions))?.max(minimum_area);
        let scale = weights(context, region_area)?;
        let target = (scale.total >> 2).max(minimum_area);
        let limit = u64::from(target) * 0x3fff;
        for channel in 0..4 {
            let address = regions.checked_add(region * 32 + channel as u32 * 8)?;
            let value = read_wide(context, address)?.wrapping_mul(u64::from(target)) / u64::from(scale.channels[channel]);
            write_wide(context, address, value.min(limit))?;
        }
        context.write_u32(dimensions.checked_add(4)?, scale.total / output_width)?;
        context.write_u32(dimensions, output_width)?;
    } }
    Ok(())
}

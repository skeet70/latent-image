use super::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use super::record_hardware_deep_calibration::{channel_areas, configuration_byte, helper_ef5448, helper_ef5310, calibration_values};

fn area<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, dimensions: GuestAddress) -> Result<u32, MemoryError> {
    if dimensions.get() == 0 { return Ok(0); }
    let width = context.read_u32(dimensions)?;
    let height = context.read_u32(dimensions.checked_add(4)?)?;
    Ok(width.wrapping_mul(height))
}

fn adjusted_word(sample: u32, product: u64, baseline: u64) -> u32 {
    if baseline < product.wrapping_add(u64::from(sample)) { sample.wrapping_add((product as u32).wrapping_sub(baseline as u32)) } else { 0 }
}

pub fn helper_fe9d30<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, grid: GuestAddress, dimensions: GuestAddress, regions: GuestAddress, region_dimensions: GuestAddress, black: GuestAddress, black_dimensions: GuestAddress, profile: u32, gain: i32) -> Result<u32, MemoryError> {
    let gain = gain as i16 as i32;
    let width = context.read_u32(GuestAddress::new(0x00ef_6ddc))?;
    let height = context.read_u32(GuestAddress::new(0x00ef_6de0))?;
    let minimum_area = width.wrapping_mul(height);
    if helper_ef5448(context, profile)? != 4 { return Ok(0); }
    let threshold = (configuration_byte(context, 0x63a4)?.wrapping_mul(5) << 17) as i32 >> 17;
    if gain.wrapping_add(helper_ef5310(context, profile)?) < threshold.wrapping_mul(2) { return Ok(0); }
    if black.get() == 0 { return Ok(132); }
    let grid_area = area(context, dimensions)?.max(minimum_area);
    let counts = channel_areas(context, grid_area)?;
    let denominator = area(context, black_dimensions)?.max(1);
    let calibration = calibration_values(context, gain, profile, 1)?;
    let mut total = 0u64;
    for channel in 0..4 { total += u64::from(context.read_u32(black.checked_add(channel * 4)?)?); }
    if grid.get() != 0 {
        for channel in 0..4 {
            let product = u64::from(calibration[channel]) * u64::from(counts[channel]);
            let baseline = total.wrapping_mul(u64::from(counts[channel])) / u64::from(denominator);
            for sample in 0..256 {
                let address = grid.checked_add(channel as u32 * 1024 + sample * 4)?;
                let original = context.read_u32(address)?;
                context.write_u32(address, adjusted_word(original, product, u64::from(baseline as u32)))?;
            }
        }
    }
    if regions.get() != 0 {
        let mut previous_area = 0;
        for region in 0..10 {
            let dimensions = region_dimensions.checked_add(region * 8)?;
            if dimensions.get() != 0 { previous_area = area(context, dimensions)?; }
            previous_area = previous_area.max(minimum_area);
            let counts = channel_areas(context, previous_area)?;
            let baselines = core::array::from_fn::<_, 4, _>(|channel| total.wrapping_mul(u64::from(counts[channel])) / u64::from(denominator));
            let mut old = [0u64; 4];
            for channel in 0..4 {
                let address = regions.checked_add(region * 32 + channel as u32 * 8)?;
                let low = context.read_u32(address)?;
                let high = context.read_u32(address.checked_add(4)?)?;
                old[channel] = u64::from(low) | u64::from(high) << 32;
            }
            for channel in 0..4 {
                let comparison_product = u64::from(counts[channel]) * u64::from(calibration[0]);
                let output = if baselines[channel] < old[channel].wrapping_add(comparison_product) {
                    counts[channel].wrapping_mul(calibration[channel]).wrapping_sub(baselines[channel] as u32).wrapping_add(old[channel] as u32)
                } else { 0 };
                let address = regions.checked_add(region * 32 + channel as u32 * 8)?;
                context.write_u32(address, output)?;
                context.write_u32(address.checked_add(4)?, 0)?;
            }
        }
    }
    Ok(0)
}

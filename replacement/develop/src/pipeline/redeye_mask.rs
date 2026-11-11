use core::ptr::{copy, read, read_volatile, write, write_bytes};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PixelAddress(pub u32);

#[derive(Clone, Copy)]
#[repr(C)]
pub struct MaskImage {
    pub pixels: PixelAddress,
    pub height: i32,
    pub width: i32,
}

impl MaskImage {
    unsafe fn get(self, offset: i32) -> u8 {
        unsafe { read((self.pixels.0.wrapping_add(offset as u32)) as *const u8) }
    }
    unsafe fn put(self, offset: i32, value: u8) {
        unsafe { write((self.pixels.0.wrapping_add(offset as u32)) as *mut u8, value) };
    }
    fn length(self) -> usize { self.height.wrapping_mul(self.width) as u32 as usize }
    unsafe fn clear(self) {
        unsafe { write_bytes(self.pixels.0 as *mut u8, 0, self.length()) };
    }
    unsafe fn copy_to(self, destination: Self, length: usize) {
        unsafe { copy(self.pixels.0 as *const u8, destination.pixels.0 as *mut u8, length) };
    }
}

#[repr(C)]
pub struct Edit {
    pub gains: [i32; 3],
    pub center_x_q4: i32,
    pub diameter: i32,
    pub center_y_q4: i32,
    pub mask_extent: i32,
    pub sampling_scale: i32,
    pub mask: MaskImage,
    pub format: u32,
    pub tag: u16,
    pub kind: u16,
    pub side: i16,
    pub padding: u16,
}

extern "C" {
    fn xm1_pipeline_image_redeye_divide(numerator: i32, denominator: i32) -> i32;
}

unsafe fn divide(numerator: i32, denominator: i32) -> i32 {
    unsafe { xm1_pipeline_image_redeye_divide(numerator, denominator) }
}
unsafe fn setting(offset: usize) -> i32 {
    unsafe { read_volatile((0x014e213e + offset) as *const u8) as i32 }
}
fn square(value: i32) -> i32 { value.wrapping_mul(value) }
fn radius_squared(x: i32, y: i32, center_x: i32, center_y: i32) -> i32 {
    square(x.wrapping_sub(center_x)).wrapping_add(square(y.wrapping_sub(center_y)))
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_chroma_score(source: *const MaskImage, destination: *const MaskImage) {
    let source = unsafe { read(source) };
    let destination = unsafe { read(destination) };
    let mut output = 0;
    for row in 0..destination.height {
        for column in 0..destination.width {
            let pair = row.wrapping_mul(source.width >> 1).wrapping_add(column >> 1).wrapping_mul(4);
            let y = unsafe { source.get(pair.wrapping_add(column & 1)) } as i32;
            let u = unsafe { source.get(pair.wrapping_add(2)) } as i32;
            let v = unsafe { source.get(pair.wrapping_add(3)) } as i32;
            let score = ((198 * v - 98 * y - 43 * u) >> 8) + 99;
            unsafe { destination.put(output, score.clamp(0, 255) as u8) };
            output += 1;
        }
    }
}

#[derive(Clone, Copy)]
enum Morphology { Dilate, Erode }
#[derive(Clone, Copy)]
enum Kernel { Cross, Square }

unsafe fn morphology(image: MaskImage, work: MaskImage, operation: Morphology, kernel: Kernel) {
    if matches!((operation, kernel), (Morphology::Erode, Kernel::Cross)) {
        unsafe { work.clear() };
    } else {
        unsafe { image.copy_to(work, work.length()) };
    }
    let stride = image.width.max(2);
    for row in 1..image.height.wrapping_sub(1) {
        for column in 1..image.width.wrapping_sub(1) {
            let center = row.wrapping_mul(stride).wrapping_add(column);
            let mut value = unsafe { image.get(center) };
            for (dy, dx) in [(-1, 0), (0, -1), (0, 1), (1, 0), (-1, -1), (-1, 1), (1, -1), (1, 1)] {
                if matches!(kernel, Kernel::Cross) && dx != 0 && dy != 0 { continue; }
                let neighbor = unsafe { image.get(center.wrapping_add(dy * stride + dx)) };
                value = match operation { Morphology::Dilate => value | neighbor, Morphology::Erode => value & neighbor };
            }
            let output = work.width.wrapping_add((row - 1).wrapping_mul(stride)).wrapping_add(column);
            unsafe { work.put(output, value) };
        }
    }
    unsafe { work.copy_to(image, image.length()) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_dilate_square(image: *const MaskImage, work: *const MaskImage) {
    unsafe { morphology(read(image), read(work), Morphology::Dilate, Kernel::Square) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_erode_square(image: *const MaskImage, work: *const MaskImage) {
    unsafe { morphology(read(image), read(work), Morphology::Erode, Kernel::Square) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_dilate_cross(image: *const MaskImage, work: *const MaskImage) {
    unsafe { morphology(read(image), read(work), Morphology::Dilate, Kernel::Cross) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_erode_cross(image: *const MaskImage, work: *const MaskImage) {
    unsafe { morphology(read(image), read(work), Morphology::Erode, Kernel::Cross) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_count_mask_pixels(image: *const MaskImage) -> i32 {
    let image = unsafe { read(image) };
    let mut count = 0_i32;
    for row in 0..image.height {
        for column in 0..image.width {
            count = count.wrapping_add((unsafe { image.get(row.wrapping_mul(image.width).wrapping_add(column)) } != 0) as i32);
        }
    }
    count
}

#[no_mangle]
pub extern "C" fn xm1_pipeline_image_redeye_intervals_touch(first_left: i32, first_right: u32, second_left: i32, second_right: u32) -> i32 {
    ((second_left.wrapping_sub(1) <= first_right as i32 && first_right <= second_right)
        || (first_left.wrapping_sub(1) <= second_right as i32 && second_right <= first_right)) as i32
}
#[no_mangle]
pub extern "C" fn xm1_pipeline_image_redeye_endpoints_within(first: i32, second: i32, low: i32, high: i32) -> i32 {
    (first >= low.wrapping_sub(1) && first <= high.wrapping_add(1)
        && second >= low.wrapping_sub(1) && second <= high.wrapping_add(1)) as i32
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_fill_mask_rows(image: *const MaskImage, work: *const MaskImage) {
    let image = unsafe { read(image) };
    let work = unsafe { read(work) };
    unsafe { work.clear() };
    for row in 0..image.height {
        let mut active = false;
        for column in 0..image.width {
            let offset = row.wrapping_mul(image.width).wrapping_add(column);
            let pixel = unsafe { image.get(offset) };
            if (row.wrapping_sub(14) as u32) > 36 {
                if pixel != 0 { unsafe { work.put(offset, 255) }; }
                continue;
            }
            if !active && pixel == 0 { continue; }
            if pixel == 0 && !(1..=5).any(|distance| unsafe { image.get(offset.wrapping_add(distance)) } != 0) {
                active = false;
                continue;
            }
            let h = image.height;
            let neighbors = [3 - 3 * h, 3 - 4 * h, 3 - 5 * h, 3 - 6 * h, -h, h];
            if pixel != 0 || neighbors.iter().any(|delta| unsafe { image.get(offset.wrapping_add(*delta)) } != 0) {
                unsafe { work.put(offset, 255) };
            }
            active = true;
        }
    }
    unsafe { work.copy_to(image, image.length()) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_fill_mask_columns(image: *const MaskImage, work: *const MaskImage) {
    let image = unsafe { read(image) };
    let work = unsafe { read(work) };
    unsafe { work.clear() };
    for column in 0..image.width {
        let mut active = false;
        for row in 0..image.height {
            let offset = row.wrapping_mul(image.width).wrapping_add(column);
            if (column.wrapping_sub(14) as u32) > 36 {
                if unsafe { image.get(offset.wrapping_add(image.width)) } != 0 { unsafe { work.put(offset, 255) }; }
                continue;
            }
            let pixel = unsafe { image.get(offset) };
            if !active {
                if pixel != 0 { unsafe { work.put(offset, 255) }; active = true; }
                continue;
            }
            if pixel != 0 { unsafe { work.put(offset, 255) }; continue; }
            if !(1..=5).any(|distance| unsafe { image.get(offset.wrapping_add(distance * image.width)) } != 0) {
                active = false;
                continue;
            }
            let w = image.width;
            let neighbors = [3 * w - 3, 3 * w - 4, 3 * w - 5, 3 * w - 6, -1, 1];
            if neighbors.iter().any(|delta| unsafe { image.get(offset.wrapping_add(*delta)) } != 0) {
                unsafe { work.put(offset, 255) };
            }
        }
    }
    unsafe { work.copy_to(image, image.length()) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_filter_mask(mask: *const MaskImage, _color: *const MaskImage, basis: *const MaskImage, distance_squared: i32) {
    let mask = unsafe { read(mask) };
    if mask.height <= 0 { return; }
    let mut weight = 0_i32;
    let mut weighted_x = 0_i32;
    let mut weighted_y = 0_i32;
    for row in 0..mask.height {
        for column in 0..mask.width {
            let value = unsafe { mask.get(row.wrapping_mul(mask.width).wrapping_add(column)) } as i32;
            weighted_x = weighted_x.wrapping_add(value.wrapping_mul(column));
            weighted_y = weighted_y.wrapping_add(value.wrapping_mul(row));
            weight = weight.wrapping_add(value);
        }
    }
    if weight == 0 { return; }
    let center_x = unsafe { divide(weighted_x, weight) };
    let center_y = unsafe { divide(weighted_y, weight) };
    let outer = unsafe { setting(5) };
    let inner = unsafe { setting(6) };
    let gain = unsafe { setting(4) };
    for row in 0..mask.height {
        for column in 0..mask.width {
            let offset = row.wrapping_mul(mask.width).wrapping_add(column);
            let radius = unsafe { divide(radius_squared(column, row, center_x, center_y).wrapping_mul(10000), distance_squared) };
            let value = if radius > outer { 0 } else {
                let scale = if radius > inner { outer.wrapping_sub(radius).wrapping_mul(gain) / 119 } else { gain };
                (scale.wrapping_mul(unsafe { mask.get(offset) } as i32) >> 8).clamp(0, 255) as u8
            };
            unsafe { mask.put(offset, value) };
        }
    }
    if unsafe { setting(8) } != 0 {
        let basis = unsafe { read(basis) };
        for row in 0..mask.height {
            for column in 0..mask.width {
                let offset = row.wrapping_mul(mask.width).wrapping_add(column);
                if unsafe { basis.get(offset) } == 0 { unsafe { mask.put(offset, 0) }; }
            }
        }
    }
}

unsafe fn histogram_cutoff(histogram: *const i32, multiplier: i32, area: i32) -> i32 {
    let mut total = unsafe { read(histogram) };
    if total.wrapping_mul(multiplier) > area { return 0; }
    for value in 1..255 {
        total = total.wrapping_add(unsafe { read(histogram.add(value)) });
        if total.wrapping_mul(multiplier) > area { return value as i32; }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_mask_gains(image: *const MaskImage, edit: *mut Edit, luminance_histogram: *mut i32, chroma_histogram: *mut i32) {
    let image = unsafe { read(image) };
    unsafe { write_bytes(luminance_histogram, 0, 256); write_bytes(chroma_histogram, 0, 256) };
    let mask = unsafe { read(core::ptr::addr_of!((*edit).mask)) };
    if mask.height <= 0 { return; }
    let mut luma_sum = 0_i32;
    let mut chroma_sum = 0_i32;
    let mut count = 0_i32;
    for row in 0..mask.height {
        for column in 0..mask.width {
            let pair = row.wrapping_mul(image.width >> 1).wrapping_add(column >> 1).wrapping_mul(4);
            let y = unsafe { image.get(pair.wrapping_add(column & 1)) } as i32;
            let u = unsafe { image.get(pair.wrapping_add(2)) } as i32;
            let v = unsafe { image.get(pair.wrapping_add(3)) } as i32;
            let chroma = (u - 128).abs().max((v - 128).abs());
            unsafe {
                let bin = luminance_histogram.add(y as usize);
                write(bin, read(bin).wrapping_add(1));
                let bin = chroma_histogram.add(chroma as usize);
                write(bin, read(bin).wrapping_add(1));
            }
            if unsafe { mask.get(row.wrapping_mul(mask.width).wrapping_add(column)) } != 0 {
                luma_sum = luma_sum.wrapping_add(y);
                chroma_sum = chroma_sum.wrapping_add(chroma);
                count = count.wrapping_add(1);
            }
        }
    }
    if count == 0 { return; }
    let mean_y = unsafe { divide(luma_sum, count) };
    let mean_chroma = unsafe { divide(chroma_sum, count) };
    let area = mask.width.wrapping_mul(mask.height);
    let cutoff_y = unsafe { histogram_cutoff(luminance_histogram, setting(1), area) };
    let cutoff_chroma = unsafe { histogram_cutoff(chroma_histogram, setting(7), area) };
    let gain_y = if mean_y == 0 { 1 } else { unsafe { divide(cutoff_y.wrapping_shl(8), mean_y) } };
    let gain_chroma = if mean_chroma == 0 { 1 } else { unsafe { divide(cutoff_chroma.wrapping_shl(8), mean_chroma) } };
    unsafe { write(core::ptr::addr_of_mut!((*edit).gains), [gain_y, gain_chroma, gain_chroma]) };
}

#[derive(Clone, Copy, PartialEq)]
#[repr(u8)]
enum ComponentState { Vacant = 0, New = 1, Seed = 2, Fork = 3, Accepted = 5 }

#[repr(C)]
pub struct Component {
    state: ComponentState,
    first_row: u8,
    length: u8,
    branches: [[u8; 4]; 48],
}

#[repr(C)]
pub struct Components { records: [Component; 10] }

unsafe fn component_byte(base: *mut Components, index: usize, offset: usize) -> u8 {
    unsafe { read((base as *const u8).add(index * 195 + offset)) }
}
unsafe fn set_component_byte(base: *mut Components, index: usize, offset: usize, value: u8) {
    unsafe { write((base as *mut u8).add(index * 195 + offset), value) };
}
fn touching(first: [u8; 2], second: [u8; 2]) -> bool {
    xm1_pipeline_image_redeye_intervals_touch(first[0] as i32, first[1] as u32, second[0] as i32, second[1] as u32) != 0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_fill_fork_gaps(image: *const MaskImage, work: *const MaskImage, components: *mut Components) {
    let image = unsafe { read(image) };
    let work = unsafe { read(work) };
    unsafe { work.clear(); write_bytes(components as *mut u8, 0, 1950) };
    let mut previous_length = 0_usize;
    for row in 8..56 {
        let mut runs = [[0_u8; 2]; 12];
        let mut count = 0_usize;
        let mut in_run = false;
        for column in 8..56 {
            let pixel = unsafe { image.get(row * image.width + column) };
            if !in_run && pixel != 0 {
                runs[count][0] = column as u8;
                in_run = true;
            } else if in_run && pixel == 0 {
                runs[count][1] = (column - 1) as u8;
                count += 1;
                if count > 10 { break; }
                in_run = false;
            }
        }
        if runs[0][0] == 0 || count == 0 { continue; }
        let mut vacant = 0_usize;
        for run in &runs[..count] {
            while vacant < 10 && unsafe { component_byte(components, vacant, 0) } != 0 { vacant += 1; }
            if vacant < 10 {
                unsafe {
                    set_component_byte(components, vacant, 3, run[0]);
                    set_component_byte(components, vacant, 4, run[1]);
                    set_component_byte(components, vacant, 0, ComponentState::New as u8);
                }
            }
        }
        'records: for index in 0..10 {
            let state = unsafe { component_byte(components, index, 0) };
            if state == ComponentState::New as u8 {
                unsafe { set_component_byte(components, index, 0, ComponentState::Seed as u8) };
                continue;
            }
            if state == ComponentState::Seed as u8 {
                let mut overlaps = 0_u8;
                if count > 1 {
                    let seed = unsafe { [component_byte(components, index, 3), component_byte(components, index, 4)] };
                    for run in &runs[..count] {
                        if !touching(seed, *run) { continue; }
                        unsafe {
                            if overlaps == 0 {
                                set_component_byte(components, index, 7, run[0]);
                                set_component_byte(components, index, 8, run[1]);
                                overlaps = 1;
                            } else {
                                if overlaps == 1 { set_component_byte(components, index, 9, run[0]); }
                                set_component_byte(components, index, 10, run[1]);
                                overlaps = overlaps.wrapping_add(1);
                                set_component_byte(components, index, 0, ComponentState::Fork as u8);
                                set_component_byte(components, index, 2, 1);
                                set_component_byte(components, index, 1, row as u8);
                            }
                        }
                    }
                }
                if overlaps <= 1 { unsafe { set_component_byte(components, index, 0, ComponentState::Vacant as u8) }; }
                continue;
            }
            if state != ComponentState::Fork as u8 { continue; }
            let mut overlaps = 0_u8;
            for run_index in 0..count {
                let length = unsafe { component_byte(components, index, 2) } as usize;
                let base = 4 * length;
                let inner = unsafe { [component_byte(components, index, base + 4), component_byte(components, index, base + 5)] };
                let run = runs[run_index];
                if xm1_pipeline_image_redeye_endpoints_within(inner[0] as i32, inner[1] as i32, run[0] as i32, run[1] as i32) != 0 {
                    unsafe { set_component_byte(components, index, 0, ComponentState::Accepted as u8) };
                } else if overlaps == 0 {
                    let left = unsafe { [component_byte(components, index, base + 3), component_byte(components, index, base + 4)] };
                    for candidate in &runs[run_index..count] {
                        if touching(left, *candidate) {
                            unsafe {
                                set_component_byte(components, index, base + 7, candidate[0]);
                                set_component_byte(components, index, base + 8, candidate[1]);
                            }
                            overlaps = 1;
                        }
                    }
                } else {
                    let right = unsafe { [component_byte(components, index, base + 5), component_byte(components, index, base + 6)] };
                    if touching(right, run) {
                        unsafe {
                            set_component_byte(components, index, base + 9, run[0]);
                            set_component_byte(components, index, base + 10, run[1]);
                            set_component_byte(components, index, 2, (length as u8).wrapping_add(1));
                        }
                        overlaps = overlaps.wrapping_add(1);
                    }
                }
                previous_length = length;
            }
            if overlaps > 1 || unsafe { component_byte(components, index, 0) } == ComponentState::Accepted as u8 { continue; }
            let base = previous_length * 4;
            for other in 0..10 {
                if other == index || unsafe { component_byte(components, other, 0) } != ComponentState::Fork as u8 { continue; }
                let other_base = unsafe { component_byte(components, other, 2) } as usize * 4;
                let merge = if overlaps == 1 {
                    unsafe {
                        component_byte(components, index, base + 5) == component_byte(components, other, other_base + 3)
                            && component_byte(components, index, base + 6) == component_byte(components, other, other_base + 4)
                    }
                } else {
                    touching(
                        unsafe { [component_byte(components, index, base + 3), component_byte(components, index, base + 4)] },
                        unsafe { [component_byte(components, other, other_base + 5), component_byte(components, other, other_base + 6)] },
                    )
                };
                if !merge { continue; }
                let side = if overlaps == 1 { 5 } else { 3 };
                for coordinate in 0..2 {
                    let value = unsafe { component_byte(components, other, other_base + side + coordinate) };
                    unsafe {
                        set_component_byte(components, index, base + side + coordinate, value);
                        set_component_byte(components, index, base + 4 + side + coordinate, value);
                    }
                }
                for coordinate in 0..4 {
                    let value = unsafe { component_byte(components, index, base + 3 + coordinate) };
                    unsafe { set_component_byte(components, index, base + 7 + coordinate, value) };
                }
                let opposite = if overlaps == 1 { 3 } else { 5 };
                for coordinate in 0..2 {
                    let value = unsafe { component_byte(components, index, base + opposite + coordinate) };
                    unsafe { set_component_byte(components, other, other_base + opposite + coordinate, value) };
                }
                let length = unsafe { component_byte(components, index, 2) };
                unsafe { set_component_byte(components, index, 2, length.wrapping_add(1)) };
                continue 'records;
            }
            unsafe { set_component_byte(components, index, 0, ComponentState::Vacant as u8) };
        }
    }
    unsafe { image.copy_to(work, image.length()) };
    for index in 0..10 {
        if unsafe { component_byte(components, index, 0) } != ComponentState::Accepted as u8 { continue; }
        let first_row = unsafe { component_byte(components, index, 1) } as i32;
        let length = unsafe { component_byte(components, index, 2) } as usize;
        for row in 0..length {
            let left = unsafe { component_byte(components, index, row * 4 + 8) };
            let right = unsafe { component_byte(components, index, row * 4 + 9) };
            for column in left..right {
                unsafe { work.put((first_row + row as i32) * image.width + column as i32, 255) };
            }
        }
    }
    unsafe { work.copy_to(image, image.length()) };
}

unsafe fn copy_run(image: MaskImage, work: MaskImage, source: i32, destination: i32, end: i32) -> i32 {
    let mut output = destination;
    for offset in source..end {
        unsafe { work.put(output, image.get(offset)) };
        output = output.wrapping_add(1);
    }
    output
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_retain_center_component(image: *const MaskImage, work: *const MaskImage) {
    let image = unsafe { read(image) };
    let work = unsafe { read(work) };
    let mut active = false;
    let mut center_found = false;
    let mut output_start = 0_i32;
    for _ in 0..10 {
        unsafe { work.clear() };
        let middle_row = image.height / 2;
        let mut source_start = middle_row.wrapping_mul(image.width);
        let mut last_column = 0_i32;
        let mut run_column = 0_i32;
        let mut output = source_start;
        let mut previous_source = source_start;
        let drift = unsafe { setting(3) };
        for row in middle_row..image.height {
            active = false;
            for column in 0..image.width {
                let offset = row.wrapping_mul(image.width).wrapping_add(column);
                if !active {
                    if unsafe { image.get(offset) } != 0 {
                        active = true;
                        output = offset;
                        run_column = column;
                        if row != middle_row { source_start = offset; } else { output_start = column; }
                    }
                } else if unsafe { image.get(offset) } == 0 {
                    if row == middle_row {
                        if output_start < work.width / 2 && work.width / 2 < column {
                            for x in output_start..column { unsafe { work.put(row * image.width + x, 255) }; }
                            output = output.wrapping_add(column - output_start);
                            center_found = true;
                        }
                    } else if output < offset && (output..offset).any(|position| unsafe { work.get(position.wrapping_sub(image.width)) } != 0) {
                        if drift.wrapping_add(run_column) < last_column && work.width / 2 + 2 < row && last_column < work.width / 2 {
                            source_start = previous_source.wrapping_add(work.height);
                            output = output.wrapping_add(last_column.wrapping_sub(run_column));
                        }
                        last_column = run_column;
                        previous_source = source_start;
                        if offset.wrapping_sub(source_start) > 3 && source_start < offset {
                            output = unsafe { copy_run(image, work, source_start, output, offset) };
                        }
                    }
                    active = false;
                }
            }
        }
        let mut last_column = 0_i32;
        for row in (1..middle_row).rev() {
            active = false;
            source_start = row * image.width;
            for column in 0..image.width {
                let offset = row * image.width + column;
                if !active {
                    if unsafe { image.get(offset) } != 0 {
                        active = true;
                        output = offset;
                        source_start = offset;
                        run_column = column;
                    }
                } else if unsafe { image.get(offset) } == 0 {
                    if output < offset && (output..offset).any(|position| unsafe { work.get(position.wrapping_add(image.width)) } != 0) {
                        if drift.wrapping_add(run_column) < last_column && work.width / 2 + 2 < row && last_column < work.width / 2 {
                            source_start = previous_source.wrapping_add(work.height);
                            output = output.wrapping_add(last_column.wrapping_sub(run_column));
                        }
                        if offset.wrapping_sub(source_start) > 3 && source_start < offset {
                            output = unsafe { copy_run(image, work, source_start, output, offset) };
                        }
                        previous_source = source_start;
                        last_column = run_column;
                        if row == work.height / 2 && run_column <= work.width / 2 && work.width / 2 <= column { center_found = true; }
                    }
                    active = false;
                }
            }
        }
        source_start = 0;
        output = 0;
        if image.height > 0 {
            for row in 0..image.height {
                for column in 0..image.width {
                    let offset = row * image.width + column;
                    if !active {
                        if unsafe { image.get(offset) } != 0 {
                            active = true;
                            output = offset;
                            source_start = offset;
                        }
                    } else if unsafe { image.get(offset) } == 0 {
                        if output < offset && offset.wrapping_sub(source_start) > 3
                            && (output..offset).any(|position| unsafe { work.get(position.wrapping_sub(image.width)) } != 0)
                            && source_start < offset {
                            output = unsafe { copy_run(image, work, source_start, output, offset) };
                        }
                        active = false;
                    }
                }
            }
            for row in (1..=image.height).rev() {
                output = row * image.width;
                source_start = output;
                for column in 0..image.width {
                    let offset = row * image.width + column;
                    if !active {
                        if unsafe { image.get(offset) } != 0 {
                            active = true;
                            output = offset;
                            source_start = offset;
                        }
                    } else if unsafe { image.get(offset) } == 0 {
                        if output < offset && offset.wrapping_sub(source_start) > 3
                            && (output..offset).any(|position| unsafe { work.get(position.wrapping_add(image.width)) } != 0)
                            && source_start < offset {
                            output = unsafe { copy_run(image, work, source_start, output, offset) };
                        }
                        active = false;
                    }
                }
            }
        }
        if center_found { break; }
        for offset in 0..work.height.wrapping_mul(work.width) {
            if unsafe { work.get(offset) } != 0 { unsafe { image.put(offset, 0) }; }
        }
    }
    unsafe { work.copy_to(image, image.length()) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_build_masks(mask: *const MaskImage, color: *const MaskImage, basis: *const MaskImage, work: *const MaskImage, _temporary: *const MaskImage, histogram: *mut i32, components: *mut Components) {
    let destination = unsafe { read(mask) };
    let color = unsafe { read(color) };
    unsafe { write_bytes(histogram, 0, 256) };
    let area = destination.height.wrapping_mul(destination.width);
    for row in 0..destination.height {
        for column in 0..destination.width {
            let value = unsafe { color.get(row.wrapping_mul(destination.width).wrapping_add(column)) };
            let bin = unsafe { histogram.add(value as usize) };
            unsafe { write(bin, read(bin).wrapping_add(1)) };
        }
    }
    let quota = area.wrapping_mul(20) / 100;
    let mut total = 0_i32;
    let mut threshold = 0_u8;
    for value in (0..256).rev() {
        total = total.wrapping_add(unsafe { read(histogram.add(value)) });
        if total > quota { threshold = value as u8; break; }
    }
    let center_x = destination.width.wrapping_sub(1) / 2;
    let center_y = destination.height.wrapping_sub(1) / 2;
    for row in 0..destination.height {
        for column in 0..destination.width {
            let offset = row.wrapping_mul(destination.width).wrapping_add(column);
            let radius = radius_squared(column, row, center_x, center_y);
            let value = if radius <= 225 || unsafe { color.get(offset) } >= threshold { 255 } else { 0 };
            unsafe { destination.put(offset, value) };
        }
    }
    if unsafe { setting(8) } != 0 {
        let basis_image = unsafe { read(basis) };
        for row in 0..destination.height {
            for column in 0..destination.width {
                let offset = row.wrapping_mul(destination.width).wrapping_add(column);
                unsafe { basis_image.put(offset, if color.get(offset) >= threshold { 255 } else { 0 }) };
            }
        }
        let inner = unsafe { setting(9) };
        let outer = unsafe { setting(0).wrapping_add(setting(2) << 8) };
        for row in 0..destination.height {
            for column in 0..destination.width {
                let offset = row.wrapping_mul(destination.width).wrapping_add(column);
                let radius = radius_squared(column, row, center_x, center_y);
                let keep = (unsafe { color.get(offset) } >= threshold || radius <= inner) && radius <= outer;
                unsafe { basis_image.put(offset, if keep { 255 } else { 0 }) };
            }
        }
        unsafe {
            xm1_pipeline_image_redeye_fill_mask_rows(basis, work);
            xm1_pipeline_image_redeye_fill_mask_columns(basis, work);
            xm1_pipeline_image_redeye_fill_fork_gaps(basis, work, components);
            xm1_pipeline_image_redeye_retain_center_component(basis, work);
            xm1_pipeline_image_redeye_dilate_cross(basis, work);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_refine_mask(mask: *const MaskImage, color: *const MaskImage, temporary: *const MaskImage, work: *const MaskImage, basis: *const MaskImage, histogram: *mut i32, components: *mut Components) {
    unsafe { xm1_pipeline_image_redeye_build_masks(mask, color, basis, work, temporary, histogram, components) };
    for (operation, count) in [(Morphology::Erode, 2), (Morphology::Dilate, 5), (Morphology::Erode, 3)] {
        for _ in 0..count {
            unsafe {
                morphology(read(mask), read(work), operation, Kernel::Cross);
                morphology(read(mask), read(work), operation, Kernel::Square);
            }
        }
    }
    let mask_image = unsafe { read(mask) };
    let temporary_image = unsafe { read(temporary) };
    let mut iterations = 0_i32;
    loop {
        unsafe {
            mask_image.copy_to(temporary_image, mask_image.length());
            xm1_pipeline_image_redeye_erode_cross(temporary, work);
            xm1_pipeline_image_redeye_erode_square(temporary, work);
        }
        if unsafe { xm1_pipeline_image_redeye_count_mask_pixels(temporary) } <= 0 { break; }
        iterations = iterations.wrapping_add(1);
        unsafe { temporary_image.copy_to(mask_image, mask_image.length()) };
    }
    for _ in 0..iterations.wrapping_add(3) {
        unsafe {
            xm1_pipeline_image_redeye_dilate_cross(mask, work);
            xm1_pipeline_image_redeye_dilate_square(mask, work);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_prepare_edit_mask(image: *const MaskImage, edit: *mut Edit, distance_squared: i32, context: *mut u8) {
    let scale = unsafe { read(core::ptr::addr_of!((*edit).sampling_scale) as *const i16) } as i32;
    let sample = context as *mut MaskImage;
    unsafe {
        xm1_pipeline_image_redeye_dispatch_sample(image, 0, scale, (*edit).center_x_q4, (*edit).center_y_q4, (*sample).width, (*sample).height, context as *mut u32);
    }
    let color = unsafe { context.add(12) } as *const MaskImage;
    let temporary = unsafe { context.add(28) } as *const MaskImage;
    let work = unsafe { context.add(44) } as *const MaskImage;
    let basis = unsafe { context.add(60) } as *const MaskImage;
    let histogram = unsafe { context.add(76) } as *mut i32;
    let components = unsafe { context.add(0x84c) } as *mut Components;
    let mask = unsafe { core::ptr::addr_of!((*edit).mask) };
    unsafe {
        xm1_pipeline_image_redeye_chroma_score(sample, color);
        xm1_pipeline_image_redeye_refine_mask(mask, color, temporary, work, basis, histogram, components);
    }
    let scaled = (distance_squared.wrapping_mul(scale) / 1024).wrapping_mul(scale) / 1024;
    if scaled > 0 {
        unsafe {
            xm1_pipeline_image_redeye_filter_mask(mask, color, basis, scaled);
            xm1_pipeline_image_redeye_mask_gains(sample, edit, histogram, context.add(0x44c) as *mut i32);
        }
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct SamplingScale(i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct CoordinateQ4(i32);
#[derive(Clone, Copy)]
struct SampleCenter { x: CoordinateQ4, y: CoordinateQ4 }
#[derive(Clone, Copy)]
enum SamplingMap { Scaled, Rotated { sine: i32, cosine: i32 } }

unsafe fn sample_coordinate(column: i32, row: i32, width: i32, height: i32, scale: SamplingScale, center: SampleCenter, mapping: SamplingMap) -> (i32, i32) {
    let x = column.wrapping_mul(16).wrapping_sub(width.wrapping_sub(1).wrapping_mul(16) >> 1);
    let y = row.wrapping_mul(16).wrapping_sub(height.wrapping_sub(1).wrapping_mul(16) >> 1);
    let (x, y) = match mapping {
        SamplingMap::Scaled => (x, y),
        SamplingMap::Rotated { sine, cosine } => (
            x.wrapping_mul(cosine).wrapping_sub(y.wrapping_mul(sine)) / 4096,
            x.wrapping_mul(sine).wrapping_add(y.wrapping_mul(cosine)) / 4096,
        ),
    };
    unsafe {
        (divide(x.wrapping_mul(1024), scale.0).wrapping_add(center.x.0),
            divide(y.wrapping_mul(1024), scale.0).wrapping_add(center.y.0))
    }
}

unsafe fn interpolated_channel(image: MaskImage, x_q4: i32, y_q4: i32, channel: usize) -> u8 {
    if x_q4 < 0 || y_q4 < 0
        || x_q4 >= image.width.wrapping_sub(1).wrapping_mul(16)
        || y_q4 >= image.height.wrapping_sub(1).wrapping_mul(16) {
        return if channel == 0 { 0 } else { 128 };
    }
    let y = y_q4 / 16;
    let fy = (y_q4.wrapping_mul(1024) / 16).wrapping_sub(y.wrapping_mul(1024));
    let (x, fx) = if channel == 0 {
        let x = x_q4 / 16;
        (x, (x_q4.wrapping_mul(1024) / 16).wrapping_sub(x.wrapping_mul(1024)))
    } else {
        let x = x_q4 / 32;
        (x, (x_q4.wrapping_mul(1024) / 16).wrapping_sub(x.wrapping_mul(2048)) / 2)
    };
    let mut horizontal = [0_i32; 2];
    for dy in 0..2 {
        let mut values = [0_i32; 2];
        for dx in 0..2 {
            let source_x = x.wrapping_add(dx as i32);
            let pair = y.wrapping_add(dy as i32).wrapping_mul(image.width >> 1)
                .wrapping_add(if channel == 0 { source_x >> 1 } else { source_x });
            let offset = pair.wrapping_mul(4).wrapping_add(if channel == 0 { source_x & 1 } else { channel as i32 + 1 });
            values[dx] = unsafe { image.get(offset) } as i32;
        }
        horizontal[dy] = (1024_i32.wrapping_sub(fx)).wrapping_mul(values[0]).wrapping_add(fx.wrapping_mul(values[1]));
    }
    ((1024_i32.wrapping_sub(fy)).wrapping_mul(horizontal[0]).wrapping_add(fy.wrapping_mul(horizontal[1])) / 1048576) as u8
}

unsafe fn sample_yuv(image: MaskImage, scale: SamplingScale, center: SampleCenter, width: i32, height: i32, output: *mut u8, mapping: SamplingMap) {
    let mut destination = output;
    for row in 0..height {
        let mut column = 0_i32;
        while column < width {
            let coordinates = [
                unsafe { sample_coordinate(column, row, width, height, scale, center, mapping) },
                unsafe { sample_coordinate(column.wrapping_add(1), row, width, height, scale, center, mapping) },
            ];
            for (pixel, &(x, y)) in coordinates.iter().enumerate() {
                unsafe { write(destination.add(pixel), interpolated_channel(image, x, y, 0)) };
            }
            let (x, y) = coordinates[0];
            unsafe {
                write(destination.add(2), interpolated_channel(image, x, y, 1));
                write(destination.add(3), interpolated_channel(image, x, y, 2));
                destination = destination.add(4);
            }
            column = column.wrapping_add(2);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_sample_scaled(pixels: PixelAddress, width: i32, height: i32, scale: i32, center_x_q4: i32, center_y_q4: i32, output_width: u32, output_height: i32, output: *mut u8) {
    if output_width & 1 != 0 { return; }
    let center = SampleCenter { x: CoordinateQ4(center_x_q4), y: CoordinateQ4(center_y_q4) };
    unsafe { sample_yuv(MaskImage { pixels, width, height }, SamplingScale(scale), center, output_width as i32, output_height, output, SamplingMap::Scaled) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_sample_rotated(pixels: PixelAddress, width: i32, height: i32, angle: i32, scale: i16, center_x_q4: i32, center_y_q4: i32, output_width: u32, output_height: i32, output: *mut u8) {
    if output_width & 1 != 0 || output_height <= 0 || output_width as i32 <= 0 { return; }
    let center = SampleCenter { x: CoordinateQ4(center_x_q4), y: CoordinateQ4(center_y_q4) };
    let mapping = unsafe { SamplingMap::Rotated { sine: xm1_pipeline_image_redeye_sine(angle), cosine: xm1_pipeline_image_redeye_cosine(angle) } };
    unsafe { sample_yuv(MaskImage { pixels, width, height }, SamplingScale(scale as i32), center, output_width as i32, output_height, output, mapping) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_dispatch_sample(image: *const MaskImage, operation: i32, scale: i32, center_x_q4: i32, center_y_q4: i32, width: i32, height: i32, configuration: *const u32) {
    if configuration.is_null() { return; }
    let image = unsafe { read(image) };
    let output = unsafe { read(configuration) } as *mut u8;
    if operation == 0 {
        unsafe { xm1_pipeline_image_redeye_sample_scaled(image.pixels, image.width, image.height, scale, center_x_q4, center_y_q4, width as u32, height, output) };
    } else {
        unsafe { xm1_pipeline_image_redeye_sample_rotated(image.pixels, image.width, image.height, operation, scale as i16, center_x_q4, center_y_q4, width as u32, height, output) };
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct AngleDegrees(i32);

fn normalize_angle(angle: AngleDegrees) -> i16 {
    let mut value = angle.0;
    if value > 179 {
        loop {
            value = value.wrapping_sub(360) as u16 as i32;
            if value as i16 <= 179 { break; }
        }
    } else if value < -180 {
        loop {
            value = value.wrapping_add(360) as u16 as i32;
            if value as i16 >= -180 { break; }
        }
    }
    value as i16
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_sine(angle: i32) -> i32 {
    let index = normalize_angle(AngleDegrees(angle)) / 15;
    unsafe { read_volatile((0x010873c4_u32.wrapping_add((index as i32 * 2) as u32)) as *const i16) as i32 }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_redeye_cosine(angle: i32) -> i32 {
    let index = normalize_angle(AngleDegrees(angle)) / 15;
    unsafe { read_volatile((0x01086d84_u32.wrapping_add((index as i32 * 2) as u32)) as *const i16) as i32 }
}

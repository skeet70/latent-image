//! Sample-grid scanline fill, UI compositing and declared YYCbCr conversion.
//!
//! Fill behavior follows `reference_raster` in
//! `tests/test_emulator_graphics_vectorized.py`.
//! Declared conversion uses stock CPU-fallback decimal coefficients, complete
//! signed-numerator division toward zero and `[255, B, G, R]` byte order.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyByteArray, PyBytes, PyDict, PyList, PyTuple};

#[derive(Clone, Copy)]
struct DramOffset(usize);
#[derive(Clone, Copy)]
struct PixelWidth(usize);
#[derive(Clone, Copy)]
struct PixelHeight(usize);
#[derive(Clone, Copy)]
struct PixelStride(usize);

#[pyclass(frozen, module = "x70_native._native")]
pub(crate) struct DeclaredUIConversionPlan {
    source: DramOffset,
    destination: DramOffset,
    width: PixelWidth,
    height: PixelHeight,
    stride: PixelStride,
    source_bytes: usize,
    destination_bytes: usize,
}

impl DeclaredUIConversionPlan {
    pub(crate) fn prepare(
        source: u64,
        destination: u64,
        width: u64,
        height: u64,
        stride: u64,
        ram_size: usize,
    ) -> Result<Self, String> {
        if !source.is_multiple_of(16) || !destination.is_multiple_of(16) {
            return Err("Declared UI conversion requires 16-aligned physical DRAM offsets".into());
        }
        if width == 0 || width >= 1 << 13 || !width.is_multiple_of(16)
            || height == 0 || height >= 1 << 13 || !height.is_multiple_of(8)
            || stride >= 1 << 15 || !stride.is_multiple_of(16) || stride < width
        {
            return Err("Invalid declared UI conversion width, height or pixel stride".into());
        }
        let source_bytes = stride.checked_mul(2)
            .and_then(|pitch| pitch.checked_mul(height - 1))
            .and_then(|rows| width.checked_mul(2).and_then(|last| rows.checked_add(last)))
            .ok_or("Declared UI source extent overflow")?;
        let destination_bytes = width.checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .filter(|&bytes| bytes <= 64 * 1024 * 1024)
            .ok_or("Declared UI output exceeds its 64MiB bound")?;
        let source_end = source.checked_add(source_bytes)
            .ok_or("Declared UI source physical extent overflow")?;
        let destination_end = destination.checked_add(destination_bytes)
            .ok_or("Declared UI destination physical extent overflow")?;
        if source_end > ram_size as u64 || destination_end > ram_size as u64 {
            return Err("Declared UI conversion extents must lie in physical DRAM".into());
        }
        if source < destination_end && destination < source_end {
            return Err("Declared UI conversion source and destination must be disjoint".into());
        }
        Ok(Self {
            source: DramOffset(source as usize),
            destination: DramOffset(destination as usize),
            width: PixelWidth(width as usize),
            height: PixelHeight(height as usize),
            stride: PixelStride(stride as usize),
            source_bytes: source_bytes as usize,
            destination_bytes: destination_bytes as usize,
        })
    }

    pub(crate) fn validate_bounds(&self, ram_size: usize) -> Result<(), String> {
        if self.source.0.checked_add(self.source_bytes).is_none_or(|end| end > ram_size)
            || self.destination.0.checked_add(self.destination_bytes).is_none_or(|end| end > ram_size)
        {
            return Err("Declared UI conversion plan exceeds the target Machine DRAM".into());
        }
        Ok(())
    }

    pub(crate) fn source_range(&self) -> std::ops::Range<usize> {
        self.source.0..self.source.0 + self.source_bytes
    }

    pub(crate) fn destination_range(&self) -> std::ops::Range<usize> {
        self.destination.0..self.destination.0 + self.destination_bytes
    }

    pub(crate) fn source_rows(&self) -> impl Iterator<Item = std::ops::Range<usize>> + '_ {
        let pitch = 2 * self.stride.0;
        let bytes = 2 * self.width.0;
        (0..self.height.0).map(move |row| {
            let start = self.source.0 + pitch * row;
            start..start + bytes
        })
    }

    pub(crate) fn convert(&self, source: &[u8], destination: &mut [u8]) {
        let source_pitch = 2 * self.stride.0;
        let source_width = 2 * self.width.0;
        let destination_pitch = 4 * self.width.0;
        // Every signed channel numerator lies in -22_681_600..=48_004_400.
        for (row, output) in destination.chunks_exact_mut(destination_pitch).enumerate() {
            let start = row * source_pitch;
            for (pair, pixels) in source[start..start + source_width].chunks_exact(4)
                .zip(output.chunks_exact_mut(8))
            {
                let u = i32::from(pair[2]) - 128;
                let v = i32::from(pair[3]) - 128;
                let blue = 177_200 * u;
                let green = -34_414 * u - 71_414 * v;
                let red = 140_200 * v;
                for (luma, pixel) in [pair[0], pair[1]].into_iter().zip(pixels.chunks_exact_mut(4)) {
                    let y = 100_000 * i32::from(luma);
                    pixel[0] = 255;
                    pixel[1] = ((y + blue) / 100_000).clamp(0, 255) as u8;
                    pixel[2] = ((y + green) / 100_000).clamp(0, 255) as u8;
                    pixel[3] = ((y + red) / 100_000).clamp(0, 255) as u8;
                }
            }
        }
    }
}

#[pymethods]
impl DeclaredUIConversionPlan {
    #[getter]
    fn source_physical(&self) -> usize {
        self.source.0
    }

    #[getter]
    fn destination_physical(&self) -> usize {
        self.destination.0
    }

    #[getter]
    fn width(&self) -> usize {
        self.width.0
    }

    #[getter]
    fn height(&self) -> usize {
        self.height.0
    }

    #[getter]
    fn stride_pixels(&self) -> usize {
        self.stride.0
    }


    #[getter]
    fn destination_bytes(&self) -> usize {
        self.destination_bytes
    }
}

/// Inside-sample counts (u8, row-major `height` x `width` pixels).
///
/// `edges` is flat `[ax, ay, bx, by, ...]` of non-horizontal emitted fill
/// edges. Pixel `(0, 0)` is at `(x0, y0)`. Raises ValueError when a sample
/// row's crossings leave an unbalanced winding (nonzero) or parity.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn ui_fill_counts<'py>(
    py: Python<'py>,
    edges: Vec<f64>,
    nonzero: bool,
    x0: i64,
    y0: i64,
    scale: i64,
    width: usize,
    height: usize,
) -> PyResult<Bound<'py, PyBytes>> {
    if edges.len() % 4 != 0 || scale < 1 || scale > 15 {
        return Err(PyValueError::new_err("Invalid fill edge list or sample scale"));
    }
    let s = scale as usize;
    let (sample_width, sample_height) = (width * s, height * s);
    let (x0f, y0f, scalef) = (x0 as f64, y0 as f64, scale as f64);
    let mut counts = vec![0u8; width * height];
    let mut crossings: Vec<(f64, i64)> = Vec::with_capacity(edges.len() / 4);
    let inside = |level: i64| if nonzero { level != 0 } else { level & 1 != 0 };
    for row in 0..sample_height {
        let y = y0f + (row as f64 + 0.5) / scalef;
        crossings.clear();
        for edge in edges.chunks_exact(4) {
            let (ax, ay, bx, by) = (edge[0], edge[1], edge[2], edge[3]);
            if ay.min(by) <= y && y < ay.max(by) {
                crossings.push((
                    ax + (y - ay) * (bx - ax) / (by - ay),
                    if by > ay { 1 } else { -1 },
                ));
            }
        }
        crossings.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)));
        let (mut level, mut previous) = (0i64, None::<f64>);
        let pixel_row = (row / s) * width;
        for &(x, direction) in &crossings {
            if let Some(previous) = previous
                && inside(level)
            {
                let left = ((previous - x0f) * scalef - 0.5).ceil().max(0.0);
                let right = ((x - x0f) * scalef - 0.5).ceil().min(sample_width as f64);
                if left < right {
                    let (left, right) = (left as usize, right as usize);
                    let mut column = left;
                    while column < right {
                        let pixel = column / s;
                        let end = right.min((pixel + 1) * s);
                        counts[pixel_row + pixel] += (end - column) as u8;
                        column = end;
                    }
                }
            }
            level = if nonzero { level + direction } else { level + 1 };
            previous = Some(x);
        }
        if inside(level) {
            return Err(PyValueError::new_err(
                "Graphics UI emitted fill edges have an unbalanced scanline",
            ));
        }
    }
    Ok(PyBytes::new(py, &counts))
}

/// Composite one draw over its target region, the `_ui_render` blend loop.
///
/// `region` is the region's RGBA bytes (row-major, `coverage.len()` pixels),
/// `coverage` its per-pixel f64 coverage, `source` either one RGBA f64 color
/// or one per pixel, all in 0..1. Every value is computed with the f64
/// operations, in the order, of the NumPy expression it replaces; pixels with
/// zero coverage (or zero alpha when blending) keep all their bytes.
#[pyfunction]
pub fn ui_composite<'py>(
    py: Python<'py>,
    region: &[u8],
    coverage: &[u8],
    source: &[u8],
    blend: bool,
) -> PyResult<Bound<'py, PyBytes>> {
    let pixels = coverage.len() / 8;
    let per_pixel = source.len() == pixels * 32;
    if coverage.len() % 8 != 0 || region.len() != pixels * 4 || !(per_pixel || source.len() == 32) {
        return Err(PyValueError::new_err("Invalid composite region/coverage/source sizes"));
    }
    let mut out = region.to_vec();
    for i in 0..pixels {
        let pixel: &mut [u8; 4] = (&mut out[i * 4..i * 4 + 4]).try_into().unwrap();
        blend_pixel(pixel, f64_at(coverage, i), source_at(source, per_pixel, i), blend);
    }
    Ok(PyBytes::new(py, &out))
}

/// `ui_composite` applied in place to a target kept in its tiled memory
/// layout (8x8 Morton tiles, `_ui_morton`): the region is the `width` x
/// `height` rectangle at (x0, y0) of a `target_width`-wide target, and each
/// pixel's RGBA is bytes (3,2,1,0) of its word for order 0, (2,1,0,3)
/// otherwise, exactly as `_ui_render` detiles and retiles it.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn ui_composite_tiled(
    target: &Bound<'_, PyByteArray>,
    target_width: usize,
    x0: usize,
    y0: usize,
    width: usize,
    height: usize,
    order: u32,
    coverage: &[u8],
    source: &[u8],
    blend: bool,
) -> PyResult<()> {
    let pixels = width * height;
    let per_pixel = source.len() == pixels * 32;
    let target_height = target.len() / 4 / target_width.max(1);
    if coverage.len() != pixels * 8
        || !(per_pixel || source.len() == 32)
        || target_width % 8 != 0
        || target.len() != target_width * target_height * 4
        || x0 + width > target_width
        || y0 + height > target_height
    {
        return Err(PyValueError::new_err("Invalid tiled composite geometry/coverage/source sizes"));
    }
    let channels: [usize; 4] = if order == 0 { [3, 2, 1, 0] } else { [2, 1, 0, 3] };
    // SAFETY: no Python code runs while this exclusive view is alive.
    let bytes = unsafe { target.as_bytes_mut() };
    for row in 0..height {
        for column in 0..width {
            let i = row * width + column;
            let (x, y) = (x0 + column, y0 + row);
            let local = (x & 1) | (y & 1) << 1 | (x & 2) << 1 | (y & 2) << 2 | (x & 4) << 2 | (y & 4) << 3;
            let word = (((y >> 3) * (target_width >> 3) + (x >> 3)) * 64 + local) * 4;
            let mut pixel = [0u8; 4];
            for (channel, &byte) in channels.iter().enumerate() {
                pixel[channel] = bytes[word + byte];
            }
            blend_pixel(&mut pixel, f64_at(coverage, i), source_at(source, per_pixel, i), blend);
            for (channel, &byte) in channels.iter().enumerate() {
                bytes[word + byte] = pixel[channel];
            }
        }
    }
    Ok(())
}

fn f64_at(bytes: &[u8], index: usize) -> f64 {
    f64::from_ne_bytes(bytes[index * 8..index * 8 + 8].try_into().unwrap())
}

fn source_at(source: &[u8], per_pixel: bool, pixel: usize) -> [f64; 4] {
    let s = if per_pixel { pixel * 4 } else { 0 };
    [f64_at(source, s), f64_at(source, s + 1), f64_at(source, s + 2), f64_at(source, s + 3)]
}

/// One pixel of the reference blend (see `ui_composite`), in place.
fn blend_pixel(pixel: &mut [u8; 4], c: f64, [sr, sg, sb, sa]: [f64; 4], blend: bool) {
    let alpha = sa * c;
    if c == 0.0 || (blend && alpha == 0.0) {
        return;
    }
    let (dr, dg, db, da) = (
        pixel[0] as f64 / 255.0,
        pixel[1] as f64 / 255.0,
        pixel[2] as f64 / 255.0,
        pixel[3] as f64 / 255.0,
    );
    let retained = if blend { 1.0 - alpha } else { 1.0 - c };
    let out_alpha = alpha + da * retained;
    let channel = |src: f64, dst: f64| {
        let numerator = src * alpha + dst * da * retained;
        if out_alpha != 0.0 { numerator / out_alpha } else { 0.0 }
    };
    let byte = |value: f64| (value.max(0.0).min(1.0) * 255.0 + 0.5).floor() as u8;
    *pixel = [byte(channel(sr, dr)), byte(channel(sg, dg)), byte(channel(sb, db)), byte(out_alpha)];
}

/// The graphics UI command-list parser, `emulator.graphics_devices._ui_stream_reference`
/// (the behavior and every error message), for one path or fill list.
///
/// `initial` is the bank's register dict. Returns `(draws, registers)` with
/// Python dicts in the reference's insertion order; a rejection raises
/// ValueError carrying the reference's UnsupportedDevice message.
#[pyfunction]
pub fn ui_stream_parse<'py>(
    py: Python<'py>,
    words: Vec<u32>,
    path: bool,
    initial: Option<&Bound<'py, PyDict>>,
) -> PyResult<(Bound<'py, PyList>, Bound<'py, PyDict>)> {
    let bank_name = if path { "path" } else { "fill" };
    let fail = |message: String| Err(PyValueError::new_err(message));
    if words.is_empty() || words.len() % 16 != 0 {
        return fail("Graphics UI requires complete16-word-aligned lists".into());
    }
    let mut registers = Registers { dict: PyDict::new(py), values: Default::default() };
    if let Some(initial) = initial {
        registers.dict = initial.copy()?;
        for (key, value) in initial.iter() {
            registers.values.insert(key.extract::<u64>()?, value.extract::<u64>()?);
        }
    }
    let draws = PyList::empty(py);
    let mut geometry: Vec<(u32, u32, Vec<f64>)> = Vec::new();
    let mut geometry_state: Option<Vec<Option<u64>>> = None;
    let (mut position, mut previous, mut pending) = (0usize, None::<u64>, None::<u64>);
    const STATE_KEYS: [u64; 12] = [0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x60, 0x69, 0x6a];
    while position < words.len() {
        let header = words[position];
        if header == 0 {
            if words.len() - position > 15 || words[position..].iter().any(|&w| w != 0) {
                return fail("Graphics UI has interior/excessive zero padding".into());
            }
            break;
        }
        position += 1;
        if header & 0xfff00000 == 0x40000000 {
            let (index, lanes) = ((header & 0xffff) as u64, header >> 16 & 15);
            if lanes == 0 || !ui_known(path, index) || position == words.len() {
                return fail(format!("Graphics UI unknown/incomplete {bank_name} register {header:#x}"));
            }
            let value = words[position] as u64;
            position += 1;
            let mask: u64 = (0..4).filter(|lane| lanes >> lane & 1 != 0).map(|lane| 255u64 << (8 * lane)).sum();
            registers.set(py, index, registers.get(index).unwrap_or(0) & !mask | value & mask)?;
            if !path && (index == 0x16b || index == 0x16c) {
                if registers.get(0x140).unwrap_or(0) & 0x400 != 0 {
                    if index == 0x16b {
                        let selector = registers.get(index).unwrap();
                        if lanes != 15 || selector & !0x1007f != 0 {
                            return fail("Graphics UI gradient ramp selector is unsupported".into());
                        }
                        registers.set(py, UI_RAMP_CURSOR, selector)?;
                    } else {
                        let cursor = registers.get(UI_RAMP_CURSOR);
                        if cursor.is_none_or(|c| c & !0x1007f != 0) || ![3, 12, 15].contains(&lanes) {
                            return fail("Graphics UI gradient ramp upload has no bounded index/lanes".into());
                        }
                        let cursor = cursor.unwrap();
                        let ramp_index = (cursor >> 16) * 128 + (cursor & 127);
                        let slot = UI_RAMP_VALUES + ramp_index;
                        registers.set(py, slot, registers.get(slot).unwrap_or(0) & !mask | value & mask)?;
                        let lane_slot = UI_RAMP_LANES + ramp_index;
                        registers.set(py, lane_slot, registers.get(lane_slot).unwrap_or(0) | lanes as u64)?;
                        registers.set(py, UI_RAMP_CURSOR, cursor + 1)?;
                    }
                } else if index == 0x16c && registers.get(index) != Some(0x40001) {
                    return fail("Graphics UI non-gradient ramp/filter write is unsupported".into());
                }
            }
            let marker = match (path, index) {
                (true, 0x10 | 0x20) | (false, 0x18 | 0x28) => Some(0x12345678),
                (true, 0x30) | (false, 0x31) => Some(0xf0),
                _ => None,
            };
            if marker.is_some_and(|m| registers.get(index) != Some(m)) {
                return fail("Graphics UI unknown stream flush marker".into());
            }
            if (path && index == 0x8d) || (!path && index == 0xad) {
                let mut sequence = registers.get(index).unwrap();
                if sequence >> 16 != 1 || sequence & 0xffff == 0 {
                    return fail("Graphics UI unknown synchronization word".into());
                }
                sequence &= 0xffff;
                if let Some(previous) = previous
                    && sequence != if previous == 0xffff { 1 } else { previous + 1 }
                {
                    return fail("Graphics UI nonconsecutive draw fences".into());
                }
                previous = Some(sequence);
                if path {
                    if geometry.is_empty() {
                        return fail("Graphics UI geometry-free draw needs the exact initializer profile".into());
                    }
                    if let Some(state) = &geometry_state
                        && STATE_KEYS.iter().zip(state).any(|(&key, &value)| registers.get(key) != value)
                    {
                        return fail("Graphics UI path transform/style changed during geometry".into());
                    }
                    let draw = PyDict::new(py);
                    draw.set_item("sequence", sequence)?;
                    draw.set_item("registers", registers.dict.copy()?)?;
                    let records = PyList::empty(py);
                    for (code, flags, values) in geometry.drain(..) {
                        records.append((code, flags, PyTuple::new(py, values)?))?;
                    }
                    draw.set_item("geometry", records)?;
                    draws.append(draw)?;
                    geometry_state = None;
                } else {
                    if pending.is_some() {
                        return fail("Graphics UI fill fence is missing its draw".into());
                    }
                    pending = Some(sequence);
                }
            } else if !path && index == 0x1a9 {
                if pending.is_none() || registers.get(index) != Some(3) {
                    return fail("Graphics UI draw requires one preceding fill fence and control3".into());
                }
                let draw = PyDict::new(py);
                draw.set_item("sequence", pending.take().unwrap())?;
                draw.set_item("registers", registers.dict.copy()?)?;
                draws.append(draw)?;
            }
        } else if path && ui_geometry_words(header >> 24).is_some() {
            let (code, flags) = (header >> 24, header >> 16 & 255);
            let count = ui_geometry_words(code).unwrap();
            if header & 0xffff != 0 || flags & !0x3f != 0 || position + count > words.len() {
                return fail("Graphics UI unknown/incomplete geometry flags or operands".into());
            }
            let values: Vec<f64> = words[position..position + count]
                .iter()
                .map(|&w| f32::from_bits(w) as f64)
                .collect();
            if values.iter().any(|v| !v.is_finite() || v.abs() > 16777216.0) {
                return fail("Graphics UI geometry is nonfinite or outside the declared bound".into());
            }
            if geometry_state.is_none() {
                geometry_state = Some(STATE_KEYS.iter().map(|&key| registers.get(key)).collect());
            }
            geometry.push((code, flags, values));
            position += count;
        } else {
            return fail(format!("Graphics UI unknown {bank_name} command {header:#x}"));
        }
    }
    if !geometry.is_empty() || pending.is_some() {
        return fail("Graphics UI list ends before its draw fence".into());
    }
    if !path
        && words[..position][position.saturating_sub(6)..]
            != [0x400f0028, 0x12345678, 0x40010031, 0xf0, 0x400f0018, 0x12345678]
    {
        return fail("Graphics UI requires the complete B90600 fill footer".into());
    }
    Ok((draws, registers.dict.copy()?))
}

const UI_RAMP_VALUES: u64 = 0x10000;
const UI_RAMP_LANES: u64 = 0x10100;
const UI_RAMP_CURSOR: u64 = 0x10200;

fn ui_geometry_words(code: u32) -> Option<usize> {
    match code {
        0x28..=0x2c => Some(2),
        0x30 => Some(4),
        0x31 => Some(6),
        0x32 | 0x33 => Some(8),
        _ => None,
    }
}

fn ui_known(path: bool, index: u64) -> bool {
    if path {
        matches!(index, 0x10 | 0x20 | 0x30 | 0x60 | 0x69 | 0x6a | 0x7b | 0x7c | 0x80 | 0x81 | 0x8b | 0x8c
            | 0x8d | 0x8e | 0x40..=0x48 | 0x61..=0x68 | 0x84..=0x88)
    } else {
        matches!(index, 0x18 | 0x28 | 0x31 | 0xa0 | 0xa2 | 0xad | 0x100 | 0x115 | 0x117 | 0x118 | 0x11b
            | 0x140 | 0x143 | 0x144 | 0x147 | 0x14d | 0x14e | 0x152..=0x157 | 0x16c | 0x168 | 0x16b
            | 0x16d | 0x178 | 0x184 | 0x185 | 0x187 | 0x188 | 0x190 | 0x1c0 | 0xa4..=0xa8 | 0xc0..=0xdf
            | 0x106..=0x114 | 0x180..=0x183 | 0x1a0..=0x1a5 | 0x1a7 | 0x1a9 | 0x1aa)
    }
}

/// A register bank: values for the parser's checks, mirrored in a Python dict
/// (insertion order as the reference's) that is copied for each draw.
struct Registers<'py> {
    dict: Bound<'py, PyDict>,
    values: std::collections::HashMap<u64, u64>,
}
impl<'py> Registers<'py> {
    fn get(&self, key: u64) -> Option<u64> {
        self.values.get(&key).copied()
    }
    fn set(&mut self, _py: Python<'py>, key: u64, value: u64) -> PyResult<()> {
        self.values.insert(key, value);
        self.dict.set_item(key, value)
    }
}

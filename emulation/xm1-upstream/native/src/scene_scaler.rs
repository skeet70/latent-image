use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyByteArray, PyBytes};

use crate::raster::DeclaredRasterPlan;

#[derive(Clone, Copy)]
struct DramOffset(usize);
#[derive(Clone, Copy)]
struct PixelStride(usize);
#[derive(Clone, Copy)]
struct PixelExtent { width: usize, height: usize }
#[derive(Clone, Copy)]
struct PixelCenter { x: i64, y: i64 }
#[derive(Clone, Copy)]
struct Q16Scale(i64);
#[derive(Clone, Copy)]
struct Raster {
    address: DramOffset,
    stride: PixelStride,
    extent: PixelExtent,
}

impl Raster {
    fn parse(address: u32, stride: u32, geometry: u32) -> PyResult<Self> {
        let extent = PixelExtent { width: (geometry & 0xffff) as usize, height: (geometry >> 16) as usize };
        if address < 0x4000 || address >= 0x2000_0000 || address & 15 != 0
            || geometry & !0x1fff_1fff != 0 || extent.width == 0 || extent.height == 0
            || extent.width & 7 != 0 || stride < extent.width as u32 || stride >= 0x8000 || stride & 7 != 0
        {
            return Err(PyValueError::new_err("Invalid scene scaler DRAM address, geometry or pixel stride"));
        }
        Ok(Self { address: DramOffset(address as usize), stride: PixelStride(stride as usize), extent })
    }

    fn bytes(self) -> usize { 2 * (self.stride.0 * (self.extent.height - 1) + self.extent.width) }
    fn range(self) -> std::ops::Range<usize> { self.address.0..self.address.0 + self.bytes() }
}

fn center(word: u32, extent: PixelExtent) -> PyResult<PixelCenter> {
    let center = PixelCenter { x: i64::from(word & 0xffff), y: i64::from(word >> 16) };
    if word & !0x1fff_1fff != 0 || center.x >= extent.width as i64 || center.y >= extent.height as i64 {
        return Err(PyValueError::new_err("Scene scaler center is outside its raster"));
    }
    Ok(center)
}

#[pyclass(frozen, module = "x70_native._native", name = "DeclaredSceneScalePlan")]
pub(crate) struct Plan {
    source: Raster,
    destination: Raster,
    source_center: PixelCenter,
    destination_center: PixelCenter,
    horizontal_scale: Q16Scale,
    vertical_scale: Q16Scale,
}

#[pymethods]
impl Plan {
    #[new]
    pub(crate) fn new(words: [u32; 17]) -> PyResult<Self> {
        if words[6] != 0 || [8, 9, 10, 11, 13, 14].into_iter().any(|index| words[index] != 0)
            || !(1..0x20000).contains(&words[7]) || !(1..0x20000).contains(&words[12])
        {
            return Err(PyValueError::new_err("Scene scaler admits positive diagonal Q16 scaling with zero window and cross terms"));
        }
        let source = Raster::parse(words[0], words[1], words[2])?;
        let destination = Raster::parse(words[3], words[4], words[5])?;
        if destination.bytes() > 64 * 1024 * 1024 {
            return Err(PyValueError::new_err("Scene scaler output exceeds the declared 64 MiB limit"));
        }
        let plan = Self {
            source, destination,
            source_center: center(words[15], source.extent)?,
            destination_center: center(words[16], destination.extent)?,
            horizontal_scale: Q16Scale(i64::from(words[12])),
            vertical_scale: Q16Scale(i64::from(words[7])),
        };
        plan.validate_bounds(0x2000_0000).map_err(PyValueError::new_err)?;
        Ok(plan)
    }

    #[getter]
    fn source_physical(&self) -> usize { self.source.address.0 }
    #[getter]
    fn source_bytes(&self) -> usize { self.source.bytes() }
    #[getter]
    fn destination_physical(&self) -> usize { self.destination.address.0 }
    #[getter]
    fn destination_bytes(&self) -> usize { self.destination.bytes() }

    fn raster(&self, source: &Bound<'_, PyBytes>, output: &Bound<'_, PyByteArray>) -> PyResult<()> {
        if source.as_bytes().len() != self.source.bytes() || output.len() != self.destination.bytes() {
            return Err(PyValueError::new_err("Scene scaler buffer size differs from its raster extent"));
        }
        // SAFETY: the GIL is held and conversion makes no Python calls.
        self.convert(source.as_bytes(), unsafe { output.as_bytes_mut() });
        Ok(())
    }
}

impl DeclaredRasterPlan for Plan {
    fn source_range(&self) -> std::ops::Range<usize> { self.source.range() }
    fn destination_range(&self) -> std::ops::Range<usize> { self.destination.range() }
    fn source_rows(&self) -> impl Iterator<Item = std::ops::Range<usize>> {
        (0..self.source.extent.height).map(|row| {
            let start = self.source.address.0 + 2 * row * self.source.stride.0;
            start..start + 2 * self.source.extent.width
        })
    }
    fn convert(&self, source: &[u8], destination: &mut [u8]) {
        let sample = |coordinate: usize, origin: i64, target: i64, scale: Q16Scale, length: usize| {
            let fixed = (origin << 16) + (coordinate as i64 - target) * scale.0;
            ((fixed + 0x8000).div_euclid(0x10000)).clamp(0, length as i64 - 1) as usize
        };
        for y in 0..self.destination.extent.height {
            let source_y = sample(y, self.source_center.y, self.destination_center.y,
                self.vertical_scale, self.source.extent.height);
            for x in 0..self.destination.extent.width {
                let source_x = sample(x, self.source_center.x, self.destination_center.x,
                    self.horizontal_scale, self.source.extent.width);
                let from = 2 * (source_y * self.source.stride.0 + source_x);
                let to = 2 * (y * self.destination.stride.0 + x);
                destination[to..to + 2].copy_from_slice(&source[from..from + 2]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use pyo3::prelude::*;
    use pyo3::types::{PyByteArray, PyBytes, PyModule};

    fn api(py: Python<'_>) -> Bound<'_, PyAny> {
        let module = PyModule::new(py, "_native").unwrap();
        crate::_native(&module).unwrap();
        module.getattr("DeclaredSceneScalePlan").unwrap()
    }

    fn descriptor() -> [u32; 17] {
        [0x10000, 24, 0x0008_0010, 0x20000, 16, 0x0004_0008, 0,
            0x1ffff, 0, 0, 0, 0, 0x1ffff, 0, 0, 0, 0]
    }

    fn input() -> Vec<u8> {
        let mut bytes = vec![0xcc; 368];
        for y in 0..8 {
            for x in 0..16 {
                let pixel = (0x1234 + 100 * y + 3 * x) as u16;
                bytes[48 * y + 2 * x..48 * y + 2 * x + 2].copy_from_slice(&pixel.to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn nearest_scene_scale_keeps_sixteen_bit_samples_and_row_padding() {
        Python::initialize();
        Python::attach(|py| {
            let plan = api(py).call1((descriptor(),)).unwrap();
            let output = PyByteArray::new(py, &[0xa5; 112]);
            assert!(plan.call_method1("raster", (PyBytes::new(py, &input()), &output)).unwrap().is_none());
            let actual = output.to_vec();
            let golden = [
                [4660u16, 4666, 4672, 4678, 4684, 4690, 4696, 4702],
                [4860, 4866, 4872, 4878, 4884, 4890, 4896, 4902],
                [5060, 5066, 5072, 5078, 5084, 5090, 5096, 5102],
                [5260, 5266, 5272, 5278, 5284, 5290, 5296, 5302],
            ];
            for (row, pixels) in golden.iter().enumerate() {
                for (column, pixel) in pixels.iter().enumerate() {
                    assert_eq!(&actual[32 * row + 2 * column..32 * row + 2 * column + 2], pixel.to_le_bytes());
                }
                if row != 3 { assert_eq!(&actual[32 * row + 16..32 * row + 32], &[0xa5; 16]); }
            }
        });
    }

    #[test]
    fn centered_scene_scale_clamps_samples_outside_the_source() {
        Python::initialize();
        Python::attach(|py| {
            let mut words = descriptor();
            words[7] = 0x10000;
            words[12] = 0x10000;
            words[16] = 0x0003_0007;
            let plan = api(py).call1((words,)).unwrap();
            let output = PyByteArray::new(py, &[0xa5; 112]);
            plan.call_method1("raster", (PyBytes::new(py, &input()), &output)).unwrap();
            let actual = output.to_vec();
            for row in 0..4 {
                assert_eq!(&actual[32 * row..32 * row + 16], &[0x34, 0x12].repeat(8));
            }
        });
    }

    #[test]
    fn scene_descriptor_refuses_overlap_unknown_transform_and_unbounded_geometry() {
        Python::initialize();
        Python::attach(|py| {
            let class = api(py);
            for (index, value) in [(0, 0x10001), (3, 0x10000), (6, 1), (7, 0),
                (7, 0x20000), (8, 1), (9, 1), (10, 1), (11, 1), (12, 0), (12, 0x20000), (13, 1), (14, 1),
                (1, 8), (2, 0), (2, 0x2000_0010), (4, 7), (15, 16), (16, 8),
                (3, 0x1fff_fff0)]
            {
                let mut words = descriptor();
                words[index] = value;
                assert!(class.call1((words,)).is_err());
            }
        });
    }

    #[test]
    fn scene_buffer_mismatch_refuses_before_any_output_change() {
        Python::initialize();
        Python::attach(|py| {
            let plan = api(py).call1((descriptor(),)).unwrap();
            let output = PyByteArray::new(py, &[0xa5; 112]);
            assert!(plan.call_method1("raster", (PyBytes::new(py, &[0; 367]), &output)).is_err());
            assert_eq!(output.to_vec(), [0xa5; 112]);
            let short = PyByteArray::new(py, &[0xa5; 111]);
            assert!(plan.call_method1("raster", (PyBytes::new(py, &input()), &short)).is_err());
            assert_eq!(short.to_vec(), [0xa5; 111]);
        });
    }
}

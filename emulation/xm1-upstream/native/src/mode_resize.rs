use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

const PYRAMID_STAGES: [[u32; 6]; 4] = [
    [0x500, 0x01e00280, 0x280, 0x00f00140, 0x00a00140, 0x10001000],
    [0x280, 0x00f00140, 0x140, 0x007800a0, 0x00a00140, 0x10001000],
    [0x140, 0x007800a0, 0x0a0, 0x003c0050, 0x005000a0, 0x00001000],
    [0x0a0, 0x003c0050, 0x050, 0x001e0028, 0x00280050, 0x00000000],
];
const MASKS: [u32; 12] = [
    0x00011113, 0xffffffff, 0x00007ff0, 0x1fff1fff,
    0xffffffff, 0x00007ff0, 0x1fff1fff, 0x000003ff,
    0x007f7f7f, 0xffffffff, 0x03ff03ff, 0x31ff31ff,
];

#[pyclass(eq, eq_int, frozen, from_py_object, module = "x70_native._native")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MovrszProfile {
    Pyramid,
    SceneIdentity,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct BusAddress(u32);
#[derive(Clone, Copy, PartialEq, Eq)]
struct ByteStride(usize);
#[derive(Clone, Copy, PartialEq, Eq)]
struct PixelExtent { width: usize, height: usize }
#[derive(Clone, Copy, PartialEq, Eq)]
enum PixelFormat { Luma16, PackedYyCbCr, Opaque16 }
#[derive(Clone, Copy, PartialEq, Eq)]
struct Raster {
    address: BusAddress,
    extent: PixelExtent,
    stride: ByteStride,
    format: PixelFormat,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation { PyramidFirst, PyramidSecond, PyramidThird, PyramidFourth, Identity16 }

#[pyclass(eq, frozen, module = "x70_native._native")]
#[derive(PartialEq, Eq)]
pub(crate) struct DeclaredMovrszPlan {
    source_raster: Raster,
    destination_raster: Raster,
    operation: Operation,
}

fn span(raster: Raster) -> PyResult<(u64, u64)> {
    let address = raster.address.0;
    let physical = if (0x40000000..0x60000000).contains(&address) {
        address & 0x3fffffff
    } else { address };
    let end = u64::from(physical) + (raster.stride.0 * raster.extent.height) as u64;
    if physical < 0x4000 || end > 0x20000000 {
        return Err(PyValueError::new_err("MOVRSZ span is outside DRAM"));
    }
    Ok((u64::from(physical), end))
}

fn extent(word: u32) -> PixelExtent {
    PixelExtent { width: (word & 0x1fff) as usize, height: (word >> 16) as usize }
}

fn format(value: u32) -> PyResult<PixelFormat> {
    match value {
        0 => Ok(PixelFormat::Luma16),
        1 => Ok(PixelFormat::PackedYyCbCr),
        2 => Ok(PixelFormat::Opaque16),
        _ => Err(PyValueError::new_err("MOVRSZ format is outside the declared profiles")),
    }
}

#[pymethods]
impl DeclaredMovrszPlan {
    #[new]
    fn new(profile: MovrszProfile, words: [u32; 12]) -> PyResult<Self> {
        if words.iter().zip(MASKS).any(|(word, mask)| word & !mask != 0)
            || words[1] & 15 != 0 || words[4] & 15 != 0
        {
            return Err(PyValueError::new_err("Invalid MOVRSZ masked configuration or pointer alignment"));
        }
        let stage = [words[2], words[3], words[5], words[6], words[10], words[11]];
        let source_extent = extent(words[3]);
        let destination_extent = extent(words[6]);
        let operation = if words[0] == 0 && words[7..10] == [0x40, 0x2040, 0x4080] {
            PYRAMID_STAGES.iter().position(|known| *known == stage)
                .map(|index| [Operation::PyramidFirst, Operation::PyramidSecond,
                    Operation::PyramidThird, Operation::PyramidFourth][index])
        } else if profile == MovrszProfile::SceneIdentity
            && words[0] == 0x10000 && words[7..10] == [0x40, 0x82030, 0x020a3e6c]
            && source_extent == destination_extent
            && source_extent.width >= 8 && source_extent.width <= 0x200
            && source_extent.width.is_multiple_of(8)
            && words[10] == (source_extent.width as u32 * 0x10001)
            && words[11] == (0x20002000 | ((source_extent.width as u32 - 4) << 16))
        {
            Some(Operation::Identity16)
        } else { None };
        let operation = operation.ok_or_else(|| PyValueError::new_err(
            "MOVRSZ supports only stock pyramid stages or the declared equal-geometry format2 transfer"))?;
        let source_raster = Raster {
            address: BusAddress(words[1]), extent: source_extent,
            stride: ByteStride(words[2] as usize), format: format(words[11] >> 12 & 3)?,
        };
        let destination_raster = Raster {
            address: BusAddress(words[4]), extent: destination_extent,
            stride: ByteStride(words[5] as usize), format: format(words[11] >> 28 & 3)?,
        };
        for raster in [source_raster, destination_raster] {
            if raster.extent.height == 0 || raster.stride.0 < raster.extent.width * 2 {
                return Err(PyValueError::new_err("MOVRSZ stride cannot hold its nonempty raster"));
            }
        }
        let source_span = span(source_raster)?;
        let destination_span = span(destination_raster)?;
        if source_span.0 < destination_span.1 && destination_span.0 < source_span.1 {
            return Err(PyValueError::new_err("MOVRSZ source and destination DRAM spans overlap"));
        }
        Ok(Self { source_raster, destination_raster, operation })
    }

    #[getter]
    fn source(&self) -> u32 { self.source_raster.address.0 }
    #[getter]
    fn destination(&self) -> u32 { self.destination_raster.address.0 }
    #[getter]
    fn source_bytes(&self) -> usize { self.source_raster.stride.0 * self.source_raster.extent.height }
    #[getter]
    fn output_bytes(&self) -> usize { self.destination_raster.stride.0 * self.destination_raster.extent.height }
    #[getter]
    fn stage(&self) -> u8 {
        match self.operation {
            Operation::PyramidFirst => 0, Operation::PyramidSecond => 1,
            Operation::PyramidThird => 2, Operation::PyramidFourth => 3,
            Operation::Identity16 => 4,
        }
    }

    fn raster<'py>(&self, py: Python<'py>, data: &Bound<'_, PyBytes>) -> PyResult<Bound<'py, PyBytes>> {
        let source = data.as_bytes();
        if source.len() != self.source_bytes() {
            return Err(PyValueError::new_err("MOVRSZ source length differs from its raster span"));
        }
        PyBytes::new_with(py, self.output_bytes(), |output| {
            let destination = self.destination_raster;
            match self.operation {
                Operation::Identity16 => {
                    let row_bytes = destination.extent.width * 2;
                    for (input, target) in source.chunks_exact(self.source_raster.stride.0)
                        .zip(output.chunks_exact_mut(destination.stride.0))
                    {
                        target[..row_bytes].copy_from_slice(&input[..row_bytes]);
                    }
                }
                Operation::PyramidFirst | Operation::PyramidSecond
                | Operation::PyramidThird | Operation::PyramidFourth => {
                    for y in 0..destination.extent.height {
                        for x in 0..destination.extent.width {
                            let reduced = self.reduced_luma(source, x, y);
                            let target = y * destination.stride.0;
                            match destination.format {
                                PixelFormat::Luma16 => output[target + x * 2..target + x * 2 + 2]
                                    .copy_from_slice(&reduced.to_le_bytes()),
                                PixelFormat::PackedYyCbCr => {
                                    let target = target + (x / 2) * 4;
                                    output[target + x % 2] = reduced as u8;
                                    if x.is_multiple_of(2) {
                                        let start = y * 2 * self.source_raster.stride.0 + x * 4;
                                        for channel in 2..4 {
                                            let top = start + channel;
                                            let bottom = top + self.source_raster.stride.0;
                                            let sum = u16::from(source[top]) + u16::from(source[top + 4])
                                                + u16::from(source[bottom]) + u16::from(source[bottom + 4]);
                                            output[target + channel] = ((sum + 2) / 4) as u8;
                                        }
                                    }
                                }
                                PixelFormat::Opaque16 => unreachable!(),
                            }
                        }
                    }
                }
            }
            Ok(())
        })
    }
}

impl DeclaredMovrszPlan {
    fn reduced_luma(&self, data: &[u8], x: usize, y: usize) -> u16 {
        let row = y * 2 * self.source_raster.stride.0;
        let read = |offset: usize| match self.source_raster.format {
            PixelFormat::PackedYyCbCr => u32::from(data[offset]) + u32::from(data[offset + 1]),
            PixelFormat::Luma16 => u32::from(u16::from_le_bytes([data[offset], data[offset + 1]]))
                + u32::from(u16::from_le_bytes([data[offset + 2], data[offset + 3]])),
            PixelFormat::Opaque16 => unreachable!(),
        };
        let start = row + x * 4;
        ((read(start) + read(start + self.source_raster.stride.0) + 2) / 4) as u16
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<MovrszProfile>()?;
    m.add_class::<DeclaredMovrszPlan>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene_words() -> [u32; 12] {
        [0x10000, 0x40010000, 16, 0x00020008, 0x41000000, 32,
         0x00020008, 0x40, 0x82030, 0x020a3e6c, 0x00080008, 0x20042000]
    }

    fn plan<'py>(py: Python<'py>, profile: MovrszProfile, words: [u32; 12]) -> PyResult<Bound<'py, PyAny>> {
        let module = PyModule::new(py, "mode_resize_test")?;
        register(&module)?;
        module.getattr("DeclaredMovrszPlan")?.call1((profile, words))
    }

    #[test]
    fn equal_geometry_transfer_preserves_rows_and_zeroes_destination_padding() {
        Python::attach(|py| {
            let value = plan(py, MovrszProfile::SceneIdentity, scene_words()).unwrap();
            let data = PyBytes::new(py, &[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,
                                         21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36]);
            let output = value.call_method1("raster", (data,)).unwrap();
            let output = output.cast::<PyBytes>().unwrap().as_bytes();
            assert_eq!(output, &[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
                                 21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]);
            assert_eq!(value.getattr("source_bytes").unwrap().extract::<usize>().unwrap(), 32);
            assert_eq!(value.getattr("output_bytes").unwrap().extract::<usize>().unwrap(), 64);
        });
    }

    #[test]
    fn scene_transfer_requires_its_profile_and_descriptor_family() {
        Python::attach(|py| {
            assert!(plan(py, MovrszProfile::Pyramid, scene_words()).is_err());
            for (index, replacement) in [(0, 0), (1, 0x40010001), (2, 8), (3, 0x00020010),
                (4, 0x40010000), (6, 0x00030008), (7, 0x41), (8, 0x2040),
                (9, 0x4080), (10, 0x00080010), (11, 0x10041000)]
            {
                let mut words = scene_words();
                words[index] = replacement;
                assert!(plan(py, MovrszProfile::SceneIdentity, words).is_err());
            }
        });
    }

    #[test]
    fn source_size_mismatch_refuses_before_raster_output() {
        Python::attach(|py| {
            let value = plan(py, MovrszProfile::SceneIdentity, scene_words()).unwrap();
            assert!(value.call_method1("raster", (PyBytes::new(py, &[1, 2]),)).is_err());
        });
    }

    #[test]
    fn pyramid_packed_channels_keep_the_declared_box_rounding() {
        Python::attach(|py| {
            let words = [0,0x40010000,0x500,0x01e00280,0x41000000,0x280,
                0x00f00140,0x40,0x2040,0x4080,0x00a00140,0x10001000];
            let value = plan(py, MovrszProfile::Pyramid, words).unwrap();
            let mut data = vec![0; 1280 * 480];
            data[..8].copy_from_slice(&[0,10,100,120,20,30,110,130]);
            data[1280..1288].copy_from_slice(&[10,20,120,140,30,40,130,150]);
            let output = value.call_method1("raster", (PyBytes::new(py, &data),)).unwrap();
            assert_eq!(&output.cast::<PyBytes>().unwrap().as_bytes()[..4], &[10,30,115,135]);
        });
    }

    #[test]
    fn pyramid_luma_keeps_sixteen_bit_values() {
        Python::attach(|py| {
            let words = [0,0x40010000,0xa0,0x003c0050,0x41000000,0x50,
                0x001e0028,0x40,0x2040,0x4080,0x00280050,0];
            let value = plan(py, MovrszProfile::Pyramid, words).unwrap();
            let mut data = vec![0; 160 * 60];
            data[..4].copy_from_slice(&[0xe8,0x03,0xd0,0x07]);
            data[160..164].copy_from_slice(&[0xb8,0x0b,0xa0,0x0f]);
            let output = value.call_method1("raster", (PyBytes::new(py, &data),)).unwrap();
            assert_eq!(&output.cast::<PyBytes>().unwrap().as_bytes()[..2], &[0xc4,0x09]);
        });
    }
}

//! X-M1 1.50 centered radial paint branch 0x008440ec.

use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};

#[derive(Clone, Copy)]
enum Geometry {
    Linear,
    CenteredRadial([f64; 2]),
}

#[derive(Clone, Copy)]
enum Spread {
    Pad,
    Repeat,
    Reflect,
}

struct Gradient {
    geometry: Geometry,
    coefficients: [f64; 4],
    colors: [[u8; 4]; 128],
    spread: Spread,
    premultiplied: bool,
}

fn float24(word: u32) -> Result<f64, &'static str> {
    let exponent = word >> 16 & 127;
    if word >> 24 != 0 || exponent == 127 || (exponent == 0 && word & 0x7fffff != 0) {
        return Err("Graphics UI gradient has unsupported float24 encoding");
    }
    let magnitude = if exponent == 0 { 0.0 } else {
        (1.0 + f64::from(word & 65535) / 65536.0) * 2.0f64.powi(exponent as i32 - 63)
    };
    if magnitude > 16777216.0 {
        return Err("Graphics UI gradient exceeds the declared coordinate bound");
    }
    Ok(if word & 0x800000 == 0 { magnitude } else { -magnitude })
}

impl Gradient {
    fn parse(word: impl Fn(u32) -> Result<u32, &'static str>) -> Result<Self, &'static str> {
        let number = |index| float24(word(index)?);
        let coefficients = [number(0x106)?, number(0x108)?, number(0x10a)?, number(0x10b)?];
        if number(0x107)? != 0.0 || number(0x109)? != 0.0 || word(0x11b)? != 0 {
            return Err("Graphics UI gradient coordinate controls are unsupported");
        }
        let geometry = match word(0x168)? {
            0 => Geometry::Linear,
            0x31 => {
                let radii = [number(0x169)?, number(0x16a)?];
                if radii[0] <= 0.0 || radii[0] != radii[1] {
                    return Err("Graphics UI centered radial gradient requires equal positive squared radii");
                }
                Geometry::CenteredRadial(radii)
            }
            _ => return Err("Graphics UI supports only linear and centered radial gradients"),
        };
        let spread = match word(0x16d)? {
            0x10 => Spread::Pad,
            0x11 => Spread::Repeat,
            0x12 => Spread::Reflect,
            _ => return Err("Graphics UI gradient spread is unsupported"),
        };
        let premultiplied = match word(0x14e)? {
            0 => false,
            0x20 => true,
            _ => return Err("Graphics UI gradient premultiplication is unsupported"),
        };
        let mut colors = [[0; 4]; 128];
        for index in 0..256 {
            if word(0x10100 + index)? != 15 {
                return Err("Graphics UI gradient requires two complete ramp banks");
            }
        }
        for (index, color) in colors.iter_mut().enumerate() {
            *color = word(0x10000 + index as u32)?.to_le_bytes();
            let following = word(0x10000 + (index + 1).min(127) as u32)?.to_le_bytes();
            let delta = std::array::from_fn(|lane| {
                (((i16::from(following[lane]) - i16::from(color[lane])) & 511) >> 1) as u8
            });
            if word(0x10080 + index as u32)? != u32::from_le_bytes(delta) {
                return Err("Graphics UI gradient delta bank disagrees with X-M1 writer 0x00842ae4");
            }
        }
        Ok(Self { geometry, coefficients, colors, spread, premultiplied })
    }

    fn sample(&self, x: f64, y: f64) -> [f64; 4] {
        let [a, c, e, f] = self.coefficients;
        let amount = match self.geometry {
            Geometry::Linear => a * x + c + e * y + f,
            Geometry::CenteredRadial([rx, ry]) => {
                let u = a * x + c;
                let v = e * y + f;
                (u * u * rx + v * v * ry).sqrt()
            }
        };
        let amount = match self.spread {
            Spread::Pad => amount.clamp(0.0, 1.0),
            Spread::Repeat => amount.rem_euclid(1.0),
            Spread::Reflect => 1.0 - (amount.rem_euclid(2.0) - 1.0).abs(),
        };
        let position = (amount * 127.0 + 0.5).clamp(0.0, 127.0);
        let low = position.floor() as usize;
        let fraction = position - low as f64;
        let mut color = std::array::from_fn(|lane| {
            f64::from(self.colors[low][lane]) / 255.0 * (1.0 - fraction)
                + f64::from(self.colors[(low + 1).min(127)][lane]) / 255.0 * fraction
        });
        if self.premultiplied {
            for lane in 0..3 {
                color[lane] = if color[3] == 0.0 { 0.0 } else { color[lane] / color[3] };
            }
        }
        color
    }
}

fn unsupported(message: &'static str) -> PyErr {
    Python::attach(|py| {
        match PyModule::import(py, "emulator.hardware").and_then(|module| module.getattr("UnsupportedDevice")) {
            Ok(class) => match class.call1((message,)) {
                Ok(error) => PyErr::from_value(error),
                Err(error) => error,
            },
            Err(error) => error,
        }
    })
}

#[pyfunction]
pub(crate) fn ui_gradient<'py>(py: Python<'py>, registers: &Bound<'py, PyDict>) -> PyResult<Bound<'py, PyDict>> {
    let gradient = Gradient::parse(|index| {
        registers.get_item(index).ok().flatten().and_then(|value| value.extract().ok())
            .ok_or("Graphics UI gradient register is missing or invalid")
    }).map_err(unsupported)?;
    let result = PyDict::new(py);
    result.set_item("coefficients", gradient.coefficients)?;
    result.set_item("colors", gradient.colors.as_slice())?;
    result.set_item("radii", match gradient.geometry { Geometry::Linear => None, Geometry::CenteredRadial(radii) => Some(radii) })?;
    result.set_item("spread", match gradient.spread { Spread::Pad => 0, Spread::Repeat => 1, Spread::Reflect => 2 })?;
    result.set_item("premultiplied", gradient.premultiplied)?;
    Ok(result)
}

#[pyfunction]
pub(crate) fn ui_gradient_colors<'py>(
    py: Python<'py>, descriptor: &Bound<'py, PyDict>, shape: (usize, usize), x0: usize, y0: usize,
) -> PyResult<Bound<'py, PyAny>> {
    let field = |name| descriptor.get_item(name)?.ok_or_else(|| unsupported("Graphics UI gradient descriptor is incomplete"));
    let radii: Option<[f64; 2]> = field("radii")?.extract()?;
    let gradient = Gradient {
        geometry: radii.map_or(Geometry::Linear, Geometry::CenteredRadial),
        coefficients: field("coefficients")?.extract()?,
        colors: field("colors")?.extract()?,
        spread: match field("spread")?.extract::<u8>()? {
            0 => Spread::Pad, 1 => Spread::Repeat, 2 => Spread::Reflect,
            _ => return Err(unsupported("Graphics UI gradient spread is unsupported")),
        },
        premultiplied: field("premultiplied")?.extract()?,
    };
    let pixels = shape.0.checked_mul(shape.1).filter(|&n| n <= 0x100000)
        .ok_or_else(|| unsupported("Graphics UI gradient exceeds the framebuffer bound"))?;
    let output = PyBytes::new_with(py, pixels * 32, |output| {
        for (row, bytes) in output.chunks_exact_mut((shape.1 * 32).max(1)).enumerate() {
            for (column, pixel) in bytes.chunks_exact_mut(32).enumerate() {
                for (channel, value) in pixel.chunks_exact_mut(8).zip(
                    gradient.sample((x0 + column) as f64 + 0.5, (y0 + row) as f64 + 0.5),
                ) {
                    channel.copy_from_slice(&value.to_ne_bytes());
                }
            }
        }
        Ok(())
    })?;
    PyModule::import(py, "numpy")?.getattr("frombuffer")?.call1((output, "float64"))?
        .call_method1("reshape", ((shape.0, shape.1, 4),))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn parse(registers: &HashMap<u32, u32>) -> Result<Gradient, &'static str> {
        Gradient::parse(|index| registers.get(&index).copied().ok_or("Missing fixture register"))
    }

    fn radial_registers() -> HashMap<u32, u32> {
        let mut registers = HashMap::from([
            (0x106, 0x3b0000), (0x107, 0), (0x108, 0xbd0000),
            (0x109, 0), (0x10a, 0x3b0000), (0x10b, 0xbd0000),
            (0x168, 0x31), (0x169, 0x430000), (0x16a, 0x430000),
            (0x16d, 0x10), (0x14e, 0), (0x11b, 0),
        ]);
        // Synthetic fixture.
        for index in 0..128 {
            let gray = if index < 64 { 0 } else { 255 };
            registers.insert(0x10000 + index, u32::from_le_bytes([gray, gray, gray, 255]));
            registers.insert(0x10080 + index, if index == 63 { 0x007f7f7f } else { 0 });
            registers.insert(0x10100 + index, 15);
            registers.insert(0x10180 + index, 15);
        }
        registers
    }

    #[test]
    fn centered_radial_ramp_is_symmetric_and_pads_outside_the_circle() {
        let gradient = parse(&radial_registers()).unwrap();
        assert_eq!(gradient.sample(4.0, 4.0), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(gradient.sample(5.9, 4.0), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(gradient.sample(6.0, 4.0), [1.0; 4]);
        for (x, y) in [(8.0, 4.0), (0.0, 4.0), (4.0, 8.0), (4.0, 0.0), (40.0, 40.0)] {
            assert_eq!(gradient.sample(x, y), [1.0; 4]);
        }
    }

    #[test]
    fn linear_spread_and_premultiplied_alpha_are_preserved() {
        let mut registers = radial_registers();
        registers.insert(0x168, 0);
        registers.insert(0x106, 0x3f0000);
        registers.insert(0x108, 0);
        registers.insert(0x10a, 0);
        registers.insert(0x10b, 0);
        assert_eq!(parse(&registers).unwrap().sample(-0.1, 0.0), [0.0, 0.0, 0.0, 1.0]);
        registers.insert(0x16d, 0x11);
        assert_eq!(parse(&registers).unwrap().sample(-0.1, 0.0), [1.0; 4]);
        registers.insert(0x16d, 0x12);
        assert_eq!(parse(&registers).unwrap().sample(1.9, 0.0), [0.0, 0.0, 0.0, 1.0]);
        registers.insert(0x14e, 0x20);
        for index in 0..128 {
            registers.insert(0x10000 + index, u32::from_le_bytes([32, 16, 0, 64]));
            registers.insert(0x10080 + index, 0);
        }
        assert_eq!(parse(&registers).unwrap().sample(0.25, 0.0), [0.5, 0.25, 0.0, 64.0 / 255.0]);
    }

    #[test]
    fn incomplete_radial_ramp_and_off_center_geometry_are_refused() {
        let mut registers = radial_registers();
        registers.insert(0x168, 1);
        assert!(parse(&registers).is_err());
        registers.insert(0x168, 0x31);
        registers.remove(&0x169);
        assert!(parse(&registers).is_err());
        registers.insert(0x169, 0x430000);
        registers.insert(0x10180, 3);
        assert!(parse(&registers).is_err());
    }
}

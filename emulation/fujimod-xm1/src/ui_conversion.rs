//! The retained engine's declared UI conversion at FFF47000
//! (`emulator/image_parameter_devices.py`, `native/src/raster.rs`): one
//! YYCbCr-to-ABGR job per start, completed one tick later with IRQ 67. It is
//! the stock CPU-fallback contract, not measured silicon behaviour.

use fujimod_emulator_devices::bus::InterruptLine;
use fujimod_emulator_devices::{Cx, Device};
use fujimod_emulator_exr2_plan::{Adapter, AdapterFactory, Resources};
use fujimod_emulator_devices::graph::Mapping;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const IMPLEMENTATION: &str = "xm1-declared-ui-conversion";
const BASE: u64 = 0xfff4_7000;
const WINDOW_BYTES: u64 = 0x84;
const MAX_OUTPUT_BYTES: usize = 64 << 20;

const CONTROL: u64 = 0x00;
const COMMAND: u64 = 0x04;
const SELECTOR: u64 = 0x08;
const SOURCE: u64 = 0x10;
const SOURCE_STRIDE: u64 = 0x14;
const SOURCE_GEOMETRY: u64 = 0x18;
const DESTINATION: u64 = 0x20;
const DESTINATION_STRIDE: u64 = 0x24;
const DESTINATION_GEOMETRY: u64 = 0x28;
const WINDOW: u64 = 0x2c;
const AUXILIARY: u64 = 0x70;
const STATUS: u64 = 0x74;
const ACKNOWLEDGE: u64 = 0x78;
const MASK: u64 = 0x7c;
const ALPHA: u64 = 0x80;

const START: u64 = 3;
const SOFTWARE_BLOCK_32: u32 = 1;
const COMPLETION_MASK: u32 = 0x1301;
const OPAQUE: u32 = 255;
const SUCCESS: u64 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum Phase {
    #[default]
    Idle,
    Programming,
    Active,
    CompletedLatched,
    Acknowledged,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    irq: InterruptLine,
    ram_bytes: u64,
}

/// One job's checked extents, in physical DRAM offsets and pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Job {
    source: u64,
    destination: u64,
    width: usize,
    height: usize,
    stride: usize,
    core: usize,
    due: u64,
}

pub struct UiConversion {
    irq: InterruptLine,
    ram_bytes: u64,
    phase: Phase,
    reset_seen: bool,
    programming: BTreeMap<u64, u32>,
    job: Option<Job>,
}

impl UiConversion {
    fn new(options: Options) -> Result<Self, String> {
        if options.ram_bytes == 0 {
            return Err("declared UI conversion requires DRAM".into());
        }
        Ok(Self { irq: options.irq, ram_bytes: options.ram_bytes, phase: Phase::Idle, reset_seen: false, programming: BTreeMap::new(), job: None })
    }

    fn field(&self, register: u64) -> Result<u32, String> {
        self.programming.get(&register).copied().ok_or_else(|| format!("register {register:#x} is not programmed"))
    }

    /// Checks one complete programming generation against the contract.
    fn plan(&self, core: usize, now: u64) -> Result<Job, String> {
        let exact = [(SELECTOR, SOFTWARE_BLOCK_32), (WINDOW, 0), (AUXILIARY, 0), (MASK, COMPLETION_MASK), (ALPHA, OPAQUE)];
        for (register, value) in exact {
            if self.field(register)? != value {
                return Err(format!("register {register:#x} differs from its declared value"));
            }
        }
        let address = |register| {
            let value = u64::from(self.field(register)?);
            if value % 16 != 0 {
                return Err("UI physical address must be 16-aligned".to_owned());
            }
            Ok(value)
        };
        let stride = |register| {
            let value = self.field(register)? as usize;
            if value == 0 || value >= 1 << 15 || value % 16 != 0 {
                return Err("UI pixel stride must be positive, 16-aligned and 15-bit".to_owned());
            }
            Ok(value)
        };
        let geometry = |register| {
            let word = self.field(register)?;
            let (width, height) = ((word & 0xffff) as usize, (word >> 16) as usize);
            if width == 0 || width >= 1 << 13 || width % 16 != 0 || height == 0 || height >= 1 << 13 || height % 8 != 0 {
                return Err("UI geometry requires 13-bit positive 16/8-aligned dimensions".to_owned());
            }
            Ok((width, height))
        };
        let (source, destination) = (address(SOURCE)?, address(DESTINATION)?);
        let (width, height) = geometry(SOURCE_GEOMETRY)?;
        let source_stride = stride(SOURCE_STRIDE)?;
        if geometry(DESTINATION_GEOMETRY)? != (width, height) || stride(DESTINATION_STRIDE)? != width || source_stride < width {
            return Err("UI conversion requires identity geometry and bounded row strides".into());
        }
        let source_bytes = (2 * source_stride * (height - 1) + 2 * width) as u64;
        let destination_bytes = 4 * width * height;
        if destination_bytes > MAX_OUTPUT_BYTES {
            return Err("UI output exceeds the declared 64 MiB limit".into());
        }
        let destination_end = destination + destination_bytes as u64;
        let source_end = source + source_bytes;
        if source_end > self.ram_bytes || destination_end > self.ram_bytes {
            return Err("UI conversion extents must lie in physical DRAM".into());
        }
        if source < destination_end && destination < source_end {
            return Err("UI conversion source and destination must be disjoint".into());
        }
        Ok(Job { source, destination, width, height, stride: source_stride, core, due: now + 1 })
    }

    fn complete(&mut self, cx: &mut Cx<'_>) {
        let Some(job) = self.job.filter(|job| self.phase == Phase::Active && cx.now() >= job.due) else {
            return;
        };
        let pitch = 2 * job.stride;
        let mut output = vec![0u8; 4 * job.width * job.height];
        let mut row = vec![0u8; 2 * job.width];
        for (index, line) in output.chunks_exact_mut(4 * job.width).enumerate() {
            if let Err(error) = cx.ram_read(job.source + (index * pitch) as u64, &mut row) {
                cx.fail(error);
                return;
            }
            convert_row(&row, line);
        }
        if let Err(error) = cx.ram_write(job.destination, &output) {
            cx.fail(error);
            return;
        }
        self.job = None;
        self.phase = Phase::CompletedLatched;
        let raised = match self.irq {
            InterruptLine::Core { core, line } => cx.raise_irq(core, line),
            InterruptLine::Shared(line) => {
                cx.set_irq(line, true);
                Ok(())
            }
        };
        if let Err(error) = raised {
            cx.fail(error);
        }
    }
}

/// Stock CPU-fallback coefficients, signed division toward zero, `[255, B, G, R]` bytes.
fn convert_row(source: &[u8], output: &mut [u8]) {
    for (pair, pixels) in source.chunks_exact(4).zip(output.chunks_exact_mut(8)) {
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

impl Device for UiConversion {
    fujimod_emulator_devices::checkpoint_fields!(phase: Phase, reset_seen: bool, programming: BTreeMap<u64, u32>, job: Option<Job>);

    fn fingerprint(&self) -> Result<serde_json::Value, String> {
        serde_json::to_value((self.phase, self.reset_seen, &self.programming, self.job)).map_err(|e| e.to_string())
    }

    fn name(&self) -> &'static str {
        "xm1-declared-ui-conversion"
    }

    fn read(&mut self, cx: &mut Cx<'_>, address: u64, size: u8) -> u64 {
        self.advance(cx);
        if size != 4 || address != BASE + STATUS || self.phase != Phase::CompletedLatched {
            cx.unsupported("declared UI conversion status is readable only after completion");
            return 0;
        }
        SUCCESS
    }

    fn write(&mut self, cx: &mut Cx<'_>, address: u64, size: u8, value: u64) {
        self.advance(cx);
        let register = address.wrapping_sub(BASE);
        if size != 4 || register > ALPHA || value > u64::from(u32::MAX) {
            cx.unsupported("declared UI conversion takes aligned 32-bit words");
            return;
        }
        let value32 = value as u32;
        if self.phase == Phase::Active {
            cx.fail("declared UI conversion: active programming cannot change");
            return;
        }
        match register {
            CONTROL => {
                let masked = self.programming.get(&MASK).copied().unwrap_or(0) == 0;
                if value != 0 || !(self.phase == Phase::Idle || self.phase == Phase::Acknowledged && masked) {
                    cx.fail("declared UI conversion: reset requires idle or acknowledged mask-zero cleanup");
                    return;
                }
                *self = Self { irq: self.irq, ram_bytes: self.ram_bytes, phase: Phase::Idle, reset_seen: true, programming: BTreeMap::new(), job: None };
            }
            COMMAND => {
                if value != START || self.phase != Phase::Programming {
                    cx.fail("declared UI conversion: start requires one fresh complete programming generation");
                    return;
                }
                match self.plan(cx.cpu, cx.now()) {
                    Ok(job) => {
                        cx.schedule(job.due, 0);
                        self.job = Some(job);
                        self.phase = Phase::Active;
                    }
                    Err(error) => cx.fail(format!("declared UI conversion: {error}")),
                }
            }
            ACKNOWLEDGE => {
                if value != SUCCESS || self.phase != Phase::CompletedLatched {
                    cx.fail("declared UI conversion: acknowledgement requires the completed latch");
                    return;
                }
                self.phase = Phase::Acknowledged;
            }
            _ if self.phase == Phase::Acknowledged => {
                if register != MASK || value != 0 {
                    cx.fail("declared UI conversion: acknowledged cleanup requires mask zero");
                    return;
                }
                self.programming.insert(MASK, 0);
            }
            _ if self.phase == Phase::CompletedLatched => {
                cx.fail("declared UI conversion: completed programming requires acknowledgement");
            }
            SELECTOR | SOURCE | SOURCE_STRIDE | SOURCE_GEOMETRY | DESTINATION | DESTINATION_STRIDE
            | DESTINATION_GEOMETRY | WINDOW | AUXILIARY | MASK | ALPHA => {
                if self.phase == Phase::Idle && register == MASK && value == 0 {
                    return;
                }
                if !self.reset_seen {
                    cx.fail("declared UI conversion: programming requires an idle-zero generation");
                    return;
                }
                self.programming.insert(register, value32);
                self.phase = Phase::Programming;
            }
            _ => cx.unsupported("declared UI conversion: unobserved register"),
        }
    }

    fn event(&mut self, cx: &mut Cx<'_>, _token: u64) {
        self.complete(cx);
    }

    fn advance(&mut self, cx: &mut Cx<'_>) {
        self.complete(cx);
    }

    fn next_deadline(&self, now: u64) -> Option<u64> {
        self.job.filter(|_| self.phase == Phase::Active).map(|job| job.due.max(now))
    }
}

/// Builds the driver's adapter devices; every other implementation is refused.
pub struct Xm1Adapters;

impl AdapterFactory for Xm1Adapters {
    fn construct(
        &mut self,
        name: &str,
        spec: &Adapter,
        windows: &[Mapping],
        _: &Resources,
        _: &BTreeMap<String, usize>,
    ) -> Result<Box<dyn Device>, String> {
        if spec.implementation != IMPLEMENTATION {
            return Err(format!("{name} requires unavailable adapter {}", spec.implementation));
        }
        if windows.len() != 1 || windows[0].base != BASE || windows[0].size < WINDOW_BYTES {
            return Err(format!("{name} requires the FFF47000 register window"));
        }
        let options: Options = serde_json::from_value(spec.options.clone()).map_err(|e| format!("{name}: {e}"))?;
        Ok(Box::new(UiConversion::new(options)?))
    }
}

#[cfg(test)]
mod tests {
    use super::convert_row;

    #[test]
    fn black_and_white_pairs_convert_to_opaque_abgr() {
        let mut output = [0x55; 8];
        convert_row(&[0, 255, 128, 128], &mut output);
        assert_eq!(output, [255, 0, 0, 0, 255, 255, 255, 255]);
    }

    #[test]
    fn saturated_chroma_clamps_each_channel() {
        let mut output = [0; 8];
        convert_row(&[128, 128, 255, 0], &mut output);
        assert_eq!(output[..4], [255, 255, 175, 0]);
    }
}

//! Full-plane completeness of a visible capture's `scanout.json`, recomputed from the native controller registers.
use super::SideFile;
use crate::composed_catalog::Sha256;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn require(value: bool, detail: &str) -> Result<(), String> { if value { Ok(()) } else { Err(detail.into()) } }

#[derive(Deserialize, Serialize)]
struct Pixels {
    width: u32,
    height: u32,
    rgb_bytes: u64,
    rgb_sha256: Sha256,
}

#[derive(Deserialize, Serialize)]
struct Artifact {
    path: PathBuf,
    sha256: Sha256,
    bytes: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(try_from = "u8", into = "u8")]
enum Layer {
    Lower,
    Upper,
}

impl TryFrom<u8> for Layer {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Lower),
            1 => Ok(Self::Upper),
            _ => Err("unknown native graphics layer"),
        }
    }
}

impl From<Layer> for u8 {
    fn from(value: Layer) -> Self {
        match value {
            Layer::Lower => 0,
            Layer::Upper => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(try_from = "u8", into = "u8")]
enum Channel {
    Zero,
    One,
    Two,
    Three,
}

impl TryFrom<u8> for Channel {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Zero),
            1 => Ok(Self::One),
            2 => Ok(Self::Two),
            3 => Ok(Self::Three),
            _ => Err("unknown native graphics channel"),
        }
    }
}

impl From<Channel> for u8 {
    fn from(value: Channel) -> Self {
        match value {
            Channel::Zero => 0,
            Channel::One => 1,
            Channel::Two => 2,
            Channel::Three => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum PlaneKind {
    Main,
    Graphics { layer: Layer, channel: Channel },
}

#[derive(Deserialize, Serialize)]
struct Plane {
    #[serde(flatten)]
    kind: PlaneKind,
    native_source_address: u64,
    offset: u64,
    bytes: u64,
}

#[derive(Deserialize, Serialize)]
struct Scanout {
    schema: u32,
    instructions: u64,
    node: String,
    instance: usize,
    pixels: Pixels,
    png: Artifact,
    source_bytes: Artifact,
    source_planes: Vec<Plane>,
    native_scanout_state: Box<RawValue>,
    native_frame_state: Box<RawValue>,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(try_from = "String")]
struct NativeWord(u32);

impl TryFrom<String> for NativeWord {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        u32::from_str_radix(value.strip_prefix("0x").ok_or("native word lacks a hexadecimal prefix")?, 16).map(Self).map_err(|error| error.to_string())
    }
}

#[derive(Deserialize)]
enum Backdrop {
    #[serde(rename = "declared-black-canvas-unmeasured")]
    Black,
    #[serde(rename = "declared-white-canvas-unmeasured")]
    White,
}

#[derive(Deserialize)]
struct NativeController {
    instances: Vec<NativeInstance>,
    backdrop_profile: Option<Backdrop>,
}

#[derive(Deserialize)]
struct NativeInstance {
    instance: usize,
    image_base: NativeWord,
    registers: BTreeMap<NativeWord, NativeWord>,
}

#[derive(Deserialize)]
struct MainGeometry {
    width: u32,
    height: u32,
    source: NativeWord,
    stride: u64,
}

#[derive(Deserialize)]
struct FramePlane {
    layer: Layer,
    channel: Channel,
    width: u32,
    height: u32,
    source: NativeWord,
    origin: [u32; 2],
    visible: Option<bool>,
    stride_bytes: Option<u64>,
}

#[derive(Deserialize)]
struct NativeFrame {
    instance: usize,
    image_base: NativeWord,
    width: u32,
    height: u32,
    #[serde(default)]
    graphics: Vec<FramePlane>,
    #[serde(default)]
    declared_backdrop_used: bool,
    main: Option<MainGeometry>,
    source: Option<NativeWord>,
    stride: Option<u64>,
    canvas: Option<[u32; 2]>,
}

#[derive(Clone, Copy)]
struct Rectangle {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// The selected planes, their extents and canvas coverage as the controller registers define them.
fn full_planes(scanout: &Scanout) -> Result<(), String> {
    let controller: NativeController = serde_json::from_str(scanout.native_scanout_state.get()).map_err(|error| error.to_string())?;
    let frame: NativeFrame = serde_json::from_str(scanout.native_frame_state.get()).map_err(|error| error.to_string())?;
    require(scanout.instance < 2 && frame.instance == scanout.instance, "native scanout instance differs")?;
    let mut instances = controller.instances.iter().filter(|instance| instance.instance == scanout.instance);
    let instance = instances.next().ok_or("native controller instance is missing")?;
    require(instances.next().is_none(), "native controller instance is duplicated")?;
    let base = [0xfff6b000, 0xfff6e000][scanout.instance];
    require(instance.image_base.0 == base && frame.image_base.0 == base, "native controller base differs")?;
    let reg = |offset: u32| instance.registers.get(&NativeWord(base + offset)).map_or(0, |word| word.0);
    let canvas = [((reg(0x10) >> 16) & 0xfff) + 1, (reg(0x10) & 0xfff) + 1];
    let mut expected: Vec<Plane> = Vec::with_capacity(9);
    let mut endpoint = 0u64;
    let mut add = |kind, address: u32, width: u32, height: u32, padding: u32, depth: u32| -> Result<u64, String> {
        let stride = u64::from(width + padding) * u64::from(depth);
        let bytes = u64::from(height - 1).checked_mul(stride).and_then(|value| value.checked_add(u64::from(width) * u64::from(depth)))
            .ok_or("native plane geometry overflows")?;
        require(u64::from(address).checked_add(bytes).is_some_and(|end| end <= 0x2000_0000), "native plane exceeds physical DRAM")?;
        expected.push(Plane { kind, native_source_address: u64::from(address), offset: endpoint, bytes });
        endpoint = endpoint.checked_add(bytes).ok_or("native source extent overflows")?;
        Ok(stride)
    };
    let main_enabled = reg(0) & 1 != 0;
    if main_enabled {
        let dimensions = reg(0x28);
        let (width, height) = (((dimensions >> 16) & 0x1fff) + 1, (dimensions & 0x1fff) + 1);
        let address = reg(0x2c);
        require(dimensions & 0xe000e000 == 0 && address & 0xc0000000 == 0 && reg(0x30) & !0xfff == 0, "native main geometry has unmodeled controls")?;
        let stride = add(PlaneKind::Main, address, width, height, reg(0x30), 2)?;
        if let Some(main) = &frame.main {
            require(main.width == width && main.height == height && main.source.0 == address && main.stride == stride,
                "native main frame geometry differs from registers")?;
        } else {
            require(frame.width == width && frame.height == height && frame.source == Some(NativeWord(address)) && frame.stride == Some(stride),
                "native direct main geometry differs from registers")?;
        }
    } else {
        require(frame.main.is_none() && frame.source.is_none(), "native frame invents an inactive main plane")?;
    }
    let mut rectangles = Vec::with_capacity(8);
    let mut channels = 0usize;
    for layer in [Layer::Lower, Layer::Upper] {
        let bank = u32::from(u8::from(layer)) * 0x50;
        let mode = reg(0x34 + bank);
        let order = if layer == Layer::Lower {
            [Channel::Zero, Channel::One, Channel::Two, Channel::Three]
        } else {
            [Channel::Three, Channel::Two, Channel::One, Channel::Zero]
        };
        for channel in order {
            let index = u32::from(u8::from(channel));
            if reg(0x40 + bank) & (1 << (index * 4)) == 0 {
                continue;
            }
            channels += 1;
            let mut matches = frame.graphics.iter().filter(|plane| plane.layer == layer && plane.channel == channel);
            let plane = matches.next().ok_or("enabled native plane identity is missing")?;
            require(matches.next().is_none(), "enabled native plane identity is duplicated")?;
            let dimensions = reg(0x54 + bank + index * 4);
            let (width, height) = (((dimensions >> 16) & 0x1fff) + 1, (dimensions & 0x1fff) + 1);
            let address = reg(0x64 + bank + index * 4);
            let padding = reg(0x74 + bank + index * 4);
            let origin = reg(0x44 + bank + index * 4);
            let orientation = (reg(8) >> if layer == Layer::Upper { 8 } else { 4 }) & 0xf;
            require(mode <= 1 && dimensions & 0xe000e000 == 0 && address & 0xc0000000 == 0 && padding & !0xfff == 0 && origin & 0xc000c000 == 0
                && matches!(orientation, 0 | 5), "native graphics geometry has unmodeled controls")?;
            let scale = if orientation == 5 { 2 } else { 1 };
            let (x, y) = (((origin >> 16) & 0x3fff) * scale, (origin & 0x3fff) * scale);
            require(plane.width == width && plane.height == height && plane.source.0 == address && plane.origin == [x, y],
                "native graphics frame geometry differs from registers")?;
            if x >= canvas[0] || y >= canvas[1] {
                require(plane.visible == Some(false), "an off-canvas native plane claims source bytes")?;
                continue;
            }
            require(plane.visible != Some(false), "a visible native plane was omitted")?;
            let stride = add(PlaneKind::Graphics { layer, channel }, address, width, height, padding, if mode == 0 { 2 } else { 4 })?;
            require(plane.stride_bytes == Some(stride), "native graphics stride differs from its geometry")?;
            rectangles.push(Rectangle { x, y, width: (width * scale).min(canvas[0] - x), height: (height * scale).min(canvas[1] - y) });
        }
    }
    require(frame.graphics.len() == channels, "native frame includes an inactive graphics plane")?;
    let dimensions = if channels == 0 { frame.canvas.unwrap_or([frame.width, frame.height]) } else { canvas };
    require([scanout.pixels.width, scanout.pixels.height] == dimensions && (channels == 0 || [frame.width, frame.height] == canvas)
        && frame.declared_backdrop_used == (channels != 0 && !main_enabled && controller.backdrop_profile.is_some()),
        "native composition canvas or backdrop differs")?;
    if !main_enabled && !frame.declared_backdrop_used {
        let mut row = Vec::with_capacity(8);
        for y in 0..canvas[1] {
            row.clear();
            row.extend(rectangles.iter().filter(|rectangle| rectangle.y <= y && y < rectangle.y + rectangle.height)
                .map(|rectangle| (rectangle.x, rectangle.x + rectangle.width)));
            row.sort_unstable();
            let mut covered = 0;
            for &(begin, end) in &row {
                require(begin <= covered, "native planes leave a canvas coverage gap")?;
                covered = covered.max(end);
            }
            require(covered == canvas[0], "native planes do not cover the full canvas")?;
        }
    }
    require(!expected.is_empty() && expected.len() == scanout.source_planes.len(), "the native selected plane set differs")?;
    for (expected, actual) in expected.iter().zip(&scanout.source_planes) {
        require(expected.kind == actual.kind && expected.native_source_address == actual.native_source_address && expected.offset == actual.offset
            && expected.bytes == actual.bytes, "a native full plane is truncated or differs")?;
    }
    require(endpoint == scanout.source_bytes.bytes, "native full-plane source bytes are incomplete")
}

fn binds(artifact: &Artifact, file: &SideFile) -> bool {
    artifact.path.file_name().and_then(|name| name.to_str()) == Some(file.path.as_str()) && artifact.sha256 == file.sha256 && artifact.bytes == file.bytes
}

/// The scanout record's bindings of the PNG and planes files, its pixel identity and its full planes.
pub(crate) fn complete(scanout: &[u8], png: &SideFile, planes: &SideFile, planes_bytes: u64) -> Result<(), String> {
    let scanout: Scanout = serde_json::from_slice(scanout).map_err(|error| format!("scanout: {error}"))?;
    let pixels = u64::from(scanout.pixels.width).checked_mul(u64::from(scanout.pixels.height)).and_then(|pixels| pixels.checked_mul(3));
    require(scanout.schema == 1 && scanout.pixels.width > 0 && scanout.pixels.height > 0 && pixels == Some(scanout.pixels.rgb_bytes),
        "native pixel identity or dimensions differ")?;
    require(binds(&scanout.png, png) && binds(&scanout.source_bytes, planes) && planes.bytes == planes_bytes, "the scanout binds other files")?;
    full_planes(&scanout)
}

/// The scanout record and planes with the last selected plane one byte short, consistently rebound.
pub(crate) fn truncate(scanout: &[u8], planes: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut scanout: Scanout = serde_json::from_slice(scanout).map_err(|error| format!("scanout: {error}"))?;
    let mut planes = planes.to_vec();
    let plane = scanout.source_planes.last_mut().ok_or("no selected plane to truncate")?;
    require(plane.bytes > 1 && plane.offset.checked_add(plane.bytes) == Some(planes.len() as u64), "the final plane is not complete")?;
    planes.pop();
    plane.bytes -= 1;
    scanout.source_bytes.bytes -= 1;
    scanout.source_bytes.sha256 = Sha256::of(&planes);
    Ok((serde_json::to_vec(&scanout).map_err(|error| error.to_string())?, planes))
}

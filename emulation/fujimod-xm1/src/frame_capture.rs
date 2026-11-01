//! Capture-only retention of native scanout state and source bytes.
use crate::bridge::{sha256, write};
use crate::run::NodeName;
use crate::trace::InstructionCount;
use fujimod_emulator_devices::exr2::scanout_frame::{Frame, State};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};
use serde_json::value::RawValue;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
#[serde(transparent)]
pub struct ContentDigest(String);

#[derive(Serialize)]
#[serde(transparent)]
struct ScanoutInstance(usize);

#[derive(Serialize)]
#[serde(transparent)]
struct ByteCount(usize);

#[derive(Serialize)]
#[serde(transparent)]
struct ByteOffset(usize);

#[derive(Serialize)]
#[serde(transparent)]
struct NativeSourceAddress(u64);

#[derive(Serialize)]
#[serde(transparent)]
struct NativeScanoutState(Box<RawValue>);

struct NativeFrameState(State);

impl Serialize for NativeFrameState {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_native_state(&self.0, serializer)
    }
}

fn serialize_native_state<S: Serializer>(state: &State, serializer: S) -> Result<S::Ok, S::Error> {
    struct BorrowedState<'a>(&'a State);
    impl Serialize for BorrowedState<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serialize_native_state(self.0, serializer)
        }
    }
    match state {
        State::None => serializer.serialize_none(),
        State::Int(value) => serializer.serialize_i64(*value),
        State::Bool(value) => serializer.serialize_bool(*value),
        State::Str(value) => serializer.serialize_str(value),
        State::Bytes(value) => serializer.serialize_bytes(value),
        State::List(values) => {
            let mut sequence = serializer.serialize_seq(Some(values.len()))?;
            for value in values {
                sequence.serialize_element(&BorrowedState(value))?;
            }
            sequence.end()
        }
        State::Dict(values) => {
            let mut map = serializer.serialize_map(Some(values.len()))?;
            for (key, value) in values {
                map.serialize_entry(key, &BorrowedState(value))?;
            }
            map.end()
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum SourcePlaneKind {
    Main,
    Graphics { layer: PlaneIndex, channel: PlaneIndex },
}

#[derive(Serialize)]
#[serde(transparent)]
struct PlaneIndex(usize);

#[derive(Serialize)]
struct SourcePlane {
    #[serde(flatten)]
    kind: SourcePlaneKind,
    native_source_address: NativeSourceAddress,
    offset: ByteOffset,
    bytes: ByteCount,
}

fn native_count(state: &State, key: &str) -> Result<usize, String> {
    match state.get(key) {
        Some(State::Int(value)) => usize::try_from(*value).map_err(|_| format!("invalid native scanout {key}")),
        _ => Err(format!("missing native scanout {key}")),
    }
}

fn source_plane(state: &State, kind: SourcePlaneKind, offset: usize, bytes: usize) -> Result<SourcePlane, String> {
    let Some(State::Str(source)) = state.get("source") else {
        return Err("missing native scanout source".into());
    };
    let address = source.strip_prefix("0x").ok_or("invalid native scanout source")?;
    let address = u64::from_str_radix(address, 16).map_err(|error| error.to_string())?;
    if bytes == 0 {
        return Err("native scanout returned no source bytes".into());
    }
    Ok(SourcePlane { kind, native_source_address: NativeSourceAddress(address), offset: ByteOffset(offset), bytes: ByteCount(bytes) })
}

fn source_planes(state: &State, raw_bytes: usize) -> Result<Vec<SourcePlane>, String> {
    let graphics = match state.get("graphics") {
        Some(State::List(planes)) => planes.as_slice(),
        None => &[],
        _ => return Err("invalid native scanout graphics".into()),
    };
    let mut graphics_bytes = 0usize;
    for plane in graphics {
        if plane.get("visible") == Some(&State::Bool(false)) {
            continue;
        }
        graphics_bytes = graphics_bytes.checked_add(native_count(plane, "captured_bytes")?)
            .ok_or("native scanout source extent overflow")?;
    }
    let main_bytes = raw_bytes.checked_sub(graphics_bytes).ok_or("native scanout source bytes are incomplete")?;
    let mut planes = Vec::with_capacity(graphics.len() + usize::from(main_bytes != 0));
    if main_bytes != 0 {
        let main = state.get("main").unwrap_or(state);
        planes.push(source_plane(main, SourcePlaneKind::Main, 0, main_bytes)?);
    } else if state.get("main").is_some_and(|main| main != &State::None) {
        return Err("native scanout returned no main source bytes".into());
    }
    let mut offset = main_bytes;
    for plane in graphics {
        if plane.get("visible") == Some(&State::Bool(false)) {
            continue;
        }
        let bytes = native_count(plane, "captured_bytes")?;
        let kind = SourcePlaneKind::Graphics {
            layer: PlaneIndex(native_count(plane, "layer")?),
            channel: PlaneIndex(native_count(plane, "channel")?),
        };
        planes.push(source_plane(plane, kind, offset, bytes)?);
        offset += bytes;
    }
    if planes.is_empty() {
        return Err("native scanout returned no source bytes".into());
    }
    Ok(planes)
}

#[derive(Serialize)]
struct CapturedPixels {
    width: u32,
    height: u32,
    rgb_bytes: ByteCount,
    rgb_sha256: ContentDigest,
}

#[derive(Serialize)]
struct CaptureMetadata<'a> {
    schema: u32,
    node: &'a str,
    instance: ScanoutInstance,
    instructions: InstructionCount,
    pixels: CapturedPixels,
    png: &'a CaptureArtifact,
    source_bytes: &'a CaptureArtifact,
    source_planes: Vec<SourcePlane>,
    native_scanout_state: NativeScanoutState,
    native_frame_state: NativeFrameState,
}

#[derive(Serialize)]
pub struct CaptureArtifact {
    pub path: PathBuf,
    pub sha256: ContentDigest,
    bytes: ByteCount,
}

impl CaptureArtifact {
    fn write(path: PathBuf, bytes: &[u8]) -> Result<Self, String> {
        write(&path, bytes)?;
        Ok(Self { path, sha256: ContentDigest(sha256(bytes)), bytes: ByteCount(bytes.len()) })
    }
}

#[derive(Serialize)]
pub struct ScanoutEvidence {
    metadata: CaptureArtifact,
    source_bytes: CaptureArtifact,
}

pub struct CapturedFrame {
    node: NodeName,
    instance: ScanoutInstance,
    instructions: InstructionCount,
    scanout_state: NativeScanoutState,
    frame_state: NativeFrameState,
    source_planes: Vec<SourcePlane>,
    source_bytes: Vec<u8>,
    pixels: fujimod_recording::Frame,
}

pub struct WrittenCapture {
    pub png: CaptureArtifact,
    pub scanout_evidence: ScanoutEvidence,
}

impl CapturedFrame {
    pub(crate) fn new(node: NodeName, instance: usize, instructions: InstructionCount, state: serde_json::Value, frame: Frame) -> Result<Self, String> {
        let source_planes = source_planes(&frame.state, frame.raw.len())?;
        let rgb = frame.rgb.ok_or("scanout did not return RGB")?;
        let width = u32::try_from(frame.width).map_err(|_| "frame width")?;
        let height = u32::try_from(frame.height).map_err(|_| "frame height")?;
        Ok(Self {
            node, instance: ScanoutInstance(instance), instructions,
            scanout_state: NativeScanoutState(serde_json::value::to_raw_value(&state).map_err(|error| error.to_string())?),
            frame_state: NativeFrameState(frame.state), source_planes, source_bytes: frame.raw,
            pixels: fujimod_recording::Frame::rgb(width, height, rgb)?,
        })
    }
    pub fn write(self, path: &Path) -> Result<WrittenCapture, String> {
        let png = CaptureArtifact::write(path.to_owned(), self.pixels.png()?)?;
        let source_bytes = CaptureArtifact::write(path.with_extension("planes.bin"), &self.source_bytes)?;
        let metadata = CaptureMetadata {
            schema: 1, node: &self.node.0, instance: self.instance, instructions: self.instructions,
            pixels: CapturedPixels {
                width: self.pixels.width(), height: self.pixels.height(),
                rgb_bytes: ByteCount(self.pixels.pixels().len()),
                rgb_sha256: ContentDigest(sha256(self.pixels.pixels())),
            },
            png: &png, source_bytes: &source_bytes, source_planes: self.source_planes,
            native_scanout_state: self.scanout_state, native_frame_state: self.frame_state,
        };
        let metadata = serde_json::to_vec(&metadata).map_err(|error| error.to_string())?;
        let metadata = CaptureArtifact::write(path.with_extension("scanout.json"), &metadata)?;
        Ok(WrittenCapture { png, scanout_evidence: ScanoutEvidence { metadata, source_bytes } })
    }
}

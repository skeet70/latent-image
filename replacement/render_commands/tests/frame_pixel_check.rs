//! Qualifies the owned VGLIB frame step on two guest RAM images, guest physical `[0, 0x4000000)`, captured at the
//! same point of a stock and an owned run.
//! Usage: frame-pixel-check --stock STOCK_RAM --owned OWNED_RAM --record ADDRESS --out DIR
//! Both scenes decode and lower as `render_adapter/frame.rs` `decide` does; the lowered command lists must be equal.
//! The owned role's published buffer, located through `rb_render_frame_record` at ADDRESS, must equal
//! `render_osd` of the owned role's scene byte for byte. Writes `summary.json`, and `host.abgr` and `owned.abgr`
//! (720x480, A,B,G,R bytes) when they exist. Exit 0 when every check passes, 1 when a check fails, 2 on usage or
//! input errors.

#[path = "../src/lib.rs"]
#[allow(dead_code, unused_imports, unused_attributes)]
mod render_commands;

use render_commands::commands::DrawCommand;
use render_commands::producer::CommandBuffer;
use render_commands::resources::{DataAddress, DataRegion, ResourceError, ResourceMemory};
use render_commands::stock_objects::{ObjectPool, PoolError, LISTS_BASE, LIST_BYTES, POOL_BASE, POOL_BYTES};
use render_commands::{RasterError, PANEL_BYTES, PANEL_WIDTH};
use render_commands::vglib_scene::{render_osd, OsdWorkspace, Scene, SceneError};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const RAM_BYTES: usize = 0x0400_0000;
/// The regions `decide` reads: RAM from `0x1000`, and all of RAM at its `0x40000000` alias.
const LOW_BASE: u32 = 0x0000_1000;
const ALIAS_BASE: u32 = 0x4000_0000;
const PHYSICAL_MASK: u32 = 0x3fff_ffff;
const COMMAND_CAPACITY: usize = 96;
/// Stage-0 image objects `O(0,0)` and `O(0,1)`; each holds its buffer address at `+0x1c`.
const IMAGE_OBJECTS: [u32; 2] = [0x008e_2158, 0x008e_215c];
const IMAGE_ADDRESS: u32 = 0x1c;
const RECORD_MAGIC: u32 = u32::from_le_bytes(*b"RBFR");
const RECORD_LAYOUT: u32 = 1;
const RECORD_BYTES: usize = 0x78;

const RASTERIZER: &str = "render_commands::vglib_scene::render_osd";
const RUSTC_VERSION: &str = env!("FRAME_PIXEL_CHECK_RUSTC_VERSION");
const HOST_TRIPLE: &str = env!("FRAME_PIXEL_CHECK_HOST_TRIPLE");
/// SHA-256 of the `sha256sum` lines of the `:module` sources, sorted by path.
const MODULE_SHA256: &str = env!("FRAME_PIXEL_CHECK_MODULE_SHA256");

/// Guest physical RAM `[0, RAM_BYTES)`.
struct RamImage(Vec<u8>);

impl RamImage {
    fn parse(bytes: Vec<u8>) -> Result<Self, InputError> {
        if bytes.len() != RAM_BYTES { return Err(InputError::RamLength(bytes.len())); }
        Ok(Self(bytes))
    }

    fn bytes(&self, address: u32, count: usize) -> Option<&[u8]> {
        let start = (address & PHYSICAL_MASK) as usize;
        self.0.get(start..start.checked_add(count)?)
    }

    fn word(&self, address: u32) -> Option<u32> {
        self.bytes(address, 4).map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn regions(&self) -> [DataRegion<'_>; 2] {
        [
            DataRegion { base: DataAddress(LOW_BASE), bytes: &self.0[LOW_BASE as usize..] },
            DataRegion { base: DataAddress(ALIAS_BASE), bytes: &self.0 },
        ]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RecordAddress(u32);

impl RecordAddress {
    fn parse(text: &str) -> Option<Self> {
        let digits = text.strip_prefix("0x")?;
        if digits.is_empty() || digits.len() > 8 { return None; }
        u32::from_str_radix(digits, 16).ok().map(Self)
    }
}

#[derive(Debug, Eq, PartialEq)]
enum InputError {
    RamLength(usize),
    RecordOutsideRam(RecordAddress),
    RecordMagic(u32),
    RecordLayout(u32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum DecodeRefusal {
    Pool(PoolError),
    Memory(ResourceError),
    Scene(SceneError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BufferState { Free, Drawing, Queued, Scanning }

impl BufferState {
    fn parse(value: u32) -> Option<Self> {
        match value { 0 => Some(Self::Free), 1 => Some(Self::Drawing), 2 => Some(Self::Queued), 3 => Some(Self::Scanning), _ => None }
    }
    fn published(self) -> bool { matches!(self, Self::Queued | Self::Scanning) }
    fn name(self) -> &'static str {
        match self { Self::Free => "free", Self::Drawing => "drawing", Self::Queued => "queued", Self::Scanning => "scanning" }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BufferRefusal {
    BufferState { slot: usize, value: u32 },
    /// No slot is queued or scanning with the record's last sequence.
    Unpublished { last_sequence: u64 },
    Ambiguous { last_sequence: u64 },
    ImageObjectOutsideRam { slot: usize, object: u32 },
    BufferOutsideRam { slot: usize, address: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PublishedBuffer { slot: usize, sequence: u64, state: BufferState, address: u32 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PixelDifference { x: usize, y: usize, host: [u8; 4], owned: [u8; 4] }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PixelComparison { differing: usize, first: Option<PixelDifference> }

#[derive(Clone, Copy, Debug, PartialEq)]
enum PixelBlocker { OwnedScene, Buffer(BufferRefusal), Render(RasterError) }

#[derive(Clone, Debug, Eq, PartialEq)]
struct SceneDifference { index: usize, stock: Option<String>, owned: Option<String> }

struct Check {
    stock: Result<usize, DecodeRefusal>,
    owned: Result<usize, DecodeRefusal>,
    scenes_equal: bool,
    scene_difference: Option<SceneDifference>,
    buffer: Result<PublishedBuffer, BufferRefusal>,
    pixels: Result<PixelComparison, PixelBlocker>,
    host_pixels: Option<Vec<u8>>,
    owned_pixels: Option<Vec<u8>>,
}

impl Check {
    fn passed(&self) -> bool {
        self.stock.is_ok() && self.owned.is_ok() && self.scenes_equal
            && matches!(self.pixels, Ok(PixelComparison { differing: 0, .. }))
    }
}

/// `decide`'s pool parse, scene decode and lowering into a 96-command buffer.
fn decode<'r>(ram: &'r RamImage, regions: &'r [DataRegion<'r>]) -> Result<Vec<DrawCommand<'r>>, DecodeRefusal> {
    let pool_start = POOL_BASE.0 as usize;
    let lists_start = LISTS_BASE.0 as usize;
    let pool = ObjectPool::parse(&ram.0[pool_start..pool_start + POOL_BYTES], &ram.0[lists_start..lists_start + LIST_BYTES])
        .map_err(DecodeRefusal::Pool)?;
    let memory = ResourceMemory::new(regions).map_err(DecodeRefusal::Memory)?;
    let scene = Scene::decode(&memory, &pool).map_err(DecodeRefusal::Scene)?;
    let mut storage = [DrawCommand::ResetClip; COMMAND_CAPACITY];
    let mut buffer = CommandBuffer::new(&mut storage);
    scene.lower(&mut buffer).map_err(DecodeRefusal::Scene)?;
    let count = buffer.draw_list().commands().len();
    Ok(storage[..count].to_vec())
}

fn describe(command: &DrawCommand<'_>) -> String {
    match *command {
        DrawCommand::ScaledImage { dest, source } => format!("ScaledImage dest=({}, {}, {}, {}) source={}x{} stride={}",
            dest.x(), dest.y(), dest.width(), dest.height(), source.width(), source.height(), source.stride()),
        DrawCommand::ScaledShape { dest, gvff, recolour } => format!("ScaledShape dest=({}, {}, {}, {}) gvff_bytes={} recoloured={}",
            dest.x(), dest.y(), dest.width(), dest.height(), gvff.len(), recolour.is_some()),
        DrawCommand::Clip(rect) => format!("Clip {rect:?}"),
        DrawCommand::ResetClip => "ResetClip".into(),
        _ => "non-scene command".into(),
    }
}

fn scene_difference(stock: &[DrawCommand<'_>], owned: &[DrawCommand<'_>]) -> Option<SceneDifference> {
    let index = (0..stock.len().max(owned.len())).find(|&index| stock.get(index) != owned.get(index))?;
    Some(SceneDifference { index, stock: stock.get(index).map(describe), owned: owned.get(index).map(describe) })
}

struct FrameRecord { last_sequence: u64, buffer_sequence: [u64; 2], buffer_state: [u32; 2] }

impl FrameRecord {
    fn read(ram: &RamImage, address: RecordAddress) -> Result<Self, InputError> {
        let bytes = ram.bytes(address.0, RECORD_BYTES).ok_or(InputError::RecordOutsideRam(address))?;
        let word = |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let double = |offset: usize| u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
        if word(0) != RECORD_MAGIC { return Err(InputError::RecordMagic(word(0))); }
        if word(4) != RECORD_LAYOUT { return Err(InputError::RecordLayout(word(4))); }
        Ok(Self { last_sequence: double(0x10), buffer_sequence: [double(0x20), double(0x28)], buffer_state: [word(0x30), word(0x34)] })
    }

    /// The slot queued or scanning with the last owned sequence, and its buffer named by its stage-0 image object.
    fn published(&self, ram: &RamImage) -> Result<PublishedBuffer, BufferRefusal> {
        let mut found = None;
        for slot in 0..2 {
            let value = self.buffer_state[slot];
            let state = BufferState::parse(value).ok_or(BufferRefusal::BufferState { slot, value })?;
            if self.last_sequence == 0 || !state.published() || self.buffer_sequence[slot] != self.last_sequence { continue; }
            if found.is_some() { return Err(BufferRefusal::Ambiguous { last_sequence: self.last_sequence }); }
            found = Some((slot, state));
        }
        let (slot, state) = found.ok_or(BufferRefusal::Unpublished { last_sequence: self.last_sequence })?;
        let object = ram.word(IMAGE_OBJECTS[slot]).ok_or(BufferRefusal::ImageObjectOutsideRam { slot, object: IMAGE_OBJECTS[slot] })?;
        let address = ram.word(object.wrapping_add(IMAGE_ADDRESS)).ok_or(BufferRefusal::ImageObjectOutsideRam { slot, object })?;
        ram.bytes(address, PANEL_BYTES).ok_or(BufferRefusal::BufferOutsideRam { slot, address })?;
        Ok(PublishedBuffer { slot, sequence: self.last_sequence, state, address })
    }
}

fn host_render(commands: &[DrawCommand<'_>]) -> Result<Vec<u8>, RasterError> {
    let mut pixels = vec![0u8; PANEL_BYTES];
    render_osd(commands, &mut pixels, &mut Box::new(OsdWorkspace::new()))?;
    Ok(pixels)
}

fn compare_pixels(host: &[u8], owned: &[u8]) -> PixelComparison {
    let mut comparison = PixelComparison { differing: 0, first: None };
    for (index, (host, owned)) in host.chunks_exact(4).zip(owned.chunks_exact(4)).enumerate() {
        if host == owned { continue; }
        comparison.differing += 1;
        comparison.first.get_or_insert(PixelDifference {
            x: index % PANEL_WIDTH, y: index / PANEL_WIDTH, host: host.try_into().unwrap(), owned: owned.try_into().unwrap(),
        });
    }
    comparison
}

fn check(stock: &RamImage, owned: &RamImage, record: RecordAddress) -> Result<Check, InputError> {
    let record = FrameRecord::read(owned, record)?;
    let stock_regions = stock.regions();
    let owned_regions = owned.regions();
    let stock_scene = decode(stock, &stock_regions);
    let owned_scene = decode(owned, &owned_regions);
    let (scenes_equal, scene_difference) = match (&stock_scene, &owned_scene) {
        (Ok(stock), Ok(owned)) => { let difference = scene_difference(stock, owned); (difference.is_none(), difference) }
        _ => (false, None),
    };
    let buffer = record.published(owned);
    let owned_pixels = buffer.ok().map(|buffer| owned.bytes(buffer.address, PANEL_BYTES).unwrap().to_vec());
    let host_pixels = match &owned_scene {
        Ok(commands) => Some(host_render(commands)),
        Err(_) => None,
    };
    let pixels = match (&owned_scene, &buffer, &host_pixels, &owned_pixels) {
        (Err(_), _, _, _) => Err(PixelBlocker::OwnedScene),
        (_, _, Some(Err(error)), _) => Err(PixelBlocker::Render(*error)),
        (_, Err(refusal), _, _) => Err(PixelBlocker::Buffer(*refusal)),
        (Ok(_), Ok(_), Some(Ok(host)), Some(owned)) => Ok(compare_pixels(host, owned)),
        _ => unreachable!("an owned scene renders and a located buffer is read"),
    };
    Ok(Check {
        stock: stock_scene.as_ref().map(Vec::len).map_err(|refusal| *refusal),
        owned: owned_scene.as_ref().map(Vec::len).map_err(|refusal| *refusal),
        scenes_equal,
        scene_difference,
        buffer,
        pixels,
        host_pixels: host_pixels.and_then(Result::ok),
        owned_pixels,
    })
}

fn quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            character if (character as u32) < 0x20 => write!(out, "\\u{:04x}", character as u32).unwrap(),
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

fn optional(text: &Option<String>) -> String { text.as_deref().map_or("null".into(), quoted) }

fn role_json(outcome: &Result<usize, DecodeRefusal>) -> String {
    match outcome {
        Ok(commands) => format!("{{\"decoded\": true, \"commands\": {commands}}}"),
        Err(refusal) => {
            let (stage, detail) = match refusal {
                DecodeRefusal::Pool(error) => ("pool", format!("{error:?}")),
                DecodeRefusal::Memory(error) => ("memory", format!("{error:?}")),
                DecodeRefusal::Scene(error) => ("scene", format!("{error:?}")),
            };
            format!("{{\"decoded\": false, \"refusal\": {{\"stage\": \"{stage}\", \"detail\": {}}}}}", quoted(&detail))
        }
    }
}

fn summary(check: &Check, stock: &Path, owned: &Path, record: RecordAddress) -> String {
    let mut json = String::new();
    writeln!(json, "{{\n  \"schema\": 1,").unwrap();
    writeln!(json, "  \"inputs\": {{\"stock\": {}, \"owned\": {}, \"record\": \"0x{:08x}\"}},",
        quoted(&stock.display().to_string()), quoted(&owned.display().to_string()), record.0).unwrap();
    writeln!(json, "  \"roles\": {{\"stock\": {}, \"owned\": {}}},", role_json(&check.stock), role_json(&check.owned)).unwrap();
    writeln!(json, "  \"scenes_equal\": {},", check.scenes_equal).unwrap();
    let difference = check.scene_difference.as_ref().map_or("null".into(), |difference| format!(
        "{{\"index\": {}, \"stock\": {}, \"owned\": {}}}", difference.index, optional(&difference.stock), optional(&difference.owned)));
    writeln!(json, "  \"first_scene_difference\": {difference},").unwrap();
    let buffer = match check.buffer {
        Ok(buffer) => format!("{{\"slot\": {}, \"sequence\": {}, \"state\": \"{}\", \"address\": \"0x{:08x}\"}}",
            buffer.slot, buffer.sequence, buffer.state.name(), buffer.address),
        Err(refusal) => format!("{{\"refusal\": {}}}", quoted(&format!("{refusal:?}"))),
    };
    writeln!(json, "  \"owned_buffer\": {buffer},").unwrap();
    match check.pixels {
        Ok(comparison) => {
            writeln!(json, "  \"pixels_compared\": true,\n  \"pixels_equal\": {},\n  \"differing_pixels\": {},",
                comparison.differing == 0, comparison.differing).unwrap();
            let first = comparison.first.map_or("null".into(), |pixel| format!(
                "{{\"x\": {}, \"y\": {}, \"host\": {:?}, \"owned\": {:?}}}", pixel.x, pixel.y, pixel.host, pixel.owned));
            writeln!(json, "  \"first_differing_pixel\": {first},").unwrap();
        }
        Err(blocker) => {
            writeln!(json, "  \"pixels_compared\": false,\n  \"pixel_blocker\": {},\n  \"pixels_equal\": false,\n  \"differing_pixels\": null,\n  \"first_differing_pixel\": null,",
                quoted(&format!("{blocker:?}"))).unwrap();
        }
    }
    writeln!(json, "  \"rasterizer\": {{\"implementation\": {}, \"host_target\": {}, \"rustc\": {}, \"module_sources_sha256\": {}, \"vertices\": {}, \"strokes\": {}}},",
        quoted(RASTERIZER), quoted(HOST_TRIPLE), quoted(RUSTC_VERSION), quoted(MODULE_SHA256), OsdWorkspace::VERTICES, OsdWorkspace::STROKES).unwrap();
    writeln!(json, "  \"passed\": {}\n}}", check.passed()).unwrap();
    json
}

struct Arguments { stock: PathBuf, owned: PathBuf, record: RecordAddress, out: PathBuf }

fn arguments() -> Option<Arguments> {
    let mut values: [Option<String>; 4] = Default::default();
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let index = ["--stock", "--owned", "--record", "--out"].iter().position(|name| *name == flag)?;
        if values[index].replace(args.next()?).is_some() { return None; }
    }
    let [stock, owned, record, out] = values;
    Some(Arguments { stock: stock?.into(), owned: owned?.into(), record: RecordAddress::parse(&record?)?, out: out?.into() })
}

fn input_error(message: String) -> ! {
    eprintln!("frame-pixel-check: {message}");
    std::process::exit(2)
}

fn read_ram(path: &Path) -> RamImage {
    let bytes = std::fs::read(path).unwrap_or_else(|error| input_error(format!("reading {}: {error}", path.display())));
    RamImage::parse(bytes).unwrap_or_else(|error| input_error(format!("{}: {error:?}", path.display())))
}

fn main() {
    let Some(arguments) = arguments() else {
        input_error("usage: frame-pixel-check --stock STOCK_RAM --owned OWNED_RAM --record 0xADDRESS --out DIR".into())
    };
    let stock = read_ram(&arguments.stock);
    let owned = read_ram(&arguments.owned);
    let check = check(&stock, &owned, arguments.record).unwrap_or_else(|error| input_error(format!("{error:?}")));
    let directory = &arguments.out;
    std::fs::create_dir_all(directory).unwrap_or_else(|error| input_error(format!("creating {}: {error}", directory.display())));
    let write = |name: &str, bytes: &[u8]| {
        let path = directory.join(name);
        std::fs::write(&path, bytes).unwrap_or_else(|error| input_error(format!("writing {}: {error}", path.display())));
    };
    if let Some(pixels) = &check.host_pixels { write("host.abgr", pixels); }
    if let Some(pixels) = &check.owned_pixels { write("owned.abgr", pixels); }
    write("summary.json", summary(&check, &arguments.stock, &arguments.owned, arguments.record).as_bytes());
    std::process::exit(if check.passed() { 0 } else { 1 });
}

#[cfg(test)]
mod tests {
    use super::*;
    use render_commands::commands::{PixelOrder, PixelSource, SceneRect};
    use render_commands::stock_objects::VglibObject;
    use render_commands::vglib_scene::ObjectRefusal;

    // Guest layout of `vglib_scene_tests.rs`: the image class table, group sentinels and counts, the node pool,
    // display words and the OpenVG handle chain from context 0x88dd2c.
    const IMAGE_OPS: u32 = 0x008e_21fc;
    const IMAGE_DRAW: u32 = 0x006d_87dc;
    const BASE_GETTERS: [(u32, u32); 12] = [
        (0x04, 0x006d_8158), (0x0c, 0x006d_8188), (0x14, 0x006d_81bc), (0x1c, 0x006d_81fc),
        (0x24, 0x006d_8264), (0x2c, 0x006d_82a8), (0x3c, 0x006d_8308), (0x44, 0x006d_8338),
        (0x4c, 0x006d_8378), (0x5c, 0x006d_83f8), (0x6c, 0x006d_8428), (0x84, 0x006d_84b8),
    ];
    const SENTINELS: u32 = 0x008f_a2a4;
    const COUNTS: u32 = 0x008f_a3c4;
    const NODE: u32 = 0x008e_22a4 + 3 * 16;
    const VG_CONTEXT: u32 = 0x0088_dd2c;
    const HIGH: u32 = 0x0100_0000;
    const MANAGER: u32 = HIGH + 0xf000;
    const REGISTRY: u32 = HIGH + 0xf020;
    const HEADER: u32 = HIGH + 0xf040;
    const HANDLE_POOL: u32 = HIGH + 0xf060;
    const STATES: u32 = HIGH + 0xf100;
    /// Image object; its VGImage record is pool element 0 at `+0x200`, its pixels at `+0x280`.
    const OBJECT: u32 = HIGH;
    const RECORD: u32 = OBJECT + 0x200;
    const PIXELS: u32 = OBJECT + 0x280;
    const IMAGE_X: usize = 100;
    const IMAGE_Y: usize = 40;
    /// Opaque 2 x 2 image, `VG_sRGBA_8888` words stored little-endian.
    const IMAGE: [u8; 16] = [0xff, 0x30, 0x20, 0x10, 0xff, 0x60, 0x50, 0x40, 0xff, 0x90, 0x80, 0x70, 0xff, 0xc0, 0xb0, 0xa0];
    const FRAME_RECORD: u32 = 0x0200_0000;
    const SLOT_OBJECT: u32 = 0x4210_0000;
    const BUFFER: u32 = 0x4220_0000;

    struct Guest(Vec<u8>);
    impl Guest {
        fn put(&mut self, address: u32, bytes: &[u8]) {
            let start = (address & PHYSICAL_MASK) as usize;
            self.0[start..start + bytes.len()].copy_from_slice(bytes);
        }
        fn put32(&mut self, address: u32, value: u32) { self.put(address, &value.to_le_bytes()); }
        fn putf(&mut self, address: u32, value: f32) { self.put32(address, value.to_bits()); }
    }

    /// One image in group 0 at (IMAGE_X, IMAGE_Y); a frame record whose slot 1 is queued with the last sequence.
    fn one_image_guest() -> Guest {
        let mut guest = Guest(vec![0; RAM_BYTES]);
        for (offset, getter) in BASE_GETTERS { guest.put32(IMAGE_OPS + offset, getter); }
        guest.put32(IMAGE_OPS + 0xa0, IMAGE_DRAW);
        for group in 1..24 {
            let sentinel = SENTINELS + group * 12;
            guest.put32(sentinel, sentinel);
            guest.put32(sentinel + 4, sentinel);
        }
        guest.put32(SENTINELS, NODE);
        guest.put32(SENTINELS + 4, NODE);
        guest.put32(NODE, SENTINELS);
        guest.put32(NODE + 4, SENTINELS);
        guest.put32(NODE + 8, OBJECT);
        guest.put32(NODE + 0xc, 1);
        guest.put(COUNTS, &[1]);

        guest.put32(OBJECT + 0xc0, IMAGE_OPS);
        guest.put(OBJECT + 0x47, &[1]);
        guest.putf(OBJECT, IMAGE_X as f32);
        guest.putf(OBJECT + 4, IMAGE_Y as f32);
        guest.putf(OBJECT + 8, 2.0);
        guest.putf(OBJECT + 0xc, 2.0);
        guest.putf(OBJECT + 0x14, 1.0);
        guest.putf(OBJECT + 0x18, 1.0);
        guest.put32(OBJECT + 0xc4, 0x2000_0000 | 1 << 25);
        guest.put32(VG_CONTEXT + 0x1c, MANAGER);
        guest.put32(MANAGER + 0x1c, REGISTRY);
        guest.put32(REGISTRY + 0xc, HEADER);
        guest.put32(HEADER + 4, HANDLE_POOL);
        guest.put32(HANDLE_POOL, 2);
        guest.put32(HANDLE_POOL + 8, RECORD);
        guest.put32(HANDLE_POOL + 0xc, 16);
        guest.put32(HANDLE_POOL + 0x20, 0x400);
        guest.put32(HANDLE_POOL + 0x30, STATES);
        guest.put(STATES, &0x2200u16.to_le_bytes());
        guest.put32(RECORD + 0x14, 1);
        guest.put(RECORD + 0x3c, &2u16.to_le_bytes());
        guest.put(RECORD + 0x40, &2u16.to_le_bytes());
        guest.put(RECORD + 0x4c, &8u16.to_le_bytes());
        guest.put32(RECORD + 0x68, PIXELS);
        guest.put(PIXELS, &IMAGE);

        guest.put32(FRAME_RECORD, RECORD_MAGIC);
        guest.put32(FRAME_RECORD + 4, RECORD_LAYOUT);
        guest.put(FRAME_RECORD + 0x10, &7u64.to_le_bytes());
        guest.put(FRAME_RECORD + 0x20, &6u64.to_le_bytes());
        guest.put(FRAME_RECORD + 0x28, &7u64.to_le_bytes());
        guest.put32(FRAME_RECORD + 0x30, 0);
        guest.put32(FRAME_RECORD + 0x34, 2);
        guest.put32(IMAGE_OBJECTS[1], SLOT_OBJECT);
        guest.put32(SLOT_OBJECT + IMAGE_ADDRESS, BUFFER);
        guest
    }

    /// The owned guest's published buffer: the unclipped image draw rendered as the frame step renders it.
    fn publish(guest: &mut Guest) {
        let source = PixelSource::new(2, 2, 8, PixelOrder::Abgr, &IMAGE).unwrap();
        let commands = [DrawCommand::ResetClip, DrawCommand::ScaledImage { dest: SceneRect::new(IMAGE_X as f32, IMAGE_Y as f32, 2.0, 2.0).unwrap(), source }];
        let mut pixels = vec![0; PANEL_BYTES];
        render_osd(&commands, &mut pixels, &mut Box::new(OsdWorkspace::new())).unwrap();
        guest.put(BUFFER, &pixels);
    }

    fn pair() -> (Guest, Guest) {
        let stock = one_image_guest();
        let mut owned = one_image_guest();
        publish(&mut owned);
        (stock, owned)
    }

    fn run(stock: Guest, owned: Guest) -> Check {
        check(&RamImage::parse(stock.0).unwrap(), &RamImage::parse(owned.0).unwrap(), RecordAddress(FRAME_RECORD)).unwrap()
    }

    #[test]
    fn equal_scenes_and_the_matching_published_buffer_pass() {
        let (stock, owned) = pair();
        let check = run(stock, owned);
        assert_eq!(check.stock, Ok(2));
        assert_eq!(check.owned, Ok(2));
        assert!(check.scenes_equal);
        assert_eq!(check.buffer.map(|buffer| (buffer.slot, buffer.state)), Ok((1, BufferState::Queued)));
        assert_eq!(check.pixels, Ok(PixelComparison { differing: 0, first: None }));
        assert!(check.passed());
    }

    #[test]
    fn a_changed_buffer_byte_fails_at_its_pixel() {
        let (stock, mut owned) = pair();
        let offset = BUFFER + (((IMAGE_Y as u32 + 1) * PANEL_WIDTH as u32 + IMAGE_X as u32 + 1) * 4) + 2;
        let original = owned.0[(offset & PHYSICAL_MASK) as usize];
        owned.put(offset, &[original ^ 0x01]);
        let check = run(stock, owned);
        assert!(check.scenes_equal);
        let comparison = check.pixels.unwrap();
        assert_eq!(comparison.differing, 1);
        let first = comparison.first.unwrap();
        assert_eq!((first.x, first.y), (IMAGE_X + 1, IMAGE_Y + 1));
        assert_eq!(first.owned[2], original ^ 0x01);
        assert_eq!(first.host[2], original);
        assert_eq!((first.host[0], first.host[1], first.host[3]), (first.owned[0], first.owned[1], first.owned[3]));
        assert!(!check.passed());
    }

    #[test]
    fn a_changed_stock_scene_fails_scene_equality_at_the_differing_command() {
        let (mut stock, owned) = pair();
        stock.put(PIXELS + 5, &[0x61]);
        let check = run(stock, owned);
        assert_eq!(check.stock, Ok(2));
        assert!(!check.scenes_equal);
        assert_eq!(check.scene_difference.as_ref().map(|difference| difference.index), Some(1));
        assert_eq!(check.pixels.as_ref().map(|comparison| comparison.differing), Ok(0));
        assert!(!check.passed());
    }

    #[test]
    fn an_unrepresentable_owned_object_fails_with_its_refusal() {
        let (stock, mut owned) = pair();
        owned.put32(OBJECT + 0xc0, 0x0012_3456);
        let check = run(stock, owned);
        assert_eq!(check.stock, Ok(2));
        assert_eq!(check.owned, Err(DecodeRefusal::Scene(SceneError::Object {
            object: VglibObject(DataAddress(OBJECT)), refusal: ObjectRefusal::UnknownClass(0x0012_3456) })));
        assert!(!check.scenes_equal);
        assert_eq!(check.pixels, Err(PixelBlocker::OwnedScene));
        assert!(!check.passed());
    }

    #[test]
    fn a_record_without_a_published_last_sequence_blocks_the_pixel_check() {
        let (stock, mut owned) = pair();
        owned.put32(FRAME_RECORD + 0x34, 1);
        let check = run(stock, owned);
        assert!(check.scenes_equal);
        assert_eq!(check.pixels, Err(PixelBlocker::Buffer(BufferRefusal::Unpublished { last_sequence: 7 })));
        assert!(!check.passed());
    }
}

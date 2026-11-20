//! Typed decode of VGLIB's object groups as the stage-0 frame step renders them, and their lowering into
//! scaled draw commands.
//!
//! Listing addresses are in `lz/id4.dis` (image 4 at `0x0061c400`) unless prefixed `img6:` (`decode/img6.dis`).
//!
//! Frame step `0x6d7920` renders through `0x6d724c` (`6d72a4-6d72b4`): `0x6d92d8(0x6d7324, &filter, 0xff)` walks the
//! group sentinels from group 23 down to group 0 (`6d92f8-6d9334`). For each group `0x6d909c` copies the group list
//! into a temporary list (`0x6d8fc4`, nodes from `0x6d8f28`) and calls the visitor with each node's object word
//! `+8` in list order (`6d90f0-6d9114`). The visitor `0x6d7324` stops the whole walk on a null object
//! (`6d7330-6d7338`), skips objects whose ops `+0x04` returns 0 (`6d733c-6d7354`), and otherwise calls ops
//! `+0xa0` and increments `0x8e2098+0x158` (`6d73e0-6d7414`). Away from frame modes 8 and 10 the filter is -1
//! (`6d7a90-6d7aa0`, `6d7b0c-6d7b10`), which draws every visible object (`6d7368-6d7378`).

use super::commands::{DrawCommand, DrawList, PixelOrder, PixelSource, RecolourBlock, SceneRect};
use super::producer::CommandBuffer;
use super::raster::{RasterError, Surface, VectorWorkspace};
use super::resources::{DataAddress, ResourceError, ResourceMemory};
use super::shape_assets::{Gvff, ShapeError};
use super::stock_objects::{Bank, ObjectKind, ObjectPool, SlotIndex, VglibObject, BANK_COUNT};
use super::vector_raster::{CurveAccuracy, DashBudget, SampleGrid, StrokePrimitive, Vertex};
use super::{PanelPoint, PanelRect, SurfaceId, PANEL_HEIGHT, PANEL_WIDTH};

pub const GROUP_COUNT: usize = 24;
/// Group sentinels `{next, prev, object}`, 12 bytes each (`0x6d920c`: `6d9224-6d9230`).
pub const SENTINEL_BASE: DataAddress = DataAddress(0x008f_a2a4);
/// Per-group member counts, one byte each (`0x6d920c`: `6d923c-6d9250`; `0x6d925c`: `6d9290-6d92a4`).
pub const COUNT_BASE: DataAddress = DataAddress(0x008f_a3c4);
/// List node pool, zeroed by `0x6d8e6c` (`6d8e74-6d8e84`).
pub const NODE_POOL: DataAddress = DataAddress(0x008e_22a4);
pub const NODE_BYTES: usize = 16;
pub const NODE_POOL_BYTES: usize = 0x18000;
pub const NODE_COUNT: usize = NODE_POOL_BYTES / NODE_BYTES;
/// `0x8e2098+0x80`, the frame step's render mode (`6d7a88-6d7aa0`).
pub const FRAME_MODE: DataAddress = DataAddress(0x008e_2118);
/// `0x8fa5dc+0x20`, the selector of `0x6dd884` and `0x6dda20` (`6dd88c-6dd89c`, `6dda28-6dda38`).
pub const DISPLAY_MODE: DataAddress = DataAddress(0x008f_a5fc);
/// `0x8fa5dc+0x206`, returned by `0x6dda00` (`6dda08-6dda10`).
pub const DISPLAY_MIRROR: DataAddress = DataAddress(0x008f_a7e2);

/// Ops table of the 0xd4-byte image class (`0x6d8cf4`: `6d8d18-6d8d48`); text objects use it too.
pub const IMAGE_OPS: u32 = 0x008e_21fc;
/// Ops table of the 0x14c-byte GVFF shape class (`0x6da610`: `6da62c-6da68c`).
pub const SHAPE_OPS: u32 = 0x008f_a484;
pub const PATH_LIST_OPS: u32 = 0x008f_a3dc;
pub const PLANE_EVENT_OPS: u32 = 0x008f_a530;

const IMAGE_DRAW: u32 = 0x006d_87dc;
const SHAPE_DRAW: u32 = 0x006d_a0ac;
/// Shape-type getter the shape constructor stores at object `+0xc8` (`6da668-6da670`; body `6d9fac-6d9fb4`).
const SHAPE_TYPE_GETTER: u32 = 0x006d_9fac;
/// Base getters at `0x7cfc94` (`7cfc94-7cfd38`) that both class tables keep: the constructors copy the 0xa8-byte
/// table and patch only `+0xa0`/`+0xa4` (`6d8d20-6d8d44`, `6da634-6da658`).
const BASE_GETTERS: [(usize, u32); 12] = [
    (0x04, 0x006d_8158), (0x0c, 0x006d_8188), (0x14, 0x006d_81bc), (0x1c, 0x006d_81fc),
    (0x24, 0x006d_8264), (0x2c, 0x006d_82a8), (0x3c, 0x006d_8308), (0x44, 0x006d_8338),
    (0x4c, 0x006d_8378), (0x5c, 0x006d_83f8), (0x6c, 0x006d_8428), (0x84, 0x006d_84b8),
];
const OPS_BYTES: usize = 0xa8;
const BASE_BYTES: usize = 0xc4;
const IMAGE_BYTES: usize = 0xd4;
const SHAPE_BYTES: usize = 0x14c;
const VG_IMAGE_BYTES: usize = 0x6c;

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct GroupIndex(u8);
impl GroupIndex {
    pub const fn new(index: u8) -> Option<Self> { if (index as usize) < GROUP_COUNT { Some(Self(index)) } else { None } }
    pub const fn index(self) -> u8 { self.0 }
    const fn sentinel(self) -> u32 { SENTINEL_BASE.0 + self.0 as u32 * 12 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SceneError {
    Memory(ResourceError),
    /// Frame modes 8 and 10 render twice with output-mode filters 0 and 3 (`6d7b40`, `6d7c48`).
    DualRender(u32),
    /// A `next` word that is neither this group's sentinel nor a node start inside the node pool.
    NodeOutsidePool { group: GroupIndex, node: DataAddress },
    /// A listed node whose in-use word `+0xc` (set to 1 by `0x6d8f28`, `6d8f6c-6d8f70`) is zero.
    FreeNode { group: GroupIndex, node: DataAddress },
    /// A node reached twice: a cycle within this group or a node shared with an earlier group.
    RepeatedNode { group: GroupIndex, node: DataAddress },
    /// A null object word, which ends the stock walk (`6d7330-6d7338`, `6d9324-6d9328`).
    NullObject { group: GroupIndex, node: DataAddress },
    CountMismatch { group: GroupIndex, count: u8, listed: usize },
    /// Fewer free nodes than the longest group: `0x6d8fc4` fails the copy and the group is not drawn (`6d90cc-6d90d8`).
    NodeCapacity { free: usize, needed: usize },
    Object { object: VglibObject, refusal: ObjectRefusal },
    Capacity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectRefusal {
    Memory(ResourceError),
    UnknownClass(u32),
    /// Class `0x8fa3dc` (`0x6d9a4c`); contour paths drawn by `0x6d9568`.
    PathList,
    /// Class `0x8fa530` (`0x6db3f0`); contour paths drawn by `0x6dae8c`.
    PlaneEvent,
    /// The class table no longer holds the base getters or the class draw method.
    OpsTable,
    NonFinite,
    /// A nonzero parameter rotates through `0x6db9a4` and `0x858af4` (`6d8aa0-6d8ab0`, `6da394-6da3ac`).
    Rotation,
    /// Display mirror byte and object mirror byte both 1 (`6d8b80-6d8bc8`, `6da340-6da388`).
    Mirror,
    /// Matrix byte 1 loads the object's own matrix from `+0x50` (`6d8a7c-6d8a98`, `6da2e8-6da304`).
    Matrix,
    /// Blend byte 1 enables the colour transform (`6d89c8-6d8a24`, `6da24c-6da29c`).
    ColourTransform,
    /// Surface size or factors from `0x6dd884`/`0x6dda20` other than 720 x 480 at 1.0.
    SurfaceScale { display_mode: u32, output_mode: u8 },
    NonIntegralGeometry,
    /// Created size words `+8`/`+0xc` differ from the VGImage extent.
    ImageSize,
    /// The image handle does not resolve to a live image (`0x82b428`).
    ImageHandle(u32),
    ImageFormat,
    ImageStorage,
    ShapeMethod,
    ShapeOwnerMissing,
    ShapeOwnerAmbiguous,
    ShapeOwnerKind(ObjectKind),
    ShapeContentMissing,
    Shape(ShapeError),
    /// Bounds words `+0x24..+0x30` differ from the owner's GVFF header bounds.
    ShapeBounds,
    /// Bounds with `x0 > x1` or `y0 < y1`, whose stock placement does not cover the object rectangle.
    ShapeOrientation,
    RecolourBlock,
}

fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]) }
fn half(bytes: &[u8], offset: usize) -> u16 { u16::from_le_bytes([bytes[offset], bytes[offset + 1]]) }
fn float(bytes: &[u8], offset: usize) -> Result<f32, ObjectRefusal> {
    let value = f32::from_bits(word(bytes, offset));
    if value.is_finite() { Ok(value) } else { Err(ObjectRefusal::NonFinite) }
}
fn read_word(memory: &ResourceMemory<'_>, address: DataAddress) -> Result<u32, ResourceError> {
    Ok(word(memory.bytes(address, 4)?, 0))
}

/// Globals the draw methods read besides the object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplayState { pub frame_mode: u32, pub display_mode: u32, pub mirror: bool }
impl DisplayState {
    pub fn read(memory: &ResourceMemory<'_>) -> Result<Self, ResourceError> {
        Ok(Self {
            frame_mode: read_word(memory, FRAME_MODE)?,
            display_mode: read_word(memory, DISPLAY_MODE)?,
            mirror: memory.bytes(DISPLAY_MIRROR, 1)?[0] == 1,
        })
    }

    /// Surface size from `0x6dd884` (`6dd88c-6dd994`) and factors from `0x6dda20` (`6dda28-6ddaa4`).
    pub fn surface(self, output_mode: u8) -> SurfaceScale {
        let (width, height) = match self.display_mode {
            3 => (360.0, 240.0),
            5 => (960.0, 544.0),
            6 => (1280.0, 720.0),
            7 => (640.0, 480.0),
            8 if output_mode == 1 => (960.0, 544.0),
            _ => (720.0, 480.0),
        };
        let factor = match self.display_mode {
            3 | 5 => 0.5,
            6 => f32::from_bits(0x3f2a_aaab),
            _ => 1.0,
        };
        SurfaceScale { width, height, x: factor, y: factor }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceScale { pub width: f32, pub height: f32, pub x: f32, pub y: f32 }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScenePoint { pub x: f32, pub y: f32 }
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneSize { pub width: f32, pub height: f32 }
/// Target size divided by created size (`0x6dbf14`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScaleWords { pub x: f32, pub y: f32 }
/// Integer clip words `+0x34..+0x40`, top-left object space (`0x6d8350`, converted at `6d8c2c-6d8c98`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectClip { pub x: i32, pub y: i32, pub width: i32, pub height: i32 }

/// Byte `+0x45` (`0x6d82a8`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlendMode {
    /// 1: colour transform on.
    ColourTransform,
    /// 2: `VG_BLEND_MULTIPLY` before a shape's per-item blend (`6da2a4-6da2ac`); source-over for images (`6d8a2c-6d8a4c`).
    Multiply,
    /// Any other value: source-over.
    Normal,
}

/// Fields of the 0xc4-byte base object (`0x6d86e0`) that the image and shape draws read through the base getters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BaseFields {
    /// Byte `+0x47` nonzero (`0x6d8158`; the visitor and both draws compare with 0).
    pub visible: bool,
    /// `+0x00/+0x04` (`0x6d81fc`).
    pub position: ScenePoint,
    /// `+0x08/+0x0c` (`0x6d8264`).
    pub size: SceneSize,
    /// `+0x14/+0x18` (`0x6d81bc`).
    pub scale: ScaleWords,
    /// `+0x1c` in degrees (`0x6d8188`; radians at `6db9c0-6db9d0`).
    pub parameter: f32,
    /// Enable byte `+0x46` nonzero (`0x6d8338`) and rectangle (`0x6d8378`).
    pub clip: Option<ObjectClip>,
    /// Byte `+0x48` (`0x6d8308`); read only on the rotated path (`6d8ab4-6d8ac8`, `6dbba4-6dbbb8`).
    pub coordinate_mode: u8,
    /// Byte `+0x49`, the group recorded at insertion (`0x6d83c8`).
    pub group: u8,
    /// Byte `+0x4a` equal to 1 (`0x6d83f8`).
    pub matrix: bool,
    /// Byte `+0x4b` equal to 1 (`0x6d8428`).
    pub mirror: bool,
    /// Byte `+0x4c` (`0x6d84b8`).
    pub output_mode: u8,
    pub blend: BlendMode,
}
impl BaseFields {
    fn parse(bytes: &[u8]) -> Result<Self, ObjectRefusal> {
        let int = |offset| word(bytes, offset) as i32;
        Ok(Self {
            visible: bytes[0x47] != 0,
            position: ScenePoint { x: float(bytes, 0)?, y: float(bytes, 4)? },
            size: SceneSize { width: float(bytes, 8)?, height: float(bytes, 0xc)? },
            scale: ScaleWords { x: float(bytes, 0x14)?, y: float(bytes, 0x18)? },
            parameter: float(bytes, 0x1c)?,
            clip: if bytes[0x46] != 0 { Some(ObjectClip { x: int(0x34), y: int(0x38), width: int(0x3c), height: int(0x40) }) } else { None },
            coordinate_mode: bytes[0x48],
            group: bytes[0x49],
            matrix: bytes[0x4a] == 1,
            mirror: bytes[0x4b] == 1,
            output_mode: bytes[0x4c],
            blend: match bytes[0x45] { 1 => BlendMode::ColourTransform, 2 => BlendMode::Multiply, _ => BlendMode::Normal },
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectClass { Image, Shape }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneObject<'a> {
    pub address: VglibObject,
    pub group: GroupIndex,
    pub class: ObjectClass,
    pub base: BaseFields,
    bytes: &'a [u8],
}
impl<'a> SceneObject<'a> {
    /// Classifies by the ops word `+0xc0` and checks the class table.
    pub fn decode(memory: &ResourceMemory<'a>, group: GroupIndex, address: VglibObject) -> Result<Self, ObjectRefusal> {
        let base = memory.bytes(address.0, BASE_BYTES).map_err(ObjectRefusal::Memory)?;
        let (class, draw, length) = match word(base, 0xc0) {
            IMAGE_OPS => (ObjectClass::Image, IMAGE_DRAW, IMAGE_BYTES),
            SHAPE_OPS => (ObjectClass::Shape, SHAPE_DRAW, SHAPE_BYTES),
            PATH_LIST_OPS => return Err(ObjectRefusal::PathList),
            PLANE_EVENT_OPS => return Err(ObjectRefusal::PlaneEvent),
            other => return Err(ObjectRefusal::UnknownClass(other)),
        };
        let ops = memory.bytes(DataAddress(word(base, 0xc0)), OPS_BYTES).map_err(ObjectRefusal::Memory)?;
        if word(ops, 0xa0) != draw || BASE_GETTERS.iter().any(|(offset, getter)| word(ops, *offset) != *getter) {
            return Err(ObjectRefusal::OpsTable);
        }
        let bytes = memory.bytes(address.0, length).map_err(ObjectRefusal::Memory)?;
        Ok(Self { address, group, class, base: BaseFields::parse(bytes)?, bytes })
    }
}

/// The OpenVG context returned by `0x8834a4` (`0x886614`: `I4:886680-886688`).
const VG_CONTEXT: DataAddress = DataAddress(0x0088_dd2c);
/// Chunk lists past the pool's first block are walked at most this many times.
const MAX_HANDLE_CHUNKS: usize = 64;

/// Handle stored at image object `+0xc4`: pool type in bits 31..28, class in 27..25, table tag in 24..16 and
/// slot in 15..0 (`I4:82b404-82b418`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageHandle(pub u32);
impl ImageHandle {
    /// The record `0x82b428` returns for this handle, reached from context `+0x1c`, then `+0x1c`, then the
    /// table header at `+0xc` (`I4:83a6c4-83a6cc`, `834878-834880`). Header word 0 is the tag and word 1 the
    /// pool. The pool's one-entry cache `+0x34/+0x38` is consulted first (`82b458-82b464`); otherwise the slot
    /// state halfword must hold 2 in bits 15..12 and the handle's bits 27..16 in 11..0, and the record is
    /// element base + stride x slot (`82b468-82b4b4`) or the same within a chunk of the list at `+0x40`
    /// (`82b4bc-82b538`).
    pub fn resolve(self, memory: &ResourceMemory<'_>) -> Result<DataAddress, ObjectRefusal> {
        let refused = ObjectRefusal::ImageHandle(self.0);
        let at = |address: u32| read_word(memory, DataAddress(address)).map_err(ObjectRefusal::Memory);
        let manager = at(VG_CONTEXT.0.wrapping_add(0x1c))?;
        let registry = at(manager.wrapping_add(0x1c))?;
        let header = at(registry.wrapping_add(0xc))?;
        if at(header)? != (self.0 >> 16) & 0x1ff { return Err(refused); }
        let pool = at(header.wrapping_add(4))?;
        if at(pool)? != self.0 >> 28 { return Err(refused); }
        if at(pool.wrapping_add(0x34))? == self.0 { return Ok(DataAddress(at(pool.wrapping_add(0x38))?)); }
        let slot = self.0 & 0xffff;
        let stride = at(pool.wrapping_add(0x20))?;
        let live = |states: u32, index: u32| -> Result<bool, ObjectRefusal> {
            let bytes = memory.bytes(DataAddress(states.wrapping_add(index * 2)), 2).map_err(ObjectRefusal::Memory)?;
            let state = u32::from(half(bytes, 0));
            Ok(state >> 12 == 2 && state & 0xfff == (self.0 >> 16) & 0xfff)
        };
        let record = if slot < at(pool.wrapping_add(0xc))? {
            if !live(at(pool.wrapping_add(0x30))?, slot)? { return Err(refused); }
            at(pool.wrapping_add(8))?.wrapping_add(stride.wrapping_mul(slot))
        } else {
            let mut chunk = at(pool.wrapping_add(0x40))?;
            let mut found = None;
            for _ in 0..MAX_HANDLE_CHUNKS {
                if chunk == 0 { break; }
                let first = at(chunk.wrapping_add(4))?;
                if slot >= first && slot < first.wrapping_add(at(chunk.wrapping_add(8))?) {
                    let index = slot - first;
                    if !live(at(chunk.wrapping_add(0x10))?, index)? { return Err(refused); }
                    found = Some(at(chunk)?.wrapping_add(stride.wrapping_mul(index)));
                    break;
                }
                chunk = at(chunk.wrapping_add(0x14))?;
            }
            found.ok_or(refused)?
        };
        if record == 0 { return Err(refused); }
        Ok(DataAddress(record))
    }
}

/// The VGImage record behind image handle `+0xc4`, filled by `0x835d90`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageStorage<'a> { pub width: u16, pub height: u16, pub pixels: PixelSource<'a> }
impl<'a> ImageStorage<'a> {
    /// Accepts format 1 (`+0x14 & 0x3f`, `835f14`) with flag bytes `+0x53/+0x54` zero (`835f30-835f38`) and no
    /// sub-window offset `+0x44/+0x46` (`8391a0-8391b4`). Width/height are `+0x3c/+0x40`, stride `+0x4c`, CPU
    /// pixels `+0x68` (`835ee0-836034`). Format 1 is `VG_sRGBA_8888` from `vgc_create_image(1, w, h, 4)` (`6d8dc0-6d8dd0`).
    pub fn decode(memory: &ResourceMemory<'a>, handle: ImageHandle) -> Result<Self, ObjectRefusal> {
        let record = memory.bytes(handle.resolve(memory)?, VG_IMAGE_BYTES).map_err(ObjectRefusal::Memory)?;
        if word(record, 0x14) & 0x3f != 1 || record[0x53] != 0 || record[0x54] != 0 { return Err(ObjectRefusal::ImageFormat); }
        if half(record, 0x44) != 0 || half(record, 0x46) != 0 { return Err(ObjectRefusal::ImageStorage); }
        let width = half(record, 0x3c);
        let height = half(record, 0x40);
        let stride = half(record, 0x4c);
        if width == 0 || height == 0 || (width as i16) < 0 || (height as i16) < 0 || (stride as i16) < 0 {
            return Err(ObjectRefusal::ImageStorage);
        }
        let stride = usize::from(stride);
        let length = stride.checked_mul(usize::from(height) - 1)
            .and_then(|rows| rows.checked_add(usize::from(width) * 4)).ok_or(ObjectRefusal::ImageStorage)?;
        let bytes = memory.bytes(DataAddress(word(record, 0x68)), length).map_err(ObjectRefusal::Memory)?;
        let pixels = PixelSource::new(width, height, stride, PixelOrder::Abgr, bytes).ok_or(ObjectRefusal::ImageStorage)?;
        Ok(Self { width, height, pixels })
    }
}

/// The GVFF behind a shape object, reached through the pool object whose handle `+4` is the shape
/// (`img6:117d75c-117d760`); its content `+0xc` is the block the constructor read (`img6:117d6a0`, `6da6b4`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapePayload<'a> {
    pub owner: (Bank, SlotIndex),
    pub gvff: &'a [u8],
    /// Header bounds `+0x28..+0x34` as x0, y0, x1, y1 (`6da6d0-6da718`).
    pub bounds: [f32; 4],
    /// Shape type `+0xe8` (`6da698-6da69c`); 2 applies the 96-byte block `+0xec` (`6da9c4-6da9d4`, `6da6a0-6da6ac`).
    pub recolour: Option<RecolourBlock>,
}
impl<'a> ShapePayload<'a> {
    pub fn decode(memory: &ResourceMemory<'a>, pool: &ObjectPool<'_>, object: &SceneObject<'a>) -> Result<Self, ObjectRefusal> {
        let bytes = object.bytes;
        if word(bytes, 0xc8) != SHAPE_TYPE_GETTER { return Err(ObjectRefusal::ShapeMethod); }
        let mut owner = None;
        for index in 0..BANK_COUNT as u8 {
            let Some(bank) = Bank::new(index) else { continue };
            for candidate in pool.listed(bank).filter(|candidate| candidate.handle == Some(object.address)) {
                if owner.is_some() { return Err(ObjectRefusal::ShapeOwnerAmbiguous); }
                owner = Some(candidate);
            }
        }
        let owner = owner.ok_or(ObjectRefusal::ShapeOwnerMissing)?;
        if !matches!(owner.kind, ObjectKind::ShapeKindOne | ObjectKind::ShapeKindTwo) { return Err(ObjectRefusal::ShapeOwnerKind(owner.kind)); }
        let content = owner.content.ok_or(ObjectRefusal::ShapeContentMissing)?;
        let tail = memory.region_tail(content).map_err(ObjectRefusal::Memory)?;
        let file = Gvff::parse(tail).map_err(ObjectRefusal::Shape)?;
        let bounds = file.bounds().map(|scalar| scalar.value());
        // Constructor stores x0 at +0x30, y0 at +0x28, x1 at +0x2c, y1 at +0x24 (`6da700-6da718`).
        let stored = [word(bytes, 0x30), word(bytes, 0x28), word(bytes, 0x2c), word(bytes, 0x24)];
        if stored != bounds.map(f32::to_bits) { return Err(ObjectRefusal::ShapeBounds); }
        let recolour = if word(bytes, 0xe8) == 2 {
            let mut block = [0; 96];
            block.copy_from_slice(&bytes[0xec..0x14c]);
            if block.chunks_exact(4).any(|component| !f32::from_le_bytes([component[0], component[1], component[2], component[3]]).is_finite()) {
                return Err(ObjectRefusal::RecolourBlock);
            }
            Some(RecolourBlock(block))
        } else { None };
        Ok(Self { owner: (owner.bank, owner.slot), gvff: &tail[..file.consumed_bytes()], bounds, recolour })
    }
}

/// What one object contributes to the frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Lowered<'a> {
    /// Visibility byte zero: the visitor skips it without counting.
    Hidden,
    /// Image handle `+0xc4` or shape paint `+0xd4` zero: the draw returns before drawing (`6d8874-6d8880`, `6da13c-6da148`).
    NoResource,
    /// Clipping enabled with a scissor rectangle that covers no panel pixel.
    ClippedOut,
    Draw { clip: Option<PanelRect>, command: DrawCommand<'a> },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SceneReport {
    pub objects: usize,
    /// Objects the visitor counts at `0x8e2098+0x158`.
    pub visible: usize,
    pub drawn: usize,
    /// Visible objects whose scissor covers no panel pixel; their draw method still returns 1 (`6d8ce4`, `6da5fc`).
    pub clipped: usize,
}

/// A validated walk of the 24 groups.
pub struct Scene<'m, 'a> {
    memory: &'m ResourceMemory<'a>,
    pool: &'m ObjectPool<'m>,
    display: DisplayState,
    lengths: [u8; GROUP_COUNT],
}

impl<'m, 'a> Scene<'m, 'a> {
    /// Walks every group list, requiring in-pool, in-use, non-repeated nodes, non-null objects, count bytes equal
    /// to the list lengths, and enough free nodes for the frame step's per-group copy.
    pub fn decode(memory: &'m ResourceMemory<'a>, pool: &'m ObjectPool<'m>) -> Result<Self, SceneError> {
        let display = DisplayState::read(memory).map_err(SceneError::Memory)?;
        if matches!(display.frame_mode, 8 | 10) { return Err(SceneError::DualRender(display.frame_mode)); }
        let nodes = memory.bytes(NODE_POOL, NODE_POOL_BYTES).map_err(SceneError::Memory)?;
        let sentinels = memory.bytes(SENTINEL_BASE, GROUP_COUNT * 12).map_err(SceneError::Memory)?;
        let counts = memory.bytes(COUNT_BASE, GROUP_COUNT).map_err(SceneError::Memory)?;
        let mut seen = [0u64; NODE_COUNT / 64];
        let mut lengths = [0u8; GROUP_COUNT];
        let mut longest = 0;
        for index in (0..GROUP_COUNT as u8).rev() {
            let group = GroupIndex(index);
            let mut next = word(sentinels, usize::from(index) * 12);
            let mut length = 0usize;
            while next != group.sentinel() {
                let node = DataAddress(next);
                let slot = node_slot(next).ok_or(SceneError::NodeOutsidePool { group, node })?;
                let bit = 1u64 << (slot % 64);
                if seen[slot / 64] & bit != 0 { return Err(SceneError::RepeatedNode { group, node }); }
                seen[slot / 64] |= bit;
                let record = &nodes[slot * NODE_BYTES..(slot + 1) * NODE_BYTES];
                if word(record, 0xc) == 0 { return Err(SceneError::FreeNode { group, node }); }
                if word(record, 8) == 0 { return Err(SceneError::NullObject { group, node }); }
                length += 1;
                next = word(record, 0);
            }
            let count = counts[usize::from(index)];
            if length != usize::from(count) { return Err(SceneError::CountMismatch { group, count, listed: length }); }
            lengths[usize::from(index)] = count;
            longest = longest.max(length);
        }
        let free = nodes.chunks_exact(NODE_BYTES).filter(|record| word(record, 0xc) == 0).count();
        if free < longest { return Err(SceneError::NodeCapacity { free, needed: longest }); }
        Ok(Self { memory, pool, display, lengths })
    }

    pub fn display(&self) -> DisplayState { self.display }
    pub fn group_length(&self, group: GroupIndex) -> u8 { self.lengths[usize::from(group.0)] }

    /// Listed objects in render order: groups 23 down to 0, each from its head.
    pub fn objects(&self) -> SceneWalk<'_, 'm, 'a> {
        SceneWalk { scene: self, next_group: GROUP_COUNT as u8, node: 0, remaining: 0, group: GroupIndex(0) }
    }

    /// The contribution of one decoded object under this scene's display state.
    pub fn lower_object(&self, object: &SceneObject<'a>) -> Result<Lowered<'a>, ObjectRefusal> {
        let base = object.base;
        if !base.visible { return Ok(Lowered::Hidden); }
        let resource = match object.class { ObjectClass::Image => word(object.bytes, 0xc4), ObjectClass::Shape => word(object.bytes, 0xd4) };
        if resource == 0 { return Ok(Lowered::NoResource); }
        if base.matrix { return Err(ObjectRefusal::Matrix); }
        if self.display.mirror && base.mirror { return Err(ObjectRefusal::Mirror); }
        if base.parameter != 0.0 { return Err(ObjectRefusal::Rotation); }
        if base.blend == BlendMode::ColourTransform { return Err(ObjectRefusal::ColourTransform); }
        let surface = self.display.surface(base.output_mode);
        if surface != (SurfaceScale { width: PANEL_WIDTH as f32, height: PANEL_HEIGHT as f32, x: 1.0, y: 1.0 }) {
            return Err(ObjectRefusal::SurfaceScale { display_mode: self.display.display_mode, output_mode: base.output_mode });
        }
        let dest = destination(base, surface)?;
        let clip = match base.clip {
            None => None,
            Some(clip) => match scissor(clip, surface, object.class) {
                Some(rect) => Some(rect),
                None => return Ok(Lowered::ClippedOut),
            },
        };
        let command = match object.class {
            ObjectClass::Image => {
                let storage = ImageStorage::decode(self.memory, ImageHandle(resource))?;
                if base.size.width != f32::from(storage.width) || base.size.height != f32::from(storage.height) {
                    return Err(ObjectRefusal::ImageSize);
                }
                DrawCommand::ScaledImage { dest, source: storage.pixels }
            }
            ObjectClass::Shape => {
                let payload = ShapePayload::decode(self.memory, self.pool, object)?;
                let [x0, y0, x1, y1] = payload.bounds;
                if x0 > x1 || y0 < y1 { return Err(ObjectRefusal::ShapeOrientation); }
                DrawCommand::ScaledShape { dest, gvff: payload.gvff, recolour: payload.recolour }
            }
        };
        Ok(Lowered::Draw { clip, command })
    }

    /// Appends `Clip`/`ResetClip` and the draw of every drawn object. `Ok` is the completeness predicate together with
    /// a successful `decode`: every listed object decoded and lowered. On error the buffer is not a valid frame.
    pub fn lower(&self, out: &mut CommandBuffer<'_, 'a>) -> Result<SceneReport, SceneError> {
        self.lower_with(|command| out.push(command).map_err(|_| SceneError::Capacity))
    }

    /// `lower` without emitting commands.
    pub fn check(&self) -> Result<SceneReport, SceneError> { self.lower_with(|_| Ok(())) }

    fn lower_with(&self, mut emit: impl FnMut(DrawCommand<'a>) -> Result<(), SceneError>) -> Result<SceneReport, SceneError> {
        let mut report = SceneReport::default();
        for entry in self.objects() {
            let (group, address) = entry?;
            let refuse = |refusal| SceneError::Object { object: address, refusal };
            let object = SceneObject::decode(self.memory, group, address).map_err(refuse)?;
            report.objects += 1;
            match self.lower_object(&object).map_err(refuse)? {
                Lowered::Hidden => {}
                Lowered::NoResource => report.visible += 1,
                Lowered::ClippedOut => { report.visible += 1; report.clipped += 1; }
                Lowered::Draw { clip, command } => {
                    report.visible += 1;
                    report.drawn += 1;
                    emit(clip.map_or(DrawCommand::ResetClip, DrawCommand::Clip))?;
                    emit(command)?;
                }
            }
        }
        Ok(report)
    }
}

pub struct SceneWalk<'s, 'm, 'a> {
    scene: &'s Scene<'m, 'a>,
    next_group: u8,
    node: u32,
    remaining: u8,
    group: GroupIndex,
}
impl Iterator for SceneWalk<'_, '_, '_> {
    type Item = Result<(GroupIndex, VglibObject), SceneError>;
    fn next(&mut self) -> Option<Self::Item> {
        while self.remaining == 0 {
            if self.next_group == 0 { return None; }
            self.next_group -= 1;
            self.group = GroupIndex(self.next_group);
            self.remaining = self.scene.lengths[usize::from(self.next_group)];
            match read_word(self.scene.memory, DataAddress(self.group.sentinel())) {
                Ok(head) => self.node = head,
                Err(error) => { self.next_group = 0; self.remaining = 0; return Some(Err(SceneError::Memory(error))); }
            }
        }
        self.remaining -= 1;
        Some(match self.scene.memory.bytes(DataAddress(self.node), 12) {
            Ok(record) => {
                self.node = word(record, 0);
                Ok((self.group, VglibObject(DataAddress(word(record, 8)))))
            }
            Err(error) => { self.next_group = 0; self.remaining = 0; Err(SceneError::Memory(error)) }
        })
    }
}

fn node_slot(address: u32) -> Option<usize> {
    let offset = address.checked_sub(NODE_POOL.0)? as usize;
    if offset % NODE_BYTES != 0 || offset >= NODE_POOL_BYTES { return None; }
    Some(offset / NODE_BYTES)
}

fn integral(value: f32) -> Option<i32> {
    let nearest = libm::roundf(value);
    if value != nearest || !(-2_147_483_648.0..2_147_483_648.0).contains(&nearest) { return None; }
    Some(nearest as i32)
}

/// Renders lowered stage-0 commands into a VGLIB format-1 OSD buffer (`VG_sRGBA_8888`, A,B,G,R bytes, the model's
/// `mode1-abgr8888-straight` layer profile). The owned frame step and the host reference both render through here.
pub fn render_osd(commands: &[DrawCommand<'_>], pixels: &mut [u8], vertices: &mut [Vertex], strokes: &mut [StrokePrimitive]) -> Result<(), RasterError> {
    let mut workspace = VectorWorkspace {
        vertices,
        strokes,
        dash_budget: DashBudget::new(256),
        accuracy: CurveAccuracy::new(0.25).ok_or(RasterError::InvalidSurfaceBuffer)?,
        samples: SampleGrid::Four,
    };
    Surface::new(SurfaceId::Osd, &mut *pixels)?.render_scene(DrawList::new(SurfaceId::Osd, commands), &mut workspace)?;
    for pixel in pixels.chunks_exact_mut(4) { pixel.reverse(); }
    Ok(())
}

/// At zero parameter `0x6dbb3c` leaves the translation slots at their zeroed values (`6dbd80-6dbef8`; zeroed at
/// `6d87ec-6d87f4` and `6da0c0-6da0c8`), so both draws translate by the scaled position alone: `(x, sh - y - h)` in
/// the y-up surface (`6d8b4c-6d8b7c`, `6da30c-6da33c`). For shapes the GVFF minimum x and minimum y land there
/// (`6da3b0-6da3d4`), so the drawn rectangle is `x, y, w, h` from the panel's top-left.
/// [INFERENCE] Images land on the same rectangle: both classes take their position from the same commit packet
/// (`img6:117d648-117d664`), and the image matrix ends in `diag(1, -1, 1)` (`0x774bac`), whose placement relative to
/// the translation is an engine law of `vgDrawImage`.
fn destination(base: BaseFields, surface: SurfaceScale) -> Result<SceneRect, ObjectRefusal> {
    let x = integral(base.position.x * surface.x);
    let y = integral(base.position.y * surface.y);
    let width = integral(base.size.width * base.scale.x * surface.x).and_then(|value| u16::try_from(value).ok());
    let height = integral(base.size.height * base.scale.y * surface.y).and_then(|value| u16::try_from(value).ok());
    match (x, y, width, height) {
        (Some(x), Some(y), Some(width), Some(height)) => SceneRect::new(x, y, width, height).ok_or(ObjectRefusal::NonIntegralGeometry),
        _ => Err(ObjectRefusal::NonIntegralGeometry),
    }
}

/// The scissor rectangle each draw sets (`6d8c2c-6d8cb4`, `6da414-6da49c`), returned in top-left panel space and
/// intersected with the panel. The shape draw converts the y-up bottom edge with `vcvt.u32` (`6da47c`), the image
/// draw with `vcvt.s32` (`6d8c94`). `None` when no panel pixel is inside.
fn scissor(clip: ObjectClip, surface: SurfaceScale, class: ObjectClass) -> Option<PanelRect> {
    let x = (clip.x as f32 * surface.x) as i32;
    let y = (clip.y as f32 * surface.y) as i32;
    let width = (surface.x * clip.width as f32) as i32;
    let height = (surface.y * clip.height as f32) as i32;
    let bottom_up = surface.height - height as f32 - y as f32;
    let bottom_up = match class { ObjectClass::Image => i64::from(bottom_up as i32), ObjectClass::Shape => i64::from(bottom_up as u32) };
    if width <= 0 || height <= 0 { return None; }
    let top = i64::from(surface.height as i32) - bottom_up - i64::from(height);
    let left = i64::from(x).max(0);
    let right = (i64::from(x) + i64::from(width)).min(PANEL_WIDTH as i64);
    let upper = top.max(0);
    let lower = (top + i64::from(height)).min(PANEL_HEIGHT as i64);
    if left >= right || upper >= lower { return None; }
    PanelRect::new(PanelPoint::new(left as u16, upper as u16)?, (right - left) as u16, (lower - upper) as u16)
}

#[cfg(test)]
#[path = "vglib_scene_tests.rs"]
mod tests;

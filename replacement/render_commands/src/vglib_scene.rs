//! Typed decode of VGLIB's object groups as the stage-0 frame step renders them, and the OpenVG calls of their
//! draw methods.
//!
//! Listing addresses are in `lz/id4.dis` (image 4 at `0x0061c400`) unless prefixed `img6:` (`decode/img6.dis`).
//!
//! Frame step `0x6d7920` renders through `0x6d724c` (`6d72a4-6d72b4`): `0x6d92d8(0x6d7324, &filter, 0xff)` walks the
//! group sentinels from group 23 down to group 0 (`6d92f8-6d9334`). For each group `0x6d909c` copies the group list
//! into a temporary list (`0x6d8fc4`, nodes from `0x6d8f28`) and calls the visitor with each node's object word
//! `+8` in list order (`6d90f0-6d9114`). The visitor `0x6d7324` stops the whole walk on a null object
//! (`6d7330-6d7338`), skips objects whose ops `+0x04` returns 0 (`6d733c-6d7354`), and otherwise calls ops
//! `+0xa0`, ORs its result into `0x8e2098+0x15c`, increments `+0x158` and delays 0 ticks when bit 1 of the count is
//! clear (`6d73e0-6d7434`). Away from frame modes 8 and 10 the filter is -1 (`6d7a90-6d7aa0`, `6d7b0c-6d7b10`),
//! which draws every visible object (`6d7368-6d7378`).

use super::resources::{DataAddress, ResourceError, ResourceMemory};
use super::stock_objects::VglibObject;
use super::vg_submission::*;

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
/// Base getters at `0x7cfc94` (`7cfc94-7cfd38`) that both class tables keep: the constructors copy the 0xa8-byte
/// table and patch only `+0xa0`/`+0xa4` (`6d8d20-6d8d44`, `6da634-6da658`).
const BASE_GETTERS: [(usize, u32); 12] = [
    (0x04, 0x006d_8158), (0x0c, 0x006d_8188), (0x14, 0x006d_81bc), (0x1c, 0x006d_81fc),
    (0x24, 0x006d_8264), (0x2c, 0x006d_82a8), (0x3c, 0x006d_8308), (0x44, 0x006d_8338),
    (0x4c, 0x006d_8378), (0x5c, 0x006d_83f8), (0x6c, 0x006d_8428), (0x84, 0x006d_84b8),
];
const OPS_BYTES: usize = 0xa8;
const IMAGE_BYTES: usize = 0xd4;
const SHAPE_BYTES: usize = 0x14c;
/// Bytes of one shape item at `+0xd8` (`6da4b0`, `6da5cc`).
pub const SHAPE_ITEM_BYTES: usize = 0x54;
/// `+0x30` of a shape item whose stroke has no dash pattern (`6da570-6da57c`).
const NO_DASH: u32 = 0x29a;

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
    /// A nonzero parameter places the draw through `0x6dbb3c`'s rotation offsets (`6dbb3c`, `6db9a4`).
    Rotation,
    /// Blend byte 1 sets the colour transform from `0x774bf4`/`0x774c7c` (`6d89c8-6d8a24`, `6da24c-6da29c`).
    ColourTransform,
}

fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]) }
fn float(bytes: &[u8], offset: usize) -> f32 { f32::from_bits(word(bytes, offset)) }
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

/// Integer clip words `+0x34..+0x40` (`0x6d8378`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectClip { pub x: i32, pub y: i32, pub width: i32, pub height: i32 }

/// Byte `+0x45` (`0x6d82a8`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlendMode {
    /// 1: colour transform on.
    ColourTransform,
    /// 2: `0x1104 = 0x2005` for shapes (`6da2a4-6da2ac`); images take the default branch's calls (`6d8a2c-6d8a4c`).
    Multiply,
    Normal,
}

/// Fields of the 0xc4-byte base object (`0x6d86e0`) that the image and shape draws read through the base getters
/// (`6d8158-6d84cc`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BaseFields {
    /// Byte `+0x47` nonzero (`0x6d8158`).
    pub visible: bool,
    /// `+0x00/+0x04` (`0x6d81fc` outputs 1 and 2).
    pub x: f32,
    pub y: f32,
    /// `+0x08/+0x0c` (`0x6d8264` outputs 1 and 2; `0x6d81fc` multiplies them by the scale words).
    pub width: f32,
    pub height: f32,
    /// `+0x14/+0x18` (`0x6d81bc`).
    pub scale_x: f32,
    pub scale_y: f32,
    /// `+0x1c` in degrees (`0x6d8188`).
    pub parameter: f32,
    /// Enable byte `+0x46` nonzero (`0x6d8338`) and rectangle (`0x6d8378`).
    pub clip: Option<ObjectClip>,
    /// Byte `+0x4a` equal to 1 (`0x6d83f8`).
    pub matrix: bool,
    /// Byte `+0x4b` equal to 1 (`0x6d8428`).
    pub mirror: bool,
    /// Byte `+0x4c` (`0x6d84b8`).
    pub output_mode: u8,
    pub blend: BlendMode,
}
impl BaseFields {
    fn parse(bytes: &[u8]) -> Self {
        let int = |offset| word(bytes, offset) as i32;
        Self {
            visible: bytes[0x47] != 0,
            x: float(bytes, 0),
            y: float(bytes, 4),
            width: float(bytes, 8),
            height: float(bytes, 0xc),
            scale_x: float(bytes, 0x14),
            scale_y: float(bytes, 0x18),
            parameter: float(bytes, 0x1c),
            clip: if bytes[0x46] != 0 { Some(ObjectClip { x: int(0x34), y: int(0x38), width: int(0x3c), height: int(0x40) }) } else { None },
            matrix: bytes[0x4a] == 1,
            mirror: bytes[0x4b] == 1,
            output_mode: bytes[0x4c],
            blend: match bytes[0x45] { 1 => BlendMode::ColourTransform, 2 => BlendMode::Multiply, _ => BlendMode::Normal },
        }
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
        let ops_word = read_word(memory, DataAddress(address.0.0.wrapping_add(0xc0))).map_err(ObjectRefusal::Memory)?;
        let (class, draw, length) = match ops_word {
            IMAGE_OPS => (ObjectClass::Image, IMAGE_DRAW, IMAGE_BYTES),
            SHAPE_OPS => (ObjectClass::Shape, SHAPE_DRAW, SHAPE_BYTES),
            PATH_LIST_OPS => return Err(ObjectRefusal::PathList),
            PLANE_EVENT_OPS => return Err(ObjectRefusal::PlaneEvent),
            other => return Err(ObjectRefusal::UnknownClass(other)),
        };
        let ops = memory.bytes(DataAddress(ops_word), OPS_BYTES).map_err(ObjectRefusal::Memory)?;
        if word(ops, 0xa0) != draw || BASE_GETTERS.iter().any(|(offset, getter)| word(ops, *offset) != *getter) {
            return Err(ObjectRefusal::OpsTable);
        }
        let bytes = memory.bytes(address.0, length).map_err(ObjectRefusal::Memory)?;
        Ok(Self { address, group, class, base: BaseFields::parse(bytes), bytes })
    }
}

/// The placement both draws compute with zero parameter, where `0x6dbb3c` leaves the translation slots at their
/// zeroed values (`6dbd80-6dbef8`; zeroed at `6d87ec-6d87f4` and `6da0c0-6da0c8`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// `0x858950` operands: `x + tx` and `((surface height - height) - y) + ty` (`6d8b4c-6d8b7c`, `6da30c-6da33c`).
    pub translate: [f32; 2],
    /// Identity from `0x774bd0`/`0x774c58`; under display and object mirror, element 0 negated and element 6
    /// `surface width - 2(x + tx)` (`6d8b80-6d8bc8`, `6da340-6da388`).
    pub first: [f32; 9],
    pub scale: [f32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Transform {
    /// Matrix byte 1: the object's matrix at `+0x50` (`6d8a7c-6d8a98`, `6da2e8-6da304`).
    Object(DataAddress),
    Placed(Placement),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageDraw {
    pub handle: u32,
    pub transform: Transform,
    /// `0x8589dc`'s product is followed by `0x774bac` with element 7 the unscaled height `+0xc` (`6d88f8-6d88fc`).
    pub height: f32,
    /// `0x858af4`'s operand, the parameter (`6d8bd4-6d8bd8`).
    pub parameter: f32,
    pub clip: Option<[i32; 4]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeDraw {
    pub blend: BlendMode,
    pub transform: Transform,
    /// `0x858950(-(sx * x0), -(sy * y1))` with x0 `+0x30` and y1 `+0x24` (`6da3b0-6da3c8`).
    pub origin: [f32; 2],
    pub clip: Option<[i32; 4]>,
    /// Items `+0xd8`, count `+0xe0` (`6da4a0-6da4a4`).
    pub items: DataAddress,
    pub count: u32,
}

/// What the visitor does with one visible object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Visit {
    /// Image handle `+0xc4` or shape paint `+0xd4` zero: the draw returns 0 before any OpenVG call
    /// (`6d8874-6d8880`, `6da13c-6da148`).
    NoResource,
    Image(ImageDraw),
    Shape(ShapeDraw),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SceneReport {
    pub objects: usize,
    /// Objects the visitor counts at `0x8e2098+0x158`.
    pub visible: usize,
}

/// A validated walk of the 24 groups.
pub struct Scene<'m, 'a> {
    memory: &'m ResourceMemory<'a>,
    display: DisplayState,
    lengths: [u8; GROUP_COUNT],
}

impl<'m, 'a> Scene<'m, 'a> {
    /// Walks every group list, requiring in-pool, in-use, non-repeated nodes, non-null objects, count bytes equal
    /// to the list lengths, and enough free nodes for the frame step's per-group copy.
    pub fn decode(memory: &'m ResourceMemory<'a>) -> Result<Self, SceneError> {
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
        Ok(Self { memory, display, lengths })
    }

    pub fn display(&self) -> DisplayState { self.display }
    pub fn group_length(&self, group: GroupIndex) -> u8 { self.lengths[usize::from(group.0)] }

    /// Listed objects in render order: groups 23 down to 0, each from its head.
    pub fn objects(&self) -> SceneWalk<'_, 'm, 'a> {
        SceneWalk { scene: self, next_group: GROUP_COUNT as u8, node: 0, remaining: 0, group: GroupIndex(0) }
    }

    /// The visitor's treatment of one decoded object; `None` for a hidden object, which it skips uncounted.
    pub fn visit(&self, object: &SceneObject<'a>) -> Result<Option<Visit>, ObjectRefusal> {
        let base = object.base;
        if !base.visible { return Ok(None); }
        let resource = match object.class { ObjectClass::Image => word(object.bytes, 0xc4), ObjectClass::Shape => word(object.bytes, 0xd4) };
        if resource == 0 { return Ok(Some(Visit::NoResource)); }
        if base.parameter != 0.0 { return Err(ObjectRefusal::Rotation); }
        if base.blend == BlendMode::ColourTransform { return Err(ObjectRefusal::ColourTransform); }
        let surface = self.display.surface(base.output_mode);
        let transform = if base.matrix {
            Transform::Object(DataAddress(object.address.0.0.wrapping_add(0x50)))
        } else {
            Transform::Placed(placement(base, surface, self.display.mirror))
        };
        let clip = base.clip.map(|clip| scissor(clip, surface, object.class));
        Ok(Some(match object.class {
            ObjectClass::Image => Visit::Image(ImageDraw { handle: resource, transform, height: base.height, parameter: base.parameter, clip }),
            ObjectClass::Shape => {
                let items = DataAddress(word(object.bytes, 0xd8));
                let count = word(object.bytes, 0xe0);
                let length = (count as usize).checked_mul(SHAPE_ITEM_BYTES).ok_or(ObjectRefusal::Memory(ResourceError::AddressOverflow))?;
                if count != 0 { self.memory.bytes(items, length).map_err(ObjectRefusal::Memory)?; }
                let scale = [base.scale_x * surface.x, base.scale_y * surface.y];
                let origin = [-(scale[0] * float(object.bytes, 0x30)), -(scale[1] * float(object.bytes, 0x24))];
                Visit::Shape(ShapeDraw { blend: base.blend, transform, origin, clip, items, count })
            }
        }))
    }

    /// Decodes every listed object and appends the visits of the visible ones to `out`. `Ok` is the completeness
    /// predicate together with a successful `decode`.
    pub fn plan(&self, out: &mut PlanBuffer<'_>) -> Result<SceneReport, SceneError> {
        let mut report = SceneReport::default();
        for entry in self.objects() {
            let (group, address) = entry?;
            let refuse = |refusal| SceneError::Object { object: address, refusal };
            let object = SceneObject::decode(self.memory, group, address).map_err(refuse)?;
            report.objects += 1;
            if let Some(visit) = self.visit(&object).map_err(refuse)? {
                report.visible += 1;
                out.push(visit)?;
            }
        }
        Ok(report)
    }
}

/// Visits in walk order, in caller storage.
pub struct PlanBuffer<'s> { storage: &'s mut [Visit], length: usize }
impl<'s> PlanBuffer<'s> {
    pub fn new(storage: &'s mut [Visit]) -> Self { Self { storage, length: 0 } }
    fn push(&mut self, visit: Visit) -> Result<(), SceneError> {
        *self.storage.get_mut(self.length).ok_or(SceneError::Capacity)? = visit;
        self.length += 1;
        Ok(())
    }
    pub fn visits(&self) -> &[Visit] { &self.storage[..self.length] }
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

/// Both draws scale position and size by the surface factors (`6d8900-6d894c`, `6da19c-6da1e8`), then translate by
/// the scaled position in the y-up surface.
fn placement(base: BaseFields, surface: SurfaceScale, display_mirror: bool) -> Placement {
    let (tx, ty) = (0.0f32, 0.0f32);
    let x = base.x * surface.x;
    let y = base.y * surface.y;
    let height = base.height * base.scale_y * surface.y;
    let left = x + tx;
    let mut first = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    if display_mirror && base.mirror {
        first[0] = -first[0];
        first[6] = surface.width - (left + left);
    }
    Placement {
        translate: [left, ((surface.height - height) - y) + ty],
        first,
        scale: [base.scale_x * surface.x, base.scale_y * surface.y],
    }
}

/// The scissor rectangle each draw passes to `0x858d24` (`6d8c2c-6d8ca8`, `6da414-6da490`): x, y-up bottom, width,
/// height. The shape draw converts the bottom with `vcvt.u32` (`6da47c`), the image draw with `vcvt.s32` (`6d8c94`).
fn scissor(clip: ObjectClip, surface: SurfaceScale, class: ObjectClass) -> [i32; 4] {
    let x = (clip.x as f32 * surface.x) as i32;
    let y = (clip.y as f32 * surface.y) as i32;
    let width = (surface.x * clip.width as f32) as i32;
    let height = (surface.y * clip.height as f32) as i32;
    let bottom = (surface.height - height as f32) - y as f32;
    let bottom = match class { ObjectClass::Image => bottom as i32, ObjectClass::Shape => bottom as u32 as i32 };
    [x, bottom, width, height]
}

/// The calls of one visit's draw method; returns the method's result.
pub fn draw(visit: &Visit, memory: &ResourceMemory<'_>, device: &mut impl StageDevice) -> Result<u32, ResourceError> {
    fn set(call: &mut impl FnMut(VgCall), parameter: VgWord, value: i32) { call(VgCall::SetInteger { parameter, value }) }
    let mut call = |operation: VgCall| device.call(operation);
    match *visit {
        Visit::NoResource => return Ok(0),
        Visit::Image(image) => {
            set(&mut call, MATRIX_MODE, 0x1401);
            set(&mut call, IMAGE_QUALITY, 0x1f00);
            set(&mut call, BLEND_MODE, 0x2001);
            set(&mut call, COLOR_TRANSFORM, 0);
            call(VgCall::LoadIdentity);
            match image.transform {
                Transform::Object(matrix) => call(VgCall::LoadMatrix(matrix)),
                Transform::Placed(placed) => {
                    call(VgCall::Translate { x: placed.translate[0], y: placed.translate[1] });
                    call(VgCall::MultiplyMatrix(Matrix::Local(placed.first)));
                    call(VgCall::Rotate(image.parameter));
                    call(VgCall::Scale { x: placed.scale[0], y: placed.scale[1] });
                    call(VgCall::MultiplyMatrix(Matrix::Local([1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, image.height, 1.0])));
                }
            }
            if let Some(values) = image.clip {
                call(VgCall::SetIntegers { parameter: SCISSOR_RECTS, values });
                set(&mut call, SCISSORING, 1);
            }
            set(&mut call, IMAGE_MODE, 4);
            call(VgCall::DrawImage(image.handle));
            set(&mut call, SCISSORING, 0);
            set(&mut call, COLOR_TRANSFORM, 0);
        }
        Visit::Shape(shape) => {
            set(&mut call, MATRIX_MODE, 0x1400);
            set(&mut call, BLEND_MODE, if shape.blend == BlendMode::Multiply { 0x2005 } else { 0x2001 });
            set(&mut call, COLOR_TRANSFORM, 0);
            set(&mut call, FILL_RULE, 0x1901);
            call(VgCall::LoadIdentity);
            match shape.transform {
                Transform::Object(matrix) => call(VgCall::MultiplyMatrix(Matrix::Guest(matrix))),
                Transform::Placed(placed) => {
                    call(VgCall::Translate { x: placed.translate[0], y: placed.translate[1] });
                    call(VgCall::MultiplyMatrix(Matrix::Local(placed.first)));
                    call(VgCall::Translate { x: shape.origin[0], y: shape.origin[1] });
                    call(VgCall::Scale { x: placed.scale[0], y: placed.scale[1] });
                }
            }
            if let Some(values) = shape.clip {
                call(VgCall::SetIntegers { parameter: SCISSOR_RECTS, values });
                set(&mut call, SCISSORING, 1);
            }
            for index in 0..shape.count {
                let address = DataAddress(shape.items.0.wrapping_add(index * SHAPE_ITEM_BYTES as u32));
                let item = memory.bytes(address, SHAPE_ITEM_BYTES)?;
                let at = |slot: usize| word(item, 4 * slot);
                let first = index == 0;
                set(&mut call, BLEND_MODE, if at(11) != 0 { 0x2001 } else { 0x2000 });
                let modes = at(1);
                if modes == 0 { continue; }
                if (modes & 2 != 0 && at(10) != 0) || first {
                    set(&mut call, FILL_RULE, at(0) as i32);
                    call(VgCall::SetPaint { paint: at(6), modes: 2 });
                }
                if (modes & 1 != 0 && at(9) != 0) || first {
                    call(VgCall::SetFloat { parameter: STROKE_LINE_WIDTH, value: f32::from_bits(at(5)) });
                    set(&mut call, STROKE_CAP_STYLE, at(2) as i32);
                    set(&mut call, STROKE_JOIN_STYLE, at(3) as i32);
                    call(VgCall::SetFloat { parameter: STROKE_MITER_LIMIT, value: f32::from_bits(at(4)) });
                    if at(12) == NO_DASH {
                        call(VgCall::SetVector { parameter: STROKE_DASH_PATTERN, count: 0, values: DataAddress(0) });
                    } else {
                        call(VgCall::SetFloat { parameter: STROKE_DASH_PHASE, value: f32::from_bits(at(14)) });
                        call(VgCall::SetVector { parameter: STROKE_DASH_PATTERN, count: at(13) as i32, values: DataAddress(address.0.wrapping_add(0x3c)) });
                    }
                    call(VgCall::SetPaint { paint: at(7), modes: 1 });
                }
                call(VgCall::DrawPath { path: at(8), modes });
            }
            set(&mut call, SCISSORING, 0);
            set(&mut call, COLOR_TRANSFORM, 0);
            call(VgCall::SetFloat { parameter: STROKE_LINE_WIDTH, value: 0.0 });
        }
    }
    Ok(1)
}

/// How the stage-0 render of `0x6d7920` ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderOutcome {
    /// `vglib_select_surface` failed; the frame step returns -255 (`I4:6d7ac0-6d7ac8`).
    SurfaceRefused,
    /// The render result `0x6d724c` returns.
    Rendered(u32),
}

/// Surface selection `0x6d788c` and the render `0x6d724c(-1, width, height, surface, object)` for planned visits.
/// `floor` is the float at `R+0x154` (`I4:6d72ec-6d730c`).
pub fn render_stage(visits: &[Visit], width: i32, height: i32, floor: f32, memory: &ResourceMemory<'_>, device: &mut impl StageDevice) -> Result<RenderOutcome, ResourceError> {
    if !device.make_current() { return Ok(RenderOutcome::SurfaceRefused); }
    if device.get_integer(RENDERING_QUALITY) != RENDERING_QUALITY_BETTER {
        device.call(VgCall::SetInteger { parameter: RENDERING_QUALITY, value: RENDERING_QUALITY_BETTER });
    }
    device.call(VgCall::SetVector { parameter: VIEWPORT, count: 4, values: VIEWPORT_VALUES });
    device.call(VgCall::Clear { x: 0, y: 0, width, height });
    let (mut visited, mut result) = (0u32, 0u32);
    device.set_visits(visited, result);
    for visit in visits {
        result |= draw(visit, memory, device)?;
        visited = visited.wrapping_add(1);
        device.set_visits(visited, result);
        if visited & 2 == 0 { device.yield_task(); }
    }
    device.call(VgCall::Finish);
    if !device.copy_buffers() { device.record_error(); }
    if result == 0 && floor > 0.0 {
        result = 1;
        device.set_visits(visited, result);
    }
    Ok(RenderOutcome::Rendered(result))
}

#[cfg(test)]
#[path = "vglib_scene_tests.rs"]
mod tests;

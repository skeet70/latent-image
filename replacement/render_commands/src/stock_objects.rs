//! Typed decode of the stock draw layer's resource-object pool and its lowering into draw commands.
//!
//! Listing addresses are in `decode/img6.dis` (plain image at `0x01100000`) unless prefixed `I4:`
//! (`lz/id4.dis`, image 4 at `0x0061c400`).

use super::commands::{Alpha, Colour, DrawCommand, Rgba, ShapePaint, ShapePalette, ShapeSecondarySource};
use super::producer::{CommandBuffer, ProducerError, SemanticPrerequisite};
use super::resources::{BitmapCatalog, BitmapKind, DataAddress, PaletteOptions, Percent, ResourceError};
use super::{PaletteIndex, PanelPoint, PanelRect, ResourceId};

/// `xm1_resource_objects`: literal at `117ab60`, bank stride from `mov r5,#27136` at `11797c4`.
pub const POOL_BASE: DataAddress = DataAddress(0x015e_7538);
/// Per-bank `{head, tail}` words (`ov48b_append_object`, `1179710-1179748`).
pub const LISTS_BASE: DataAddress = DataAddress(0x015e_748c);
pub const BANK_COUNT: usize = 19;
pub const SLOTS_PER_BANK: usize = 128;
pub const OBJECT_BYTES: usize = 0xd4;
pub const BANK_STRIDE: usize = SLOTS_PER_BANK * OBJECT_BYTES;
pub const POOL_BYTES: usize = BANK_COUNT * BANK_STRIDE;
pub const LIST_BYTES: usize = BANK_COUNT * 8;

const FILL_RESOURCE: ResourceId = ResourceId(0xff00_0838);

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct Bank(u8);
impl Bank {
    pub const fn new(index: u8) -> Option<Self> { if (index as usize) < BANK_COUNT { Some(Self(index)) } else { None } }
    pub const fn index(self) -> u8 { self.0 }
    /// Banks whose objects `FUN_0117db18` retires and creates (`117db5c-117db74`, literals `117dc00/117dc04`).
    pub const fn committed(self) -> bool { self.0 != 0 }
    const fn base(self) -> u32 { POOL_BASE.0 + self.0 as u32 * BANK_STRIDE as u32 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct SlotIndex(u8);
impl SlotIndex {
    pub const fn new(index: u8) -> Option<Self> { if (index as usize) < SLOTS_PER_BANK { Some(Self(index)) } else { None } }
    pub const fn index(self) -> u8 { self.0 }
}

/// Word `+4`: the VGLIB object returned by `0x6dce28`/`0x6dcc08` (`117d75c-117d760`, `117d800-117d804`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VglibObject(pub DataAddress);

/// Word `+0x10`. Writers: bank reset (2, exact), `ov48b_mark_object` (0/1, exact), `xm1_draw_resource`
/// (0 at `117a23c`/`117a490`/`117a4ac`, 1 at `117a474`, 2 at `117a678`, 3 at `117a204`/`117aab8`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AllocationState {
    /// Allocated this event; the commit creates its VGLIB object.
    Created,
    /// Reused this event with changed content; the commit updates its VGLIB object.
    Matched,
    /// Not drawn this event; the commit retires it (`117db20-117db58`).
    Free,
    /// Reused with every compared field equal to the previous event (`117a7d4-117ab0c`); the commit skips it.
    Unchanged,
}
impl AllocationState {
    fn parse(raw: u32) -> Option<Self> {
        match raw { 0 => Some(Self::Created), 1 => Some(Self::Matched), 2 => Some(Self::Free), 3 => Some(Self::Unchanged), _ => None }
    }
}

/// Word `+0x14`, from `0x1179a50` (`1179a50-1179ab0`) or 3 for a text argument (`117ab58`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectKind {
    /// Zeroed by `ov48b_clear_resource_object` or a failed commit create.
    Unset,
    /// Catalog resources `0..=0x83d` and `0xfe000000`.
    Text,
    /// Native bitmaps and `0xfd000000..=0xfd000001`.
    Bitmap,
    /// `0xfc000000` with a text argument.
    Plane,
    /// Bitmaps of catalog kind 1.
    ShapeKindOne,
    /// Bitmaps of catalog kind 2.
    ShapeKindTwo,
}
impl ObjectKind {
    fn parse(raw: u32) -> Option<Self> {
        match raw { 0 => Some(Self::Unset), 1 => Some(Self::Text), 2 => Some(Self::Bitmap), 3 => Some(Self::Plane), 5 => Some(Self::ShapeKindOne), 6 => Some(Self::ShapeKindTwo), _ => None }
    }
}

/// Signed halfwords `+0x34/+0x36` (`117a4d4-117a4e8`), after text alignment (`117a684-117a6d8`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectPoint { pub x: i16, pub y: i16 }

/// Halfwords `+0x38..+0x3e` (`117a4ec-117a518`): the drawing bounds at draw time, else 720 x 480 or 1920 x 1080.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectClip { pub x: i16, pub y: i16, pub width: u16, pub height: u16 }

/// Halfwords `+0x24..+0x2c`; bitmap stores at `117a5dc-117a614`, text stores at `117d13c-117d150`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectExtent { pub width: u16, pub height: u16, pub pitch: u16, pub scaled_width: u16, pub scaled_height: u16 }

/// Text box fields `+0x1c/+0x1e` (glyph size, `117a554-117a590`, read at `117cfc8-117cfd4`) and `+0x20` (row extent, `117a594-117a598`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextBox { pub glyph: [u16; 2], pub row_extent: u32 }

/// Bytes whose writer or meaning is not established.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnconfirmedBytes {
    pub word_30: [u8; 4],
    pub pad_42: [u8; 2],
    pub byte_63: u8,
    pub word_68: [u8; 4],
    pub key_bytes: [u8; 0x66],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StockObject {
    pub bank: Bank,
    pub slot: SlotIndex,
    pub handle: Option<VglibObject>,
    pub identifier: ResourceId,
    /// Word `+0xc`: text or bitmap data (`0x1179b0c`); a null word makes the commit create nothing (`117d3a8-117d3b4`).
    pub content: Option<DataAddress>,
    pub state: AllocationState,
    pub kind: ObjectKind,
    /// `+0x18/+0x1a` (`117a524-117a530`).
    pub scale: [Percent; 2],
    pub text_box: TextBox,
    pub extent: ObjectExtent,
    /// `+0x2e` (`117a548`): catalog advance from `0x117de64`.
    pub advance: i16,
    pub position: ObjectPoint,
    pub clip: ObjectClip,
    /// `+0x40..+0x59` and `+0x64`, copied by `ov48b_apply_palette_options` (`1179b94-1179bfc`).
    pub options: PaletteOptions,
    /// `+0x5b..+0x62`: component-table RGB with alpha, primary then secondary (`1179c1c-1179c6c`).
    pub components: [Rgba; 2],
    /// `+0x6c`: catalog metric from `0x117df70` (`117a53c`); 1 centres and 2 right-aligns new text (`117a684-117a6d8`).
    pub alignment: i16,
    pub unconfirmed: UnconfirmedBytes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectField { BankByte, Enabled, Style, OffsetY, Initialized }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PoolError {
    PoolLength(usize),
    ListLength(usize),
    /// A head, tail or next word that is not a slot start of its own bank.
    Pointer { bank: Bank, address: DataAddress },
    Cycle { bank: Bank },
    /// The tail word does not name the last node reached from the head.
    Tail { bank: Bank },
    State { bank: Bank, slot: SlotIndex, raw: u32 },
    Kind { bank: Bank, slot: SlotIndex, raw: u32 },
    Field { bank: Bank, slot: SlotIndex, field: ObjectField },
}

fn half(bytes: &[u8], offset: usize) -> u16 { u16::from_le_bytes([bytes[offset], bytes[offset + 1]]) }
fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]) }
fn nonzero(value: u32) -> Option<DataAddress> { if value == 0 { None } else { Some(DataAddress(value)) } }

impl StockObject {
    fn decode(bank: Bank, slot: SlotIndex, bytes: &[u8]) -> Result<Self, PoolError> {
        let field = |field| PoolError::Field { bank, slot, field };
        let raw_state = word(bytes, 0x10);
        let state = AllocationState::parse(raw_state).ok_or(PoolError::State { bank, slot, raw: raw_state })?;
        let raw_kind = word(bytes, 0x14);
        let kind = ObjectKind::parse(raw_kind).ok_or(PoolError::Kind { bank, slot, raw: raw_kind })?;
        if kind != ObjectKind::Unset && bytes[0x5a] != bank.0 { return Err(field(ObjectField::BankByte)); }
        let flag = |offset, name| match word(bytes, offset) { 0 => Ok(false), 1 => Ok(true), _ => Err(field(name)) };
        let short = |offset, name| u16::try_from(word(bytes, offset)).map_err(|_| field(name));
        let options = PaletteOptions {
            enabled: flag(0x40, ObjectField::Enabled)?,
            style: flag(0x64, ObjectField::Style)?,
            palette: PaletteIndex(u32::from(half(bytes, 0x44))),
            offset_y: short(0x48, ObjectField::OffsetY)?,
            packed_bounds: word(bytes, 0x4c),
            bounds: [half(bytes, 0x50), half(bytes, 0x52)],
            initialized: short(0x54, ObjectField::Initialized)?,
            primary: bytes[0x58],
            secondary: bytes[0x59],
        };
        let rgba = |offset: usize| Rgba { red: bytes[offset], green: bytes[offset + 1], blue: bytes[offset + 2], alpha: bytes[offset + 3] };
        let mut key_bytes = [0; 0x66];
        key_bytes.copy_from_slice(&bytes[0x6e..OBJECT_BYTES]);
        Ok(Self {
            bank, slot,
            handle: nonzero(word(bytes, 4)).map(VglibObject),
            identifier: ResourceId(word(bytes, 8)),
            content: nonzero(word(bytes, 0xc)),
            state, kind,
            scale: [Percent(half(bytes, 0x18)), Percent(half(bytes, 0x1a))],
            text_box: TextBox { glyph: [half(bytes, 0x1c), half(bytes, 0x1e)], row_extent: word(bytes, 0x20) },
            extent: ObjectExtent { width: half(bytes, 0x24), height: half(bytes, 0x26), pitch: half(bytes, 0x28), scaled_width: half(bytes, 0x2a), scaled_height: half(bytes, 0x2c) },
            advance: half(bytes, 0x2e) as i16,
            position: ObjectPoint { x: half(bytes, 0x34) as i16, y: half(bytes, 0x36) as i16 },
            clip: ObjectClip { x: half(bytes, 0x38) as i16, y: half(bytes, 0x3a) as i16, width: half(bytes, 0x3c), height: half(bytes, 0x3e) },
            options,
            components: [rgba(0x5b), rgba(0x5f)],
            alignment: half(bytes, 0x6c) as i16,
            unconfirmed: UnconfirmedBytes {
                word_30: [bytes[0x30], bytes[0x31], bytes[0x32], bytes[0x33]],
                pad_42: [bytes[0x42], bytes[0x43]],
                byte_63: bytes[0x63],
                word_68: [bytes[0x68], bytes[0x69], bytes[0x6a], bytes[0x6b]],
                key_bytes,
            },
        })
    }
}

/// The pool and its event lists. Object order per bank is the `next` chain (`+0`, written at `1179738`) from the head word.
pub struct ObjectPool<'a> {
    banks: &'a [u8],
    order: [[SlotIndex; SLOTS_PER_BANK]; BANK_COUNT],
    lengths: [u8; BANK_COUNT],
}

impl<'a> ObjectPool<'a> {
    /// A pool with no members.
    pub const fn empty() -> ObjectPool<'static> {
        ObjectPool { banks: &[], order: [[SlotIndex(0); SLOTS_PER_BANK]; BANK_COUNT], lengths: [0; BANK_COUNT] }
    }

    /// `banks` is `POOL_BYTES` from `POOL_BASE`; `lists` is `LIST_BYTES` from `LISTS_BASE`.
    pub fn parse(banks: &'a [u8], lists: &[u8]) -> Result<Self, PoolError> {
        let mut pool: ObjectPool<'a> = ObjectPool::empty();
        pool.reparse(banks, lists)?;
        Ok(pool)
    }

    /// `parse` into `self`; on error every bank is left empty.
    pub fn reparse(&mut self, banks: &'a [u8], lists: &[u8]) -> Result<(), PoolError> {
        self.lengths = [0; BANK_COUNT];
        let result = self.validate(banks, lists);
        if result.is_err() { self.lengths = [0; BANK_COUNT]; }
        result
    }

    fn validate(&mut self, banks: &'a [u8], lists: &[u8]) -> Result<(), PoolError> {
        if banks.len() != POOL_BYTES { return Err(PoolError::PoolLength(banks.len())); }
        if lists.len() != LIST_BYTES { return Err(PoolError::ListLength(lists.len())); }
        self.banks = banks;
        for index in 0..BANK_COUNT {
            let bank = Bank(index as u8);
            let head = word(lists, index * 8);
            let tail = word(lists, index * 8 + 4);
            let mut visited = [false; SLOTS_PER_BANK];
            let mut length = 0usize;
            let mut last = 0u32;
            let mut next = head;
            while next != 0 {
                let slot = slot_of(bank, next).ok_or(PoolError::Pointer { bank, address: DataAddress(next) })?;
                if visited[slot.0 as usize] { return Err(PoolError::Cycle { bank }); }
                visited[slot.0 as usize] = true;
                self.order[index][length] = slot;
                length += 1;
                last = next;
                let bytes = self.object_bytes(bank, slot);
                StockObject::decode(bank, slot, bytes)?;
                next = word(bytes, 0);
            }
            if tail != last { return Err(PoolError::Tail { bank }); }
            self.lengths[index] = length as u8;
        }
        Ok(())
    }

    fn object_bytes(&self, bank: Bank, slot: SlotIndex) -> &'a [u8] {
        let banks: &'a [u8] = self.banks;
        let start = bank.0 as usize * BANK_STRIDE + slot.0 as usize * OBJECT_BYTES;
        &banks[start..start + OBJECT_BYTES]
    }

    /// Every list member of `bank`, in list order, including `Free` members.
    pub fn listed(&self, bank: Bank) -> impl Iterator<Item = StockObject> + '_ {
        let slots = &self.order[bank.0 as usize][..self.lengths[bank.0 as usize] as usize];
        slots.iter().filter_map(move |slot| StockObject::decode(bank, *slot, self.object_bytes(bank, *slot)).ok())
    }

    /// List members that survive the commit's retirement sweep.
    pub fn live(&self, bank: Bank) -> impl Iterator<Item = StockObject> + '_ {
        self.listed(bank).filter(|object| object.state != AllocationState::Free)
    }
}

fn slot_of(bank: Bank, address: u32) -> Option<SlotIndex> {
    let offset = address.checked_sub(bank.base())? as usize;
    if offset % OBJECT_BYTES != 0 { return None; }
    SlotIndex::new(u8::try_from(offset / OBJECT_BYTES).ok()?)
}

/// Word `0x957aa0+0x2bf0`, read by the commit transform `0x1177a08` (`1177a30-1177a4c`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputSelector {
    /// Value 1: positions and sizes pass unchanged.
    Identity,
    /// Value 2: the 1920 x 1080 doubling branch (`1177a50-1177abc`).
    Doubled,
    /// Any other value: the per-option branch (`1177ac0-1177c40`).
    Panel,
}
impl OutputSelector {
    pub const fn from_word(value: u32) -> Self {
        match value { 1 => Self::Identity, 2 => Self::Doubled, _ => Self::Panel }
    }
}

/// The 3-byte component table at `0x007a1f24` that `xm1_apply_indexed_item_components` copies from (`1179c1c-1179c20`).
#[derive(Clone, Copy)]
pub struct ComponentTable<'a> { bytes: &'a [u8] }
impl<'a> ComponentTable<'a> {
    pub fn new(bytes: &'a [u8]) -> Result<Self, ProducerError> {
        if bytes.is_empty() || bytes.len() % 3 != 0 { return Err(ProducerError::Resource(ResourceError::InvalidFormat)); }
        Ok(Self { bytes })
    }
    /// Lowest index whose entry equals the RGB of `colour`.
    pub fn index_of(self, colour: Rgba) -> Option<PaletteIndex> {
        self.bytes.chunks_exact(3).position(|entry| entry == [colour.red, colour.green, colour.blue])
            .map(|index| PaletteIndex(index as u32))
    }
}

/// `Fill` colours name a palette index; the asset pack's palette records must reproduce the component table for pixel equality.
pub struct LoweringContext<'a> {
    pub bitmaps: &'a BitmapCatalog<'a>,
    pub components: ComponentTable<'a>,
    pub output: OutputSelector,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Placed { x: i32, y: i32, width: u16, height: u16 }

/// `0x1178ab4`: round half away from zero with a 16-bit magnitude (`1178abc-1178b18`).
fn round_eighths(eighths: i64) -> i32 {
    if eighths > 0 { ((eighths + 4) / 8 & 0xffff) as i32 } else { -(((4 - eighths) / 8 & 0xffff) as i32) }
}

/// `0x1177a08` with option byte 0 on the `Panel` branch: every option bit clear (`1177ac0-1177c40`).
fn panel_transform(x: i32, y: i32, width: u16, height: u16) -> Placed {
    let x8 = i64::from(x) * 9;
    let placed_x = round_eighths(x8);
    let placed_y = round_eighths(i64::from(y) * 8);
    let mut placed_width = ((u32::from(width) * 9 / 8) & 0xffff) as u16;
    let mut placed_height = height;
    let right = round_eighths((i64::from(x) + i64::from(width) + 1) * 9);
    if right - (i32::from(placed_width) + placed_x) > 1 { placed_width = placed_width.wrapping_add(1); }
    let bottom = round_eighths((i64::from(y) + i64::from(height) + 1) * 8);
    if bottom - (i32::from(placed_height) + placed_y) > 1 { placed_height = placed_height.wrapping_add(1); }
    Placed { x: placed_x, y: placed_y, width: placed_width, height: placed_height }
}

fn visible(placed: Placed) -> Option<PanelRect> {
    let left = placed.x.max(0);
    let top = placed.y.max(0);
    let right = (placed.x + i32::from(placed.width)).min(super::PANEL_WIDTH as i32);
    let bottom = (placed.y + i32::from(placed.height)).min(super::PANEL_HEIGHT as i32);
    if left >= right || top >= bottom { return None; }
    PanelRect::new(PanelPoint::new(left as u16, top as u16)?, (right - left) as u16, (bottom - top) as u16)
}

fn intersect(a: PanelRect, b: PanelRect) -> Option<PanelRect> {
    let span = |rect: PanelRect| (i32::from(rect.origin().x()), i32::from(rect.origin().y()), i32::from(rect.origin().x()) + i32::from(rect.width()), i32::from(rect.origin().y()) + i32::from(rect.height()));
    let (al, at, ar, ab) = span(a);
    let (bl, bt, br, bb) = span(b);
    visible(Placed { x: al.max(bl), y: at.max(bt), width: (ar.min(br) - al.max(bl)).max(0) as u16, height: (ab.min(bb) - at.max(bt)).max(0) as u16 })
}

impl StockObject {
    fn refuse(&self, prerequisite: fn(ResourceId) -> SemanticPrerequisite) -> ProducerError {
        ProducerError::MissingSemantic(prerequisite(self.identifier))
    }

    fn place(&self, context: &LoweringContext<'_>, x: i32, y: i32, width: u16, height: u16) -> Result<Placed, ProducerError> {
        if self.options.secondary == 0xff { return Ok(Placed { x, y, width, height }); }
        match context.output {
            OutputSelector::Identity => Ok(Placed { x, y, width, height }),
            OutputSelector::Panel if self.options.secondary == 0 => Ok(panel_transform(x, y, width, height)),
            _ => Err(self.refuse(SemanticPrerequisite::ObjectTransform)),
        }
    }

    /// Appends `Clip`, the object's draw and `ResetClip`, as the commit's create (`0x117d390`) would present it.
    /// Emits nothing for objects that produce no VGLIB object. A `Capacity` error can leave this object's earlier commands appended.
    pub fn lower<'assets>(&self, context: &LoweringContext<'_>, out: &mut CommandBuffer<'_, 'assets>) -> Result<(), ProducerError> {
        match self.state {
            AllocationState::Free => return Ok(()),
            AllocationState::Matched | AllocationState::Unchanged => return Err(self.refuse(SemanticPrerequisite::ReusedObject)),
            AllocationState::Created => {}
        }
        if self.content.is_none() { return Ok(()); }
        let id = self.identifier;
        match self.kind {
            ObjectKind::Unset => return Ok(()),
            ObjectKind::Text if id.0 == 0xfe00_0000 => return Err(ProducerError::MissingSemantic(SemanticPrerequisite::TextCache)),
            ObjectKind::Text => return Err(self.refuse(SemanticPrerequisite::ObjectText)),
            ObjectKind::Plane => return Err(self.refuse(SemanticPrerequisite::PlaneResource)),
            ObjectKind::Bitmap if (0xfd00_0000..=0xfd00_0001).contains(&id.0) => return Err(self.refuse(SemanticPrerequisite::CustomResource)),
            ObjectKind::Bitmap | ObjectKind::ShapeKindOne | ObjectKind::ShapeKindTwo => {}
        }
        let options = self.options;
        if options.palette.0 != 0 || options.offset_y != 0 || options.packed_bounds != 0 || options.bounds != [0; 2]
            || options.primary != 0 || options.initialized != 1 || self.scale != [Percent(100); 2] {
            return Err(self.refuse(SemanticPrerequisite::ObjectStyling));
        }
        let clip = self.place(context, i32::from(self.clip.x), i32::from(self.clip.y), self.clip.width, self.clip.height)?;
        let placed = self.place(context, i32::from(self.position.x), i32::from(self.position.y), self.extent.scaled_width, self.extent.scaled_height)?;
        let entry = context.bitmaps.entry(id).map_err(ProducerError::Bitmap)?;
        let expected = match entry.kind { BitmapKind::NativeRgba => ObjectKind::Bitmap, BitmapKind::ShapeKindOne => ObjectKind::ShapeKindOne, BitmapKind::ShapeKindTwo => ObjectKind::ShapeKindTwo };
        if expected != self.kind { return Err(ProducerError::MissingSemantic(SemanticPrerequisite::BitmapKind(id))); }
        let Some(clip) = visible(clip) else { return Ok(()) };
        let [primary, secondary] = self.components;
        let secondary_source = if options.style { ShapeSecondarySource::Red } else { ShapeSecondarySource::White };
        let command = if id == FILL_RESOURCE {
            if primary.alpha != 0 || options.style { return Err(self.refuse(SemanticPrerequisite::ObjectStyling)); }
            let palette = context.components.index_of(secondary).ok_or(self.refuse(SemanticPrerequisite::ObjectColour))?;
            let Some(rect) = visible(placed).and_then(|rect| intersect(rect, clip)) else { return Ok(()) };
            DrawCommand::Fill { rect, colour: Colour { palette, alpha: Alpha(secondary.alpha) } }
        } else {
            if placed.width != entry.width || placed.height != entry.height { return Err(self.refuse(SemanticPrerequisite::ObjectTransform)); }
            let origin = u16::try_from(placed.x).ok().zip(u16::try_from(placed.y).ok())
                .and_then(|(x, y)| PanelPoint::new(x, y)).ok_or(ProducerError::Geometry)?;
            match self.kind {
                ObjectKind::ShapeKindOne => DrawCommand::Shape { origin, image: entry.image, alpha: Alpha::OPAQUE, paint: ShapePaint::Recolour(ShapePalette { primary, secondary, secondary_source }) },
                ObjectKind::ShapeKindTwo => DrawCommand::Shape { origin, image: entry.image, alpha: Alpha::OPAQUE, paint: ShapePaint::Original },
                _ => DrawCommand::Image { origin, image: entry.image, alpha: Alpha::OPAQUE },
            }
        };
        out.push(DrawCommand::Clip(clip))?;
        out.push(command)?;
        out.push(DrawCommand::ResetClip)
    }
}

/// Lowers every live pool object in VGLIB composite order: the traversal `I4:0x6d92d8` with selector 0xff visits
/// groups 23 down to 0 (`I4:6d92f8-6d9338`), each in insertion order (`I4:6d8f94-6d8fa8` appends at the tail,
/// `I4:6d909c` walks from the head). The group is packet byte `+0x14` (`I4:6dcd34`, `I4:6dceb0`, `I4:6dcf7c`), which the
/// commit's create fills from object byte `+0x5a`, the bank (`117d49c-117d4a0`). New objects are created in list order
/// (`117db84-117dbe8`), so a bank holding only `Created` objects composites in list order.
/// Plane-4 draw events (`0x6dd4d4`) also add VGLIB objects; they are not part of the pool and are not lowered here.
pub fn frame_commands<'assets>(pool: &ObjectPool<'_>, context: &LoweringContext<'_>, out: &mut CommandBuffer<'_, 'assets>) -> Result<(), ProducerError> {
    if pool.live(Bank(0)).next().is_some() { return Err(ProducerError::MissingSemantic(SemanticPrerequisite::CompositeOrder)); }
    for index in (1..BANK_COUNT as u8).rev() {
        for object in pool.live(Bank(index)) { object.lower(context, out)?; }
    }
    Ok(())
}

#[cfg(test)]
#[path = "stock_objects_tests.rs"]
mod tests;

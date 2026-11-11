use super::{CatalogTextId, FontId, PaletteIndex, ResourceId};
use super::commands::{Alpha, Colour, Rgba, ShapePaint, ShapePalette, ShapeSecondarySource, TextAdvance, TextCoordinate, TextPenOp, TextX, TextY};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DataAddress(pub u32);
#[derive(Clone, Copy)]
pub struct DataRegion<'a> { pub base: DataAddress, pub bytes: &'a [u8] }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceError { MissingData(DataAddress), MissingLanguage(LanguageId), AddressOverflow, RegionOrder, OverlappingRegions, InvalidText, Capacity, Variant, InvalidFormat, FormatArgument }

pub struct ResourceMemory<'a> { regions: &'a [DataRegion<'a>] }
impl<'a> ResourceMemory<'a> {
    pub fn new(regions: &'a [DataRegion<'a>]) -> Result<Self, ResourceError> {
        let mut previous_base = None;
        let mut previous_end = 0u64;
        for region in regions {
            let base = u64::from(region.base.0);
            let end = base.checked_add(region.bytes.len() as u64).ok_or(ResourceError::AddressOverflow)?;
            if end > u64::from(u32::MAX) + 1 { return Err(ResourceError::AddressOverflow); }
            if previous_base.is_some_and(|previous| region.base.0 <= previous) { return Err(ResourceError::RegionOrder); }
            if base < previous_end { return Err(ResourceError::OverlappingRegions); }
            previous_base = Some(region.base.0);
            previous_end = end;
        }
        Ok(Self { regions })
    }
    pub fn bytes(&self, address: DataAddress, count: usize) -> Result<&'a [u8], ResourceError> {
        self.region_tail(address)?.get(..count).ok_or(ResourceError::MissingData(address))
    }
    pub fn region_tail(&self, address: DataAddress) -> Result<&'a [u8], ResourceError> {
        let index = self.regions.partition_point(|region| region.base.0 <= address.0).checked_sub(1).ok_or(ResourceError::MissingData(address))?;
        let region = &self.regions[index];
        let start = (address.0 - region.base.0) as usize;
        region.bytes.get(start..).ok_or(ResourceError::MissingData(address))
    }
    fn half(&self, address: DataAddress) -> Result<u16, ResourceError> {
        Ok(u16::from_le_bytes(self.bytes(address, 2)?.try_into().unwrap()))
    }
    fn word(&self, address: DataAddress) -> Result<u32, ResourceError> {
        Ok(u32::from_le_bytes(self.bytes(address, 4)?.try_into().unwrap()))
    }
    pub fn utf16<'out>(&self, address: DataAddress, output: &'out mut [u8]) -> Result<&'out str, ResourceError> {
        let mut address = address;
        let mut length = 0;
        loop {
            let first = self.half(address)?;
            if first == 0 { break; }
            address = add(address, 2)?;
            let scalar = if (0xd800..=0xdbff).contains(&first) {
                let second = self.half(address)?;
                if !(0xdc00..=0xdfff).contains(&second) { return Err(ResourceError::InvalidText); }
                address = add(address, 2)?;
                0x10000 + ((u32::from(first) - 0xd800) << 10) + u32::from(second) - 0xdc00
            } else { u32::from(first) };
            let character = char::from_u32(scalar).ok_or(ResourceError::InvalidText)?;
            let end = length + character.len_utf8();
            character.encode_utf8(output.get_mut(length..end).ok_or(ResourceError::Capacity)?);
            length = end;
        }
        core::str::from_utf8(&output[..length]).map_err(|_| ResourceError::InvalidText)
    }
}
fn add(address: DataAddress, offset: u32) -> Result<DataAddress, ResourceError> {
    address.0.checked_add(offset).map(DataAddress).ok_or(ResourceError::AddressOverflow)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LanguageId(u8);
impl LanguageId {
    pub const fn new(raw: i32) -> Self { Self(if raw >= 0 && raw <= 34 { raw as u8 } else { 1 }) }
    pub const fn value(self) -> u8 { self.0 }
    pub const fn font_class(self) -> FontLanguage {
        match self.0 { 0 => FontLanguage::Three, 5 => FontLanguage::Two, 6 => FontLanguage::One, 12 => FontLanguage::Four, 13 => FontLanguage::Six, 22 | 26 | 30 => FontLanguage::Zero, _ => FontLanguage::Five }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FontLanguage { Zero, One, Two, Three, Four, Five, Six }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextVariant(pub u8);
#[derive(Clone, Copy)]
pub struct LanguageCatalog { pub language: LanguageId, pub records: DataAddress }

pub struct CatalogSet<'a> { pub memory: ResourceMemory<'a>, pub catalogs: &'a [LanguageCatalog] }
impl CatalogSet<'_> {
    pub fn text<'out>(&self, language: LanguageId, id: CatalogTextId, variant: TextVariant, output: &'out mut [u8]) -> Result<&'out str, ResourceError> {
        let id = id.value();
        if id == 0 { return Err(ResourceError::Variant); }
        let address = if id >= 0x83c {
            add(DataAddress(0x0037b784), (id - 0x83c) * 0x42)?
        } else {
            let catalog = self.catalogs.iter().find(|catalog| catalog.language == language).ok_or(ResourceError::MissingLanguage(language))?;
            let record = add(catalog.records, id * 8)?;
            let count = self.memory.bytes(add(record, 2)?, 1)?[0];
            if variant.0 >= count { return Err(ResourceError::Variant); }
            let metrics = DataAddress(self.memory.word(add(record, 4)?)?);
            if metrics.0 == 0 { return Err(ResourceError::MissingData(metrics)); }
            DataAddress(self.memory.word(add(metrics, u32::from(variant.0) * 12)?)?)
        };
        self.memory.utf16(address, output)
    }

    pub fn binding<'out>(&self, language: LanguageId, id: CatalogTextId, variant: TextVariant, output: &'out mut [u8]) -> Result<TextBinding<'out>, ResourceError> {
        Ok(TextBinding { resource: ResourceId(id.value()), text: self.text(language, id, variant, output)? })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Percent(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FormatOptions { pub angle: bool, pub parenthesis: bool, pub primary_enabled: bool }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FormatState {
    pub x: u16, pub y: u16, pub font: FontId, pub primary: PaletteIndex, pub secondary: PaletteIndex,
    pub primary_alpha: Alpha, pub secondary_alpha: Alpha, pub options: FormatOptions,
    pub scale: [Percent; 2], pub offset: [u16; 2], pub packed_bounds: u32,
    pub bounds: [u16; 2], pub initialized: u16,
}
impl FormatState {
    pub const fn new(font: FontId, primary: Colour, secondary: Colour) -> Self {
        Self { x: 0, y: 0, font, primary: primary.palette, secondary: secondary.palette, primary_alpha: primary.alpha, secondary_alpha: secondary.alpha,
            options: FormatOptions { angle: false, parenthesis: false, primary_enabled: false },
            scale: [Percent(0); 2], offset: [0; 2], packed_bounds: 0, bounds: [0; 2], initialized: 1 }
    }
    pub fn parse_stock<'text>(mut self, bytes: &'text [u8], argument: FormatArgument<'text>, output: &mut [FormatEmission<'text>]) -> Result<(Self, usize), ResourceError> {
        let count = self.parse_stock_in_place(bytes, argument, output)?;
        Ok((self, count))
    }

    pub fn parse_stock_in_place<'text>(&mut self, bytes: &'text [u8], argument: FormatArgument<'text>, output: &mut [FormatEmission<'text>]) -> Result<usize, ResourceError> {
        let state = self;
        let mut cursor = 0;
        let mut count = 0;
        let mut secondary = 0u16;
        while bytes.get(cursor).copied().unwrap_or(0) != 0 {
            if bytes[cursor] != b'%' {
                let start = cursor;
                cursor += 1;
                emit(output, &mut count, FormatEmission { state: *state, action: FormatAction::Text(EmittedText::Bytes(&bytes[start..])) })?;
                continue;
            }
            cursor += 1;
            let mut value = 0u16;
            loop {
                let code = *bytes.get(cursor).ok_or(ResourceError::InvalidFormat)?;
                if code == 0 { return Err(ResourceError::InvalidFormat); }
                cursor += 1;
                let terminal = match code.to_ascii_lowercase() {
                    b'i' => { let FormatArgument::Scalar { primary, secondary: other } = argument else { return Err(ResourceError::FormatArgument); }; value = primary; secondary = other; false }
                    b'0'..=b'9' => { value = value.wrapping_mul(10).wrapping_add(u16::from(code - b'0')); false }
                    b'<' => { state.options.angle = true; true }
                    b'>' => { state.options.angle = false; true }
                    b'(' => { state.options.parenthesis = true; true }
                    b')' => { state.options.parenthesis = false; true }
                    b'a' => { state.options.primary_enabled = value & 1 != 0; true }
                    b'b' => { if value > 255 { return Err(ResourceError::InvalidFormat); } state.secondary = PaletteIndex(u32::from(value)); true }
                    b'c' => { if value > 255 { return Err(ResourceError::InvalidFormat); } state.primary = PaletteIndex(u32::from(value)); true }
                    b'p' => { if value > 18 { return Err(ResourceError::InvalidFormat); } state.font = FontId(u32::from(value)); true }
                    b't' => { state.primary_alpha = Alpha(value.min(255) as u8); true }
                    b'w' => { state.secondary_alpha = Alpha(value.min(255) as u8); true }
                    b'x' => { if value > 640 { return Err(ResourceError::InvalidFormat); } state.x = value; emit(output, &mut count, FormatEmission { state: *state, action: FormatAction::Pen(TextPenOp::SetX(TextX::new_for(super::SurfaceId::Osd, value).ok_or(ResourceError::InvalidFormat)?)) })?; true }
                    b'y' => { if value > 480 { return Err(ResourceError::InvalidFormat); } state.y = value; emit(output, &mut count, FormatEmission { state: *state, action: FormatAction::Pen(TextPenOp::SetY(TextY::new_for(super::SurfaceId::Osd, value).ok_or(ResourceError::InvalidFormat)?)) })?; true }
                    b'm' => { if value > 32 { return Err(ResourceError::InvalidFormat); } emit(output, &mut count, FormatEmission { state: *state, action: FormatAction::Pen(TextPenOp::AdvanceX { pixels: TextAdvance::new(value * 20), max_before: TextCoordinate::new((33 - value) * 20 - 1) }) })?; true }
                    b'z' => { state.scale = [scaled(state.scale[0], value), scaled(state.scale[1], secondary)]; true }
                    b'r' => { state.offset = [value, secondary]; false }
                    b'e' => { state.packed_bounds = u32::from(value); false }
                    b'f' => { state.bounds = [value, secondary]; false }
                    b'h' => { state.initialized = value; false }
                    b'%' => { emit(output, &mut count, FormatEmission { state: *state, action: FormatAction::Text(EmittedText::Bytes(b"%\0")) })?; true }
                    b's' => { let FormatArgument::Bytes(text) = argument else { return Err(ResourceError::FormatArgument); }; emit(output, &mut count, FormatEmission { state: *state, action: FormatAction::Text(EmittedText::Bytes(text)) })?; true }
                    b'u' => { let FormatArgument::Wide(text) = argument else { return Err(ResourceError::FormatArgument); }; emit(output, &mut count, FormatEmission { state: *state, action: FormatAction::Text(EmittedText::Wide(text)) })?; true }
                    _ => false,
                };
                if terminal || value == u16::MAX { break; }
            }
        }
        Ok(count)
    }
}
fn scaled(previous: Percent, value: u16) -> Percent {
    Percent(if previous.0 == 0 || previous.0 == 100 { value } else { previous.0.wrapping_mul(value) / 100 })
}
fn emit<'a>(output: &mut [FormatEmission<'a>], count: &mut usize, emission: FormatEmission<'a>) -> Result<(), ResourceError> {
    *output.get_mut(*count).ok_or(ResourceError::Capacity)? = emission;
    *count += 1;
    Ok(())
}
#[derive(Clone, Copy)]
pub enum FormatArgument<'a> { Scalar { primary: u16, secondary: u16 }, Bytes(&'a [u8]), Wide(&'a [u16]) }
#[derive(Clone, Copy)]
pub enum EmittedText<'a> { Bytes(&'a [u8]), Wide(&'a [u16]) }
#[derive(Clone, Copy)]
pub struct FormatEmission<'a> { pub state: FormatState, pub action: FormatAction<'a> }
#[derive(Clone, Copy)]
pub enum FormatAction<'a> { Text(EmittedText<'a>), Pen(TextPenOp) }

pub struct TextBinding<'a> { pub resource: ResourceId, pub text: &'a str }

pub fn emission_text<'out>(text: EmittedText<'_>, output: &'out mut [u8]) -> Result<&'out str, ResourceError> {
    let count = match text {
        EmittedText::Bytes(bytes) => bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len()),
        EmittedText::Wide(words) => words.iter().position(|word| *word == 0).unwrap_or(words.len()),
    };
    if count == 0 || count > 32 { return Err(ResourceError::InvalidText); }
    let mut length = 0;
    for index in 0..count {
        let word = match text { EmittedText::Bytes(bytes) => u16::from(bytes[index]), EmittedText::Wide(words) => words[index] };
        let scalar = if word < 0x100 { u32::from(word) + 0xe100 } else { u32::from(word) };
        let character = char::from_u32(scalar).ok_or(ResourceError::InvalidText)?;
        let end = length + character.len_utf8();
        character.encode_utf8(output.get_mut(length..end).ok_or(ResourceError::Capacity)?);
        length = end;
    }
    core::str::from_utf8(&output[..length]).map_err(|_| ResourceError::InvalidText)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaletteOptions {
    pub enabled: bool,
    pub style: bool,
    pub palette: PaletteIndex,
    pub offset_y: u16,
    pub packed_bounds: u32,
    pub bounds: [u16; 2],
    pub initialized: u16,
    pub primary: u8,
    pub secondary: u8,
}
impl PaletteOptions {
    pub fn parse(bytes: &[u8]) -> Result<Self, ResourceError> {
        let bytes = bytes.get(..0x30).ok_or(ResourceError::InvalidFormat)?;
        let half = |offset| u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
        Ok(Self {
            enabled: bytes[0x11] & 0x40 != 0, style: bytes[0x11] & 0x20 != 0,
            palette: PaletteIndex(u32::from(half(0x1a))), offset_y: half(0x1c),
            packed_bounds: u32::from_le_bytes(bytes[0x20..0x24].try_into().unwrap()),
            bounds: [half(0x24), half(0x26)], initialized: half(0x28),
            primary: bytes[0x2a], secondary: bytes[0x2b],
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BitmapKind { NativeRgba, ShapeKindOne, ShapeKindTwo }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BitmapEntry {
    pub image: super::ImageId,
    pub width: u16,
    pub height: u16,
    pub kind: BitmapKind,
    pub data: DataAddress,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BitmapError { Resource(ResourceError), Raster(super::raster::RasterError), Shape(super::shape_assets::ShapeError), InvalidIdentifier(ResourceId), Unsupported(BitmapKind), Geometry }

pub struct BitmapCatalog<'a> { pub memory: ResourceMemory<'a> }
impl<'data> BitmapCatalog<'data> {
    pub fn entry(&self, resource: ResourceId) -> Result<BitmapEntry, BitmapError> {
        let payload = resource.0.checked_sub(0xff000000).filter(|payload| *payload <= 0x8c5).ok_or(BitmapError::InvalidIdentifier(resource))?;
        let address = if payload == 0x8c5 { DataAddress(0x0037bab0) } else { DataAddress(0x0022a8b4 + payload * 12) };
        let raw = self.memory.bytes(address, 12).map_err(BitmapError::Resource)?;
        let kind = match u32::from_le_bytes(raw[4..8].try_into().unwrap()) { 1 => BitmapKind::ShapeKindOne, 2 => BitmapKind::ShapeKindTwo, _ => BitmapKind::NativeRgba };
        Ok(BitmapEntry {
            image: super::ImageId::new(payload).ok_or(BitmapError::InvalidIdentifier(resource))?,
            width: u16::from_le_bytes(raw[0..2].try_into().unwrap()),
            height: u16::from_le_bytes(raw[2..4].try_into().unwrap()),
            kind, data: DataAddress(u32::from_le_bytes(raw[8..12].try_into().unwrap())),
        })
    }

    pub fn resolve(&self, resource: ResourceId, palette: Option<IndexedItemPalette>) -> Result<super::producer::ResourceContent<'static>, super::producer::ProducerError> {
        use super::producer::{classify_stock_resource, ProducerError, ResourceContent, SemanticPrerequisite};
        if !(0xff000000..=0xff0008c5).contains(&resource.0) { return classify_stock_resource(resource); }
        let entry = self.entry(resource).map_err(ProducerError::Bitmap)?;
        Ok(match entry.kind {
            BitmapKind::NativeRgba => ResourceContent::Image { image: entry.image, alpha: Alpha::OPAQUE },
            BitmapKind::ShapeKindOne => {
                let palette = palette.ok_or(ProducerError::MissingSemantic(SemanticPrerequisite::PaletteOptions))?;
                let colours = palette.resolve(&self.memory).map_err(ProducerError::Resource)?;
                ResourceContent::Shape { image: entry.image, alpha: Alpha::OPAQUE, paint: ShapePaint::Recolour(colours) }
            }
            BitmapKind::ShapeKindTwo => ResourceContent::Shape { image: entry.image, alpha: Alpha::OPAQUE, paint: ShapePaint::Original },
        })
    }

    pub fn shape_binding(&self, resource: ResourceId) -> Result<super::raster::ShapeBinding<'data>, BitmapError> {
        let entry = self.entry(resource)?;
        let asset = super::shape_assets::ShapeAsset::decode(entry, &self.memory).map_err(BitmapError::Shape)?;
        Ok(super::raster::ShapeBinding::new(asset))
    }

    pub fn extract_rcp1<'out>(
        &self,
        resource: ResourceId,
        provenance: super::raster::AssetProvenance<'out>,
        output: &'out mut [u8],
    ) -> Result<super::raster::AssetPack<'out>, BitmapError> {
        let entry = self.entry(resource)?;
        if entry.kind != BitmapKind::NativeRgba { return Err(BitmapError::Unsupported(entry.kind)); }
        if entry.width == 0 || entry.height == 0 { return Err(BitmapError::Geometry); }
        let stride = entry.width.checked_mul(4).ok_or(BitmapError::Geometry)?;
        let length = usize::from(stride).checked_mul(usize::from(entry.height)).ok_or(BitmapError::Geometry)?;
        let output = output.get_mut(..32usize.checked_add(length).ok_or(BitmapError::Geometry)?).ok_or(BitmapError::Resource(ResourceError::Capacity))?;
        let pixels = self.memory.bytes(entry.data, length).map_err(BitmapError::Resource)?;
        output[..32].fill(0);
        output[..4].copy_from_slice(b"RCP1");
        output[4..8].copy_from_slice(&1u32.to_le_bytes());
        output[8] = 2;
        output[12..16].copy_from_slice(&entry.image.value().to_le_bytes());
        output[20..22].copy_from_slice(&entry.width.to_le_bytes());
        output[22..24].copy_from_slice(&entry.height.to_le_bytes());
        output[24..26].copy_from_slice(&stride.to_le_bytes());
        output[28..32].copy_from_slice(&(length as u32).to_le_bytes());
        for (source, destination) in pixels.chunks_exact(4).zip(output[32..].chunks_exact_mut(4)) {
            destination.copy_from_slice(&[source[3], source[2], source[1], source[0]]);
        }
        super::raster::AssetPack::parse(output, provenance).map_err(BitmapError::Raster)
    }
}

#[cfg(test)]
#[path = "resources_bridge_tests.rs"]
mod bridge_tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexedItemPalette {
    pub primary: Colour,
    pub secondary: Colour,
    pub secondary_source: ShapeSecondarySource,
}

impl IndexedItemPalette {
    pub fn resolve(self, memory: &ResourceMemory<'_>) -> Result<ShapePalette, ResourceError> {
        fn colour(memory: &ResourceMemory<'_>, colour: Colour) -> Result<Rgba, ResourceError> {
            let offset = colour.palette.0.checked_mul(3).ok_or(ResourceError::AddressOverflow)?;
            let rgb = memory.bytes(add(DataAddress(0x007a1f24), offset)?, 3)?;
            Ok(Rgba { red: rgb[0], green: rgb[1], blue: rgb[2], alpha: colour.alpha.0 })
        }
        Ok(ShapePalette { primary: colour(memory, self.primary)?, secondary: colour(memory, self.secondary)?, secondary_source: self.secondary_source })
    }
}

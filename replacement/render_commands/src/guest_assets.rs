use super::{FontId, PaletteIndex, PanelPoint, ResourceId};
use super::commands::{CatalogTextId, Rgba};
use super::font_core::{CoverageEncoding, FontAsset, FontConfig, FontEngine, FontError, FontSize, FontWorkspace, GlyphId, PixelExtent, WritingMode};
use super::font_core::outline::FdlResource;
use super::raster::{AssetPack, AssetProvenance, FontBinding, RasterError, Surface};
use super::resources::{BitmapCatalog, BitmapError, CatalogSet, DataAddress, DataRegion, FontLanguage, LanguageCatalog, LanguageId, ResourceError, ResourceMemory, TextVariant};

/// Language argument of the catalog selector 0x0117e1b8, stored by 0x0117e1c0..0x0117e1cc.
const RENDER_LANGUAGE: DataAddress = DataAddress(0x0095_a6a4);
/// Catalog table base written per language by 0x0117e1b8 and read by the entry accessor 0x0117deb0.
const CATALOG_TABLE: DataAddress = DataAddress(0x0117_8aac);
/// Text-font index stored by 0x00649030..0x00649060 after 0x006aa750 loads that font.
const LOADED_TEXT_FONT: DataAddress = DataAddress(0x0095_53c4);
/// Eight 24-byte records {name, name length, address, size}: ARABIC, BIG5, GB, JIS, KSC, LATIN, THAI, ICON.
const FONT_REGISTRY: DataAddress = DataAddress(0x0118_04a4);
const ICON_REGISTRY_INDEX: u32 = 7;
const TEXT_FONT_BASE: DataAddress = DataAddress(0x0045_c000);
const ICON_FONT_BASE: DataAddress = DataAddress(0x003b_c000);
const PALETTE: DataAddress = DataAddress(0x007a_1f24);
/// 10-byte records {x, y, cell width, cell height, style} for ids 1..=0x83b.
const LAYOUT_RECORDS: DataAddress = DataAddress(0x0013_0000);
/// 66-byte records for ids 0x83c..=0x83d, cell width and height at +0x3c.
const SPECIAL_RECORDS: DataAddress = DataAddress(0x0037_b784);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FontFile { Text, Icon }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuestAssetError {
    Resource(ResourceError),
    Bitmap(BitmapError),
    Raster(RasterError),
    Font { file: FontFile, error: FontError },
    /// `xm1_draw_resource` returns -1 for catalog ids while the table base is zero (0x01179f08..0x01179f14).
    CatalogUnset,
    /// The loaded-font word is outside the seven text fonts 0x006aa750 accepts (0x006aa758).
    TextFontIndex(u32),
    RegistryAddress { file: FontFile, address: DataAddress },
    /// `ov48b_configure_font` rejects a width or height of 0 or above 100.
    TextSizeRejected { width: u16, height: u16 },
    PaletteIndex(PaletteIndex),
    MissingGlyph(char),
    UnsupportedCharacter(char),
}
impl From<ResourceError> for GuestAssetError { fn from(error: ResourceError) -> Self { Self::Resource(error) } }
impl From<BitmapError> for GuestAssetError { fn from(error: BitmapError) -> Self { Self::Bitmap(error) } }
impl From<RasterError> for GuestAssetError { fn from(error: RasterError) -> Self { Self::Raster(error) } }

pub const fn whole_ram(image: &[u8]) -> DataRegion<'_> { DataRegion { base: DataAddress(0), bytes: image } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CatalogEntry { pub id: CatalogTextId, pub variant: TextVariant }
#[derive(Clone, Copy, Debug)]
pub struct PackRequest<'r> { pub palette: &'r [PaletteIndex], pub images: &'r [ResourceId], pub texts: &'r [CatalogEntry] }

pub struct GuestAssets<'a> {
    regions: &'a [DataRegion<'a>],
    catalog: LanguageCatalog,
    text_font: FontLanguage,
    text: FdlResource<'a>,
    icon: FdlResource<'a>,
}

impl<'a> GuestAssets<'a> {
    pub fn bind(regions: &'a [DataRegion<'a>]) -> Result<Self, GuestAssetError> {
        let memory = ResourceMemory::new(regions)?;
        let language = LanguageId::new(word(&memory, RENDER_LANGUAGE)? as i32);
        let table = word(&memory, CATALOG_TABLE)?;
        if table == 0 { return Err(GuestAssetError::CatalogUnset); }
        let index = word(&memory, LOADED_TEXT_FONT)?;
        let text_font = font_language(index).ok_or(GuestAssetError::TextFontIndex(index))?;
        let text = registered_font(&memory, FontFile::Text, index, TEXT_FONT_BASE)?;
        let icon = registered_font(&memory, FontFile::Icon, ICON_REGISTRY_INDEX, ICON_FONT_BASE)?;
        Ok(Self {
            regions,
            catalog: LanguageCatalog { language, records: DataAddress(table) },
            text_font,
            text: FdlResource::parse(text).map_err(|error| GuestAssetError::Font { file: FontFile::Text, error })?,
            icon: FdlResource::parse(icon).map_err(|error| GuestAssetError::Font { file: FontFile::Icon, error })?,
        })
    }

    pub const fn language(&self) -> LanguageId { self.catalog.language }
    pub const fn text_font(&self) -> FontLanguage { self.text_font }
    pub const fn font_bytes(&self, file: FontFile) -> &'a [u8] {
        match file { FontFile::Text => self.text.bytes(), FontFile::Icon => self.icon.bytes() }
    }
    pub fn memory(&self) -> Result<ResourceMemory<'a>, GuestAssetError> { Ok(ResourceMemory::new(self.regions)?) }
    pub fn bitmaps(&self) -> Result<BitmapCatalog<'a>, GuestAssetError> { Ok(BitmapCatalog { memory: self.memory()? }) }

    pub fn catalog_text<'out>(&self, id: CatalogTextId, variant: TextVariant, output: &'out mut [u8]) -> Result<&'out str, GuestAssetError> {
        let set = CatalogSet { memory: self.memory()?, catalogs: core::slice::from_ref(&self.catalog) };
        Ok(set.text(self.catalog.language, id, variant, output)?)
    }

    /// Size of catalog text drawn by `overlay_draw_grid_resource_with_mode` (0x011121d4): the layout cell
    /// (0x0117dce0, 0x0117dd48) times the variant's percentages (0x0117dee0), divided by 100 (0x0117a554..0x0117a590).
    pub fn grid_text_size(&self, id: CatalogTextId, variant: TextVariant) -> Result<FontSize, GuestAssetError> {
        let memory = self.memory()?;
        let id = id.value();
        if id == 0 { return Err(GuestAssetError::TextSizeRejected { width: 0, height: 0 }); }
        let (cell, percent) = if id <= 0x83b {
            let cell = halves(&memory, offset(LAYOUT_RECORDS, id * 10 + 4)?)?;
            let entry = offset(self.catalog.records, id * 8)?;
            if variant.0 >= memory.bytes(offset(entry, 2)?, 1)?[0] { return Err(ResourceError::Variant.into()); }
            let metrics = word(&memory, offset(entry, 4)?)?;
            let percent = if metrics == 0 { [100, 100] } else { halves(&memory, offset(DataAddress(metrics), u32::from(variant.0) * 12 + 4)?)? };
            (cell, percent)
        } else {
            (halves(&memory, offset(SPECIAL_RECORDS, (id - 0x83c) * 0x42 + 0x3c)?)?, [100, 100])
        };
        let scale = |percent: u16, cell: u16| (i32::from(percent as i16) * i32::from(cell) / 100) as u16;
        let (width, height) = (scale(percent[0], cell[0]), scale(percent[1], cell[1]));
        if !(1..=100).contains(&width) || !(1..=100).contains(&height) { return Err(GuestAssetError::TextSizeRejected { width, height }); }
        Ok(FontSize { width: PixelExtent::new(u32::from(width)), height: PixelExtent::new(u32::from(height)) })
    }

    pub fn grid_text_config(&self, id: CatalogTextId, variant: TextVariant, coverage: CoverageEncoding) -> Result<FontConfig, GuestAssetError> {
        Ok(FontConfig { size: self.grid_text_size(id, variant)?, coverage, writing: WritingMode::Horizontal })
    }

    pub fn glyph_font(&self, character: char) -> Result<FontFile, GuestAssetError> {
        let glyph = GlyphId::new(character as u32);
        for (file, font) in [(FontFile::Text, &self.text), (FontFile::Icon, &self.icon)] {
            match font.glyph_program(glyph, WritingMode::Horizontal) {
                Ok(_) => return Ok(file),
                Err(FontError::MissingGlyph(_)) => {}
                Err(error) => return Err(GuestAssetError::Font { file, error }),
            }
        }
        Err(GuestAssetError::MissingGlyph(character))
    }

    pub fn engine<'w>(&self, file: FontFile, config: FontConfig, workspace: FontWorkspace<'w>) -> Result<FontEngine<'a, 'w>, GuestAssetError> {
        let asset = FontAsset::parse(self.font_bytes(file)).map_err(|error| GuestAssetError::Font { file, error })?;
        FontEngine::new(asset, config, workspace).map_err(|error| GuestAssetError::Font { file, error })
    }
    pub fn glyph_engines<'w>(&self, config: FontConfig, text: FontWorkspace<'w>, icon: FontWorkspace<'w>) -> Result<GlyphEngines<'a, 'w>, GuestAssetError> {
        Ok(GlyphEngines { text: self.engine(FontFile::Text, config, text)?, icon: self.engine(FontFile::Icon, config, icon)? })
    }
    pub fn font_binding<'w>(&self, id: FontId, file: FontFile, config: FontConfig, workspace: FontWorkspace<'w>) -> Result<FontBinding<'a, 'w>, GuestAssetError> {
        Ok(FontBinding::new(id, self.font_bytes(file), config, workspace)?)
    }

    /// The draw path reads the palette index as a byte (0x0117a044).
    pub fn palette_rgb(&self, index: PaletteIndex) -> Result<Rgba, GuestAssetError> {
        if index.0 > 0xff { return Err(GuestAssetError::PaletteIndex(index)); }
        let rgb = self.memory()?.bytes(offset(PALETTE, index.0 * 3)?, 3)?;
        Ok(Rgba { red: rgb[0], green: rgb[1], blue: rgb[2], alpha: 0xff })
    }

    /// Writes an RCP1 pack of palette colours, native RGBA bitmaps and catalog texts; each list ascends by id.
    /// `scratch` receives each bitmap's single-record extraction before it is copied.
    pub fn pack<'out>(&self, request: PackRequest<'_>, provenance: AssetProvenance<'out>, scratch: &mut [u8], output: &'out mut [u8]) -> Result<AssetPack<'out>, GuestAssetError> {
        let mut position = 8;
        let mut count = 0u32;
        for &index in request.palette {
            let colour = self.palette_rgb(index)?;
            let record = output.get_mut(position..position + 28).ok_or(ResourceError::Capacity)?;
            record_header(record, 1, index.0, 4);
            record[24..].copy_from_slice(&[colour.red, colour.green, colour.blue, colour.alpha]);
            position += 28;
            count += 1;
        }
        let bitmaps = self.bitmaps()?;
        for &resource in request.images {
            bitmaps.extract_rcp1(resource, provenance, &mut *scratch)?;
            let length = 24 + u32::from_le_bytes([scratch[28], scratch[29], scratch[30], scratch[31]]) as usize;
            output.get_mut(position..position + length).ok_or(ResourceError::Capacity)?.copy_from_slice(&scratch[8..8 + length]);
            position += length;
            count += 1;
        }
        for entry in request.texts {
            let payload = output.get_mut(position + 24..).ok_or(ResourceError::Capacity)?;
            let length = self.catalog_text(entry.id, entry.variant, payload)?.len();
            record_header(&mut output[position..position + 24], 4, entry.id.value(), length as u32);
            position += 24 + length;
            count += 1;
        }
        let header = output.get_mut(..8).ok_or(ResourceError::Capacity)?;
        header[..4].copy_from_slice(b"RCP1");
        header[4..].copy_from_slice(&count.to_le_bytes());
        let bytes: &'out [u8] = output;
        Ok(AssetPack::parse(&bytes[..position], provenance)?)
    }
}

pub struct GlyphEngines<'a, 'w> { pub text: FontEngine<'a, 'w>, pub icon: FontEngine<'a, 'w> }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlyphRun { pub glyphs: u32, pub advance: i32 }

impl<'a, 'w> GlyphEngines<'a, 'w> {
    pub fn engine(&mut self, file: FontFile) -> &mut FontEngine<'a, 'w> {
        match file { FontFile::Text => &mut self.text, FontFile::Icon => &mut self.icon }
    }
    /// Draws each character with the font `GuestAssets::glyph_font` selects and moves the pen by its
    /// horizontal advance; no shaping and no engine advance bias.
    pub fn draw(&mut self, assets: &GuestAssets<'_>, text: &str, baseline: PanelPoint, colour: Rgba, surface: &mut Surface<'_>) -> Result<GlyphRun, GuestAssetError> {
        let mut run = GlyphRun { glyphs: 0, advance: 0 };
        for character in text.chars() {
            let code = u16::try_from(character as u32).map_err(|_| GuestAssetError::UnsupportedCharacter(character))?;
            let file = assets.glyph_font(character)?;
            let mask = self.engine(file).glyph_mask(GlyphId::new(u32::from(code))).map_err(|error| GuestAssetError::Font { file, error })?;
            let x = i32::from(baseline.x()).checked_add(run.advance).and_then(|x| u16::try_from(x).ok());
            let pen = x.and_then(|x| PanelPoint::new(x, baseline.y())).ok_or(RasterError::TextPositionOverflow)?;
            surface.draw_glyph_mask(pen, &mask, colour)?;
            run.advance = run.advance.checked_add(mask.metrics().advance_x.get()).ok_or(RasterError::TextPositionOverflow)?;
            run.glyphs += 1;
        }
        Ok(run)
    }
}

/// FontLanguage order is the registry and load-table (0x007c8f9c) order.
fn font_language(index: u32) -> Option<FontLanguage> {
    Some(match index {
        0 => FontLanguage::Zero, 1 => FontLanguage::One, 2 => FontLanguage::Two, 3 => FontLanguage::Three,
        4 => FontLanguage::Four, 5 => FontLanguage::Five, 6 => FontLanguage::Six,
        _ => return None,
    })
}
fn registered_font<'a>(memory: &ResourceMemory<'a>, file: FontFile, index: u32, base: DataAddress) -> Result<&'a [u8], GuestAssetError> {
    let record = offset(FONT_REGISTRY, index * 24)?;
    let address = DataAddress(word(memory, offset(record, 8)?)?);
    if address != base { return Err(GuestAssetError::RegistryAddress { file, address }); }
    Ok(memory.bytes(address, word(memory, offset(record, 12)?)? as usize)?)
}
fn record_header(header: &mut [u8], kind: u8, id: u32, length: u32) {
    header[..24].fill(0);
    header[0] = kind;
    header[4..8].copy_from_slice(&id.to_le_bytes());
    header[20..24].copy_from_slice(&length.to_le_bytes());
}
fn offset(address: DataAddress, delta: u32) -> Result<DataAddress, ResourceError> {
    address.0.checked_add(delta).map(DataAddress).ok_or(ResourceError::AddressOverflow)
}
fn word(memory: &ResourceMemory<'_>, address: DataAddress) -> Result<u32, ResourceError> {
    let bytes = memory.bytes(address, 4)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}
fn halves(memory: &ResourceMemory<'_>, address: DataAddress) -> Result<[u16; 2], ResourceError> {
    let bytes = memory.bytes(address, 4)?;
    Ok([u16::from_le_bytes([bytes[0], bytes[1]]), u16::from_le_bytes([bytes[2], bytes[3]])])
}

#[cfg(test)]
#[path = "guest_assets_tests.rs"]
mod tests;

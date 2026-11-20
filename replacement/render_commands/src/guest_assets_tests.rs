extern crate std;
use std::vec;
use std::vec::Vec;
use super::*;
use super::super::commands::{Alpha, Colour, DrawCommand, DrawList, TextRef};
use super::super::font_core::raster::RasterWorkspace;
use super::super::raster::{FontRegistry, RasterResources};
use super::super::{PanelRect, SurfaceId};

/// The all-ink rectangle program of `develop/src/font_core/raster.rs` tests: an original program covering the full em.
const RECTANGLE: &[u8] = &[0xfa, 0, 14, 0, 0x90, 1, 0, 0, 0xff, 0, 0x94, 0x23, 0, 0xff, 0xff, 0xff, 0x71, 0];

/// A dense format-4 FDL holding only `code`.
fn font(code: u16) -> Vec<u8> {
    let count = usize::from(code) + 1;
    let directory = 0x40;
    let data = directory + (count + 1) * 4;
    let mut bytes = vec![0; data + 6 + RECTANGLE.len()];
    bytes[..8].copy_from_slice(b"FDL Font");
    bytes[0x1c..0x1e].copy_from_slice(&(count as u16).to_le_bytes());
    bytes[0x2e..0x32].copy_from_slice(&(directory as u32).to_le_bytes());
    bytes[0x32..0x36].copy_from_slice(&(data as u32).to_le_bytes());
    bytes[0x36..0x38].copy_from_slice(&4u16.to_le_bytes());
    bytes[0x38..0x3a].copy_from_slice(&0x100u16.to_le_bytes());
    let entry = directory + usize::from(code) * 4;
    bytes[entry..entry + 4].copy_from_slice(&0x0100_0000u32.to_le_bytes());
    bytes[entry + 4..entry + 8].copy_from_slice(&(6 + RECTANGLE.len() as u32).to_le_bytes());
    bytes[data + 2..data + 4].copy_from_slice(&500u16.to_le_bytes());
    bytes[data + 4..data + 6].copy_from_slice(&(RECTANGLE.len() as u16).to_le_bytes());
    bytes[data + 6..].copy_from_slice(RECTANGLE);
    bytes
}

fn words(values: &[u32]) -> Vec<u8> { values.iter().flat_map(|value| value.to_le_bytes()).collect() }
fn halves(values: &[u16]) -> Vec<u8> { values.iter().flat_map(|value| value.to_le_bytes()).collect() }
fn utf16(text: &str) -> Vec<u8> { text.encode_utf16().chain([0]).flat_map(|unit| unit.to_le_bytes()).collect() }

const TABLE: u32 = 0x0039_f754;
const METRICS: u32 = 0x003a_0000;

/// Synthetic guest RAM laid out at the stock addresses `GuestAssets` reads.
struct Ram { blocks: Vec<(u32, Vec<u8>)> }
impl Ram {
    /// Language 1, LATIN loaded, catalog id 0x31 with variants "AB" (100%, 100%) and "A" (50%, 300%) in a 30x36 cell,
    /// id 0x83c in a 20x24 cell, text font holding 'A', icon font holding 'B', palette entry 2 = 11 22 33.
    fn stock() -> Self {
        let text = font(u16::from(b'A'));
        let icon = font(u16::from(b'B'));
        let mut registry = vec![0u8; 8 * 24];
        registry[5 * 24 + 8..5 * 24 + 16].copy_from_slice(&words(&[0x0045_c000, text.len() as u32]));
        registry[7 * 24 + 8..7 * 24 + 16].copy_from_slice(&words(&[0x003b_c000, icon.len() as u32]));
        let mut table = vec![0u8; 0x31 * 8 + 8];
        table[0x31 * 8 + 2] = 2;
        table[0x31 * 8 + 4..0x31 * 8 + 8].copy_from_slice(&METRICS.to_le_bytes());
        let mut metrics = vec![0u8; 0x140];
        metrics[..8].copy_from_slice(&words(&[METRICS + 0x100, 100 | 100 << 16]));
        metrics[12..20].copy_from_slice(&words(&[METRICS + 0x120, 50 | 300 << 16]));
        metrics[0x100..0x106].copy_from_slice(&utf16("AB"));
        metrics[0x120..0x124].copy_from_slice(&utf16("A"));
        let mut special = vec![0u8; 0x42];
        special[0x3c..0x40].copy_from_slice(&halves(&[20, 24]));
        let mut palette = vec![0u8; 9];
        palette[6..9].copy_from_slice(&[0x11, 0x22, 0x33]);
        Self { blocks: vec![
            (0x0013_0000 + 0x31 * 10, halves(&[500, 0x90, 30, 36, 48])),
            (0x0037_b784, special),
            (TABLE, table),
            (METRICS, metrics),
            (0x003b_c000, icon),
            (0x0045_c000, text),
            (0x007a_1f24, palette),
            (0x0095_53c4, words(&[5])),
            (0x0095_a6a4, words(&[1])),
            (0x0117_8aac, words(&[TABLE])),
            (0x0118_04a4, registry),
        ] }
    }
    fn set(&mut self, address: u32, bytes: &[u8]) {
        let (base, block) = self.blocks.iter_mut().find(|(base, block)| *base <= address && address as usize + bytes.len() <= *base as usize + block.len()).unwrap();
        let start = (address - *base) as usize;
        block[start..start + bytes.len()].copy_from_slice(bytes);
    }
    fn regions(&self) -> Vec<DataRegion<'_>> {
        self.blocks.iter().map(|(base, bytes)| DataRegion { base: DataAddress(*base), bytes }).collect()
    }
    fn image(&self) -> Vec<u8> {
        let end = self.blocks.iter().map(|(base, bytes)| *base as usize + bytes.len()).max().unwrap();
        let mut image = vec![0; end];
        for (base, bytes) in &self.blocks { image[*base as usize..*base as usize + bytes.len()].copy_from_slice(bytes); }
        image
    }
}

fn id(value: u32) -> CatalogTextId { CatalogTextId::new(value).unwrap() }
fn size(width: u32, height: u32) -> FontSize { FontSize { width: PixelExtent::new(width), height: PixelExtent::new(height) } }
fn eight() -> FontConfig { FontConfig { size: size(8, 8), coverage: CoverageEncoding::PackedMsb1, writing: WritingMode::Horizontal } }
fn pixel(bytes: &[u8], x: usize, y: usize) -> [u8; 4] { let at = (y * 720 + x) * 4; [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]] }

#[test]
fn binds_the_resident_catalog_and_loaded_text_font() {
    let ram = Ram::stock();
    let regions = ram.regions();
    let assets = GuestAssets::bind(&regions).unwrap();
    assert_eq!(assets.language().value(), 1);
    assert_eq!(assets.text_font(), FontLanguage::Five);
    let mut output = [0u8; 16];
    assert_eq!(assets.catalog_text(id(0x31), TextVariant(0), &mut output), Ok("AB"));
    assert_eq!(assets.catalog_text(id(0x31), TextVariant(1), &mut output), Ok("A"));
    assert_eq!(assets.font_bytes(FontFile::Text), &font(u16::from(b'A'))[..]);
    assert_eq!(assets.font_bytes(FontFile::Icon), &font(u16::from(b'B'))[..]);
}

#[test]
fn grid_text_size_scales_variant_percentages_by_the_layout_cell() {
    let ram = Ram::stock();
    let regions = ram.regions();
    let assets = GuestAssets::bind(&regions).unwrap();
    assert_eq!(assets.grid_text_size(id(0x31), TextVariant(0)), Ok(size(30, 36)));
    assert_eq!(assets.grid_text_size(id(0x31), TextVariant(1)), Err(GuestAssetError::TextSizeRejected { width: 15, height: 108 }));
    assert_eq!(assets.grid_text_size(id(0x31), TextVariant(2)), Err(GuestAssetError::Resource(ResourceError::Variant)));
    assert_eq!(assets.grid_text_size(id(0x83c), TextVariant(0)), Ok(size(20, 24)));
    assert_eq!(assets.grid_text_size(id(0), TextVariant(0)), Err(GuestAssetError::TextSizeRejected { width: 0, height: 0 }));
    let config = assets.grid_text_config(id(0x31), TextVariant(0), CoverageEncoding::Gray4).unwrap();
    assert_eq!((config.size, config.coverage), (size(30, 36), CoverageEncoding::Gray4));
}

#[test]
fn negative_percentages_are_refused() {
    let mut ram = Ram::stock();
    ram.set(METRICS + 4, &halves(&[0xff9c, 100]));
    let regions = ram.regions();
    let assets = GuestAssets::bind(&regions).unwrap();
    assert_eq!(assets.grid_text_size(id(0x31), TextVariant(0)), Err(GuestAssetError::TextSizeRejected { width: 65506, height: 36 }));
}

#[test]
fn glyphs_come_from_the_text_font_then_the_icon_font() {
    let ram = Ram::stock();
    let regions = ram.regions();
    let assets = GuestAssets::bind(&regions).unwrap();
    assert_eq!(assets.glyph_font('A'), Ok(FontFile::Text));
    assert_eq!(assets.glyph_font('B'), Ok(FontFile::Icon));
    assert_eq!(assets.glyph_font('C'), Err(GuestAssetError::MissingGlyph('C')));
}

#[test]
fn unbound_globals_and_registry_entries_are_refused() {
    let mut ram = Ram::stock();
    ram.set(0x0095_53c4, &words(&[7]));
    assert_eq!(GuestAssets::bind(&ram.regions()).err(), Some(GuestAssetError::TextFontIndex(7)));
    let mut ram = Ram::stock();
    ram.set(0x0118_04a4 + 7 * 24 + 8, &words(&[0x003b_d000]));
    assert_eq!(GuestAssets::bind(&ram.regions()).err(), Some(GuestAssetError::RegistryAddress { file: FontFile::Icon, address: DataAddress(0x003b_d000) }));
    let mut ram = Ram::stock();
    ram.set(0x0117_8aac, &words(&[0]));
    assert_eq!(GuestAssets::bind(&ram.regions()).err(), Some(GuestAssetError::CatalogUnset));
}

#[test]
fn glyph_run_draws_text_and_icon_glyphs_at_their_advances() {
    // Two raster workspaces exceed the default test-thread stack.
    std::thread::Builder::new().stack_size(64 << 20).spawn(glyph_run_scenario).unwrap().join().unwrap();
}

fn glyph_run_scenario() {
    let ram = Ram::stock();
    let regions = ram.regions();
    let assets = GuestAssets::bind(&regions).unwrap();
    let (mut text_raster, mut icon_raster) = (RasterWorkspace::new(), RasterWorkspace::new());
    let (mut text_pixels, mut icon_pixels) = ([0u8; 64], [0u8; 64]);
    let mut engines = assets.glyph_engines(eight(), FontWorkspace::fdl(&mut text_raster, &mut text_pixels), FontWorkspace::fdl(&mut icon_raster, &mut icon_pixels)).unwrap();
    let mut bytes = vec![0u8; SurfaceId::Osd.byte_len()];
    let mut surface = Surface::new(SurfaceId::Osd, &mut bytes).unwrap();
    let red = Rgba { red: 255, green: 0, blue: 0, alpha: 255 };
    assert_eq!(engines.draw(&assets, "AB", PanelPoint::new(10, 20).unwrap(), red, &mut surface), Ok(GlyphRun { glyphs: 2, advance: 8 }));
    assert_eq!(engines.draw(&assets, "C", PanelPoint::new(10, 20).unwrap(), red, &mut surface), Err(GuestAssetError::MissingGlyph('C')));
    // A 500-per-mille advance at an 8-pixel em is 4 pixels; each mask covers the 8x8 em above the baseline.
    assert_eq!(pixel(&bytes, 10, 13), [255, 0, 0, 255]);
    assert_eq!(pixel(&bytes, 21, 20), [255, 0, 0, 255]);
    assert_eq!(pixel(&bytes, 22, 20), [0; 4]);
    assert_eq!(pixel(&bytes, 9, 13), [0; 4]);
    assert_eq!(pixel(&bytes, 10, 21), [0; 4]);
}

#[test]
fn pack_carries_palette_rgb_and_catalog_text_to_the_renderer() {
    let ram = Ram::stock();
    let regions = ram.regions();
    let assets = GuestAssets::bind(&regions).unwrap();
    let mut storage = [0u8; 256];
    let request = PackRequest { palette: &[PaletteIndex(2)], images: &[], texts: &[CatalogEntry { id: id(0x31), variant: TextVariant(1) }] };
    let pack = assets.pack(request, AssetProvenance::PrivateExtract { source: "synthetic guest RAM" }, &mut [], &mut storage).unwrap();
    let mut raster = RasterWorkspace::new();
    let mut glyph_pixels = [0u8; 64];
    let mut bindings = [assets.font_binding(FontId(0), FontFile::Text, eight(), FontWorkspace::fdl(&mut raster, &mut glyph_pixels)).unwrap()];
    let mut resources = RasterResources::new(&pack, FontRegistry::Engines(&mut bindings), &[], None).unwrap();
    let colour = Colour { palette: PaletteIndex(2), alpha: Alpha::OPAQUE };
    let commands = [
        DrawCommand::Fill { rect: PanelRect::new(PanelPoint::new(0, 0).unwrap(), 4, 4).unwrap(), colour },
        DrawCommand::Text { origin: PanelPoint::new(10, 20).unwrap(), text: TextRef::Catalog(id(0x31)), font: FontId(0), colour },
    ];
    let mut bytes = vec![0u8; SurfaceId::Osd.byte_len()];
    Surface::new(SurfaceId::Osd, &mut bytes).unwrap().render(DrawList::new(SurfaceId::Osd, &commands), &mut resources).unwrap();
    assert_eq!(pixel(&bytes, 0, 0), [0x11, 0x22, 0x33, 255]);
    assert_eq!(pixel(&bytes, 10, 13), [0x11, 0x22, 0x33, 255]);
    assert_eq!(pixel(&bytes, 30, 13), [0; 4]);
    let refused = PackRequest { palette: &[PaletteIndex(256)], images: &[], texts: &[] };
    assert_eq!(assets.pack(refused, AssetProvenance::PrivateExtract { source: "synthetic guest RAM" }, &mut [], &mut storage).err(), Some(GuestAssetError::PaletteIndex(PaletteIndex(256))));
}

#[test]
fn a_whole_ram_image_binds_like_its_regions() {
    let image = Ram::stock().image();
    let regions = [whole_ram(&image)];
    let assets = GuestAssets::bind(&regions).unwrap();
    let mut output = [0u8; 16];
    assert_eq!(assets.catalog_text(id(0x31), TextVariant(0), &mut output), Ok("AB"));
    assert_eq!(assets.grid_text_size(id(0x31), TextVariant(0)), Ok(size(30, 36)));
    assert_eq!(assets.palette_rgb(PaletteIndex(2)), Ok(Rgba { red: 0x11, green: 0x22, blue: 0x33, alpha: 255 }));
}

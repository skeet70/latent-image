//! Checks `GuestAssets` against a full guest RAM dump (bytes from address 0).
//! Usage: guest_assets_check <ram-dump> <output-dir>
//! Writes 720x480 straight-alpha RGBA renders and `summary.json`; exits 1 when a check fails.

#[path = "../src/lib.rs"]
#[allow(dead_code, unused_imports, unused_attributes)]
mod render_commands;

use render_commands::commands::{Alpha, DrawCommand, DrawList, Rgba, ShapePaint};
use render_commands::font_core::raster::RasterWorkspace;
use render_commands::font_core::{CoverageEncoding, FontWorkspace, GlyphId};
use render_commands::guest_assets::{whole_ram, FontFile, GuestAssets, PackRequest};
use render_commands::raster::{AssetProvenance, RasterResources, Surface, VectorWorkspace};
use render_commands::resources::{BitmapKind, TextVariant};
use render_commands::shape_assets::ShapeAsset;
use render_commands::vector_raster::{Crossing, CurveAccuracy, DashBudget, Event, SampleGrid, Scanlines, Span, StrokePrimitive, Vertex};
use render_commands::{CatalogTextId, PaletteIndex, PanelPoint, ResourceId, SurfaceId, PANEL_WIDTH};
use std::fmt::Write as _;
use std::path::Path;

/// Labels of the picker-15 pane frame.
const TEXT_IDS: [u32; 4] = [0x432, 0x449, 0x31, 0x35];
const BITMAPS: [u32; 11] = [0xff0000e8, 0xff0000f3, 0xff0000e7, 0xff000216, 0xff000217, 0xff000218, 0xff000219, 0xff00021a, 0xff00021b, 0xff00021c, 0xff00021d];
const PALETTE: usize = 0x007a_1f24;
const COVERAGE: CoverageEncoding = CoverageEncoding::Gray8;
const WHITE: Rgba = Rgba { red: 255, green: 255, blue: 255, alpha: 255 };

struct Report { json: String, failures: Vec<String> }
impl Report {
    fn fail(&mut self, message: String) { eprintln!("FAIL {message}"); self.failures.push(message); }
}

fn quoted(text: &str) -> String {
    let mut output = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            ' '..='~' => output.push(character),
            _ => for unit in character.encode_utf16(&mut [0; 2]) { write!(output, "\\u{unit:04x}").unwrap(); },
        }
    }
    output.push('"');
    output
}

fn write_rgba(directory: &Path, name: &str, pixels: &[u8]) {
    std::fs::write(directory.join(name), pixels).unwrap_or_else(|error| panic!("writing {name}: {error}"));
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.len() != 3 {
        eprintln!("usage: {} <ram-dump> <output-dir>", arguments[0]);
        std::process::exit(2);
    }
    let image = std::fs::read(&arguments[1]).unwrap_or_else(|error| panic!("reading {}: {error}", arguments[1]));
    let directory = Path::new(&arguments[2]);
    std::fs::create_dir_all(directory).unwrap_or_else(|error| panic!("creating {}: {error}", directory.display()));
    let regions = [whole_ram(&image)];
    let assets = GuestAssets::bind(&regions).unwrap_or_else(|error| panic!("binding guest assets: {error:?}"));
    let mut report = Report { json: String::new(), failures: Vec::new() };
    write!(report.json, "{{\n  \"dump\": {},\n  \"dump_bytes\": {},\n  \"render_format\": \"RGBA8 straight alpha, 720x480, row-major\",\n", quoted(&arguments[1]), image.len()).unwrap();
    write!(report.json, "  \"language\": {},\n  \"text_font\": \"{:?}\",\n  \"text_font_bytes\": {},\n  \"icon_font_bytes\": {},\n  \"coverage\": \"{COVERAGE:?}\",\n",
        assets.language().value(), assets.text_font(), assets.font_bytes(FontFile::Text).len(), assets.font_bytes(FontFile::Icon).len()).unwrap();
    texts(&assets, directory, &mut report);
    bitmaps(&assets, directory, &mut report);
    palette(&assets, &image, &mut report);
    report.json.push_str("  \"failures\": [");
    let failures: Vec<String> = report.failures.iter().map(|failure| quoted(failure)).collect();
    report.json.push_str(&failures.join(", "));
    report.json.push_str("]\n}\n");
    std::fs::write(directory.join("summary.json"), &report.json).unwrap_or_else(|error| panic!("writing summary.json: {error}"));
    if !report.failures.is_empty() { std::process::exit(1); }
}

fn texts(assets: &GuestAssets<'_>, directory: &Path, report: &mut Report) {
    report.json.push_str("  \"texts\": [\n");
    let mut text_raster = Box::new(RasterWorkspace::new());
    let mut icon_raster = Box::new(RasterWorkspace::new());
    let mut text_pixels = vec![0u8; 1 << 16];
    let mut icon_pixels = vec![0u8; 1 << 16];
    let mut entries = Vec::new();
    for value in TEXT_IDS {
        let mut entry = format!("    {{\"id\": \"0x{value:x}\", \"variant\": 0");
        text_entry(assets, value, directory, report, &mut entry, [&mut *text_raster, &mut *icon_raster], [&mut text_pixels, &mut icon_pixels]);
        entry.push('}');
        entries.push(entry);
    }
    report.json.push_str(&entries.join(",\n"));
    report.json.push_str("\n  ],\n");
}

fn text_entry(assets: &GuestAssets<'_>, value: u32, directory: &Path, report: &mut Report, entry: &mut String, rasters: [&mut RasterWorkspace; 2], pixels: [&mut Vec<u8>; 2]) {
    let id = CatalogTextId::new(value).unwrap();
    let variant = TextVariant(0);
    let mut storage = [0u8; 512];
    let text = match assets.catalog_text(id, variant, &mut storage) {
        Ok(text) => text.to_owned(),
        Err(error) => return report.fail(format!("catalog 0x{value:x}: {error:?}")),
    };
    if text.is_empty() { report.fail(format!("catalog 0x{value:x}: empty text")); }
    write!(entry, ", \"text\": {}", quoted(&text)).unwrap();
    let config = match assets.grid_text_config(id, variant, COVERAGE) {
        Ok(config) => config,
        Err(error) => return report.fail(format!("size 0x{value:x}: {error:?}")),
    };
    write!(entry, ", \"size\": [{}, {}]", config.size.width.get(), config.size.height.get()).unwrap();
    let [text_raster, icon_raster] = rasters;
    let [text_pixels, icon_pixels] = pixels;
    let mut engines = match assets.glyph_engines(config, FontWorkspace::fdl(text_raster, text_pixels), FontWorkspace::fdl(icon_raster, icon_pixels)) {
        Ok(engines) => engines,
        Err(error) => return report.fail(format!("engines 0x{value:x}: {error:?}")),
    };
    let mut glyphs = Vec::new();
    for character in text.chars() {
        let code = character as u32;
        let file = match assets.glyph_font(character) {
            Ok(file) => file,
            Err(error) => { report.fail(format!("0x{value:x} U+{code:04X}: {error:?}")); continue; }
        };
        let engine = engines.engine(file);
        let advance = match engine.advance(GlyphId::new(code)) {
            Ok(advance) => advance.get(),
            Err(error) => { report.fail(format!("0x{value:x} U+{code:04X} advance: {error:?}")); continue; }
        };
        let mask = match engine.glyph_mask(GlyphId::new(code)) {
            Ok(mask) => mask,
            Err(error) => { report.fail(format!("0x{value:x} U+{code:04X} mask: {error:?}")); continue; }
        };
        let (width, height, stride) = (mask.width().get() as usize, mask.height().get() as usize, mask.stride().get());
        let ink = (0..height).map(|row| mask.pixels()[row * stride..row * stride + width].iter().filter(|sample| **sample != 0).count()).sum::<usize>();
        if advance <= 0 { report.fail(format!("0x{value:x} U+{code:04X}: advance {advance}")); }
        if ink == 0 && !character.is_whitespace() { report.fail(format!("0x{value:x} U+{code:04X}: blank mask")); }
        glyphs.push(format!("{{\"code\": \"U+{code:04X}\", \"font\": \"{file:?}\", \"advance\": {advance}, \"mask\": [{width}, {height}], \"ink\": {ink}}}"));
    }
    write!(entry, ", \"glyphs\": [{}]", glyphs.join(", ")).unwrap();
    let mut surface_pixels = vec![0u8; SurfaceId::Osd.byte_len()];
    let name = format!("text-{value:04x}.rgba");
    let run = {
        let mut surface = Surface::new(SurfaceId::Osd, &mut surface_pixels).unwrap();
        engines.draw(assets, &text, PanelPoint::new(20, 60).unwrap(), WHITE, &mut surface)
    };
    match run {
        Ok(run) => write!(entry, ", \"run\": {{\"glyphs\": {}, \"advance\": {}}}", run.glyphs, run.advance).unwrap(),
        Err(error) => report.fail(format!("render 0x{value:x}: {error:?}")),
    }
    write_rgba(directory, &name, &surface_pixels);
    write!(entry, ", \"render\": \"{name}\"").unwrap();
}

fn bitmaps(assets: &GuestAssets<'_>, directory: &Path, report: &mut Report) {
    report.json.push_str("  \"bitmaps\": [\n");
    let memory = assets.memory().unwrap();
    let catalog = assets.bitmaps().unwrap();
    let mut vertices = vec![Vertex::EMPTY; 1 << 16];
    let mut strokes = vec![StrokePrimitive::EMPTY; 1 << 16];
    let mut crossings = vec![Crossing::EMPTY; 1 << 16];
    let mut spans = vec![Span::EMPTY; 2 << 16];
    let mut events = vec![Event::EMPTY; 2 * PANEL_WIDTH];
    let mut entries = Vec::new();
    for value in BITMAPS {
        let resource = ResourceId(value);
        let entry = match catalog.entry(resource) {
            Ok(entry) => entry,
            Err(error) => { report.fail(format!("bitmap 0x{value:08x}: {error:?}")); continue; }
        };
        let mut pixels = vec![0u8; SurfaceId::Osd.byte_len()];
        let origin = PanelPoint::new(0, 0).unwrap();
        let drawn = match entry.kind {
            BitmapKind::NativeRgba => {
                let length = 32 + usize::from(entry.width) * usize::from(entry.height) * 4;
                let (mut scratch, mut storage) = (vec![0u8; length], vec![0u8; length]);
                let images = [resource];
                let request = PackRequest { palette: &[], images: &images, texts: &[] };
                assets.pack(request, AssetProvenance::PrivateExtract { source: "guest RAM dump" }, &mut scratch, &mut storage)
                    .map_err(|error| format!("{error:?}"))
                    .and_then(|pack| {
                        let commands = [DrawCommand::Image { origin, image: entry.image, alpha: Alpha::OPAQUE }];
                        let mut resources = RasterResources::coverage8(&pack);
                        Surface::new(SurfaceId::Osd, &mut pixels).unwrap().render(DrawList::new(SurfaceId::Osd, &commands), &mut resources).map_err(|error| format!("{error:?}"))
                    })
            }
            BitmapKind::ShapeKindOne | BitmapKind::ShapeKindTwo => {
                let scanlines = Scanlines { crossings: &mut crossings, spans: &mut spans, events: &mut events };
                let mut workspace = VectorWorkspace { vertices: &mut vertices, strokes: &mut strokes, scanlines, dash_budget: DashBudget::new(1 << 12), accuracy: CurveAccuracy::new(0.25).unwrap(), samples: SampleGrid::Four };
                ShapeAsset::decode(entry, &memory).map_err(|error| format!("{error:?}")).and_then(|asset| {
                    Surface::new(SurfaceId::Osd, &mut pixels).unwrap().draw_shape(origin, asset, Alpha::OPAQUE, ShapePaint::Original, &mut workspace).map_err(|error| format!("{error:?}"))
                })
            }
        };
        if let Err(error) = &drawn { report.fail(format!("bitmap 0x{value:08x} draw: {error}")); }
        let (mut inside, mut outside) = (0usize, 0usize);
        for (index, pixel) in pixels.chunks_exact(4).enumerate() {
            if pixel[3] == 0 { continue; }
            let (x, y) = (index % 720, index / 720);
            if x < usize::from(entry.width) && y < usize::from(entry.height) { inside += 1; } else { outside += 1; }
        }
        if inside == 0 { report.fail(format!("bitmap 0x{value:08x}: no coverage inside {}x{}", entry.width, entry.height)); }
        let name = format!("bitmap-{value:08x}.rgba");
        write_rgba(directory, &name, &pixels);
        entries.push(format!("    {{\"resource\": \"0x{value:08x}\", \"kind\": \"{:?}\", \"width\": {}, \"height\": {}, \"data\": \"0x{:08x}\", \"ink_inside\": {inside}, \"ink_outside\": {outside}, \"render\": \"{name}\"}}",
            entry.kind, entry.width, entry.height, entry.data.0));
    }
    report.json.push_str(&entries.join(",\n"));
    report.json.push_str("\n  ],\n");
}

fn palette(assets: &GuestAssets<'_>, image: &[u8], report: &mut Report) {
    let mut equal = 0;
    for index in 0..=255u32 {
        let expected = &image[PALETTE + index as usize * 3..PALETTE + index as usize * 3 + 3];
        match assets.palette_rgb(PaletteIndex(index)) {
            Ok(colour) if [colour.red, colour.green, colour.blue] == expected && colour.alpha == 255 => equal += 1,
            Ok(colour) => report.fail(format!("palette {index}: {colour:?} differs from dump {expected:02x?}")),
            Err(error) => report.fail(format!("palette {index}: {error:?}")),
        }
    }
    write!(report.json, "  \"palette\": {{\"address\": \"0x{PALETTE:08x}\", \"entries\": 256, \"equal_to_dump\": {equal}}},\n").unwrap();
}

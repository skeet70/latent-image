//! Renders the VGLIB stage-0 scene held in a full guest RAM dump with the owned renderer (`render_osd`) and
//! compares it with the OSD buffer the dump's display shadow names.
//! Usage: scene_check <ram-dump> <output-dir>
//! Writes `owned.rgba` and `front-abgr-as-rgba.rgba` (720x480 straight RGBA, the front buffer read as A,B,G,R
//! bytes) and `summary.json` comparing both byte orders, with each draw's alpha coverage in both renders; exits 1
//! when the scene does not decode or render.

#[path = "../src/lib.rs"]
#[allow(dead_code, unused_imports, unused_attributes)]
mod render_commands;

use render_commands::commands::{DrawCommand, SceneRect};
use render_commands::producer::CommandBuffer;
use render_commands::resources::{DataAddress, DataRegion, ResourceMemory};
use render_commands::stock_objects::{ObjectPool, LISTS_BASE, LIST_BYTES, POOL_BASE, POOL_BYTES};
use render_commands::vector_raster::{StrokePrimitive, Vertex};
use render_commands::vglib_scene::{render_osd, Scene};
use render_commands::SurfaceId;
use std::fmt::Write as _;
use std::path::Path;

/// Engine-1 display shadow `0x96b9bc + 0x1a4`, group 1 item 0 at `+0xb0` (target design 1.4).
const SHADOW_OSD: usize = 0x0096_b9bc + 0x1a4 + 0xb0;
const ALIAS: u32 = 0x4000_0000;
const WIDTH: usize = 720;
const HEIGHT: usize = 480;

#[derive(Clone, Copy)]
enum FrontOrder { Rgba, Abgr }

#[derive(Default)]
struct Difference { pixels: usize, equal: usize, max_delta: u8, total_delta: u64 }
impl Difference {
    fn add(&mut self, owned: [u8; 4], front: [u8; 4]) {
        self.pixels += 1;
        if owned == front { self.equal += 1; }
        for (a, b) in owned.iter().zip(front) {
            let delta = a.abs_diff(b);
            self.max_delta = self.max_delta.max(delta);
            self.total_delta += u64::from(delta);
        }
    }
    fn json(&self) -> String {
        let mean = if self.pixels == 0 { 0.0 } else { self.total_delta as f64 / (self.pixels * 4) as f64 };
        format!("{{\"pixels\": {}, \"equal\": {}, \"max_channel_delta\": {}, \"mean_channel_delta\": {mean:.4}}}", self.pixels, self.equal, self.max_delta)
    }
}

fn word(image: &[u8], address: usize) -> u32 { u32::from_le_bytes(image[address..address + 4].try_into().unwrap()) }

fn front_pixel(front: &[u8], order: FrontOrder, x: usize, y: usize) -> [u8; 4] {
    let start = (y * WIDTH + x) * 4;
    let [a, b, c, d]: [u8; 4] = front[start..start + 4].try_into().unwrap();
    match order { FrontOrder::Rgba => [a, b, c, d], FrontOrder::Abgr => [d, c, b, a] }
}

/// Panel pixels `[left, top, right, bottom)` that `rect` touches.
fn span(rect: SceneRect) -> (usize, usize, usize, usize) {
    let edge = |value: f64, limit: usize| value.clamp(0.0, limit as f64) as usize;
    let (x, y) = (f64::from(rect.x()), f64::from(rect.y()));
    (
        edge(x.floor(), WIDTH),
        edge(y.floor(), HEIGHT),
        edge((x + f64::from(rect.width())).ceil(), WIDTH),
        edge((y + f64::from(rect.height())).ceil(), HEIGHT),
    )
}

fn compare(owned: &[u8], front: &[u8], order: FrontOrder, rect: Option<SceneRect>) -> Difference {
    let (left, top, right, bottom) = rect.map_or((0, 0, WIDTH, HEIGHT), span);
    let mut difference = Difference::default();
    for y in top..bottom {
        for x in left..right {
            let start = (y * WIDTH + x) * 4;
            difference.add(owned[start..start + 4].try_into().unwrap(), front_pixel(front, order, x, y));
        }
    }
    difference
}

/// Pixels with nonzero alpha inside `rect` and their bounding box `[left, top, right, bottom)`, as JSON.
fn coverage(pixel: impl Fn(usize, usize) -> [u8; 4], rect: SceneRect) -> String {
    let (left, top, right, bottom) = span(rect);
    let mut count = 0usize;
    let mut bounds: Option<[usize; 4]> = None;
    for y in top..bottom {
        for x in left..right {
            if pixel(x, y)[3] == 0 { continue; }
            count += 1;
            bounds = Some(match bounds {
                None => [x, y, x + 1, y + 1],
                Some([l, t, r, b]) => [l.min(x), t.min(y), r.max(x + 1), b.max(y + 1)],
            });
        }
    }
    match bounds {
        None => format!("{{\"pixels\": {count}, \"bbox\": null}}"),
        Some([l, t, r, b]) => format!("{{\"pixels\": {count}, \"bbox\": [{l}, {t}, {r}, {b}]}}"),
    }
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
    let regions = [DataRegion { base: DataAddress(0), bytes: &image }, DataRegion { base: DataAddress(ALIAS), bytes: &image }];
    let memory = ResourceMemory::new(&regions).expect("dump regions");
    let pool_start = POOL_BASE.0 as usize;
    let lists_start = LISTS_BASE.0 as usize;
    let pool = ObjectPool::parse(&image[pool_start..pool_start + POOL_BYTES], &image[lists_start..lists_start + LIST_BYTES])
        .unwrap_or_else(|error| { eprintln!("FAIL pool: {error:?}"); std::process::exit(1) });
    let scene = Scene::decode(&memory, &pool).unwrap_or_else(|error| { eprintln!("FAIL scene: {error:?}"); std::process::exit(1) });
    let mut storage = vec![DrawCommand::ResetClip; 4096];
    let mut buffer = CommandBuffer::new(&mut storage);
    let report = scene.lower(&mut buffer).unwrap_or_else(|error| { eprintln!("FAIL lower: {error:?}"); std::process::exit(1) });
    let list = buffer.draw_list();

    let mut vertices = vec![Vertex::EMPTY; 512];
    let mut strokes = vec![StrokePrimitive::EMPTY; 256];
    let mut owned = vec![0u8; SurfaceId::Osd.byte_len()];
    render_osd(list.commands(), &mut owned, &mut vertices, &mut strokes)
        .unwrap_or_else(|error| { eprintln!("FAIL render: {error:?}"); std::process::exit(1) });
    for pixel in owned.chunks_exact_mut(4) { pixel.reverse(); }

    let front_address = (word(&image, SHADOW_OSD) & 0x3fff_ffff) as usize;
    let front = &image[front_address..front_address + WIDTH * HEIGHT * 4];
    let orders = [("rgba", FrontOrder::Rgba), ("abgr", FrontOrder::Abgr)];
    let mut json = String::new();
    write!(json, "{{\n  \"dump\": {:?},\n  \"front_byte\": {},\n  \"front_address\": \"0x{front_address:08x}\",\n", arguments[1], image[0x008e_21f8]).unwrap();
    write!(json, "  \"scene\": {{\"objects\": {}, \"visible\": {}, \"drawn\": {}, \"commands\": {}}},\n", report.objects, report.visible, report.drawn, list.commands().len()).unwrap();
    json.push_str("  \"frame\": {");
    for (index, (name, order)) in orders.iter().enumerate() {
        if index > 0 { json.push_str(", "); }
        write!(json, "\"{name}\": {}", compare(&owned, front, *order, None).json()).unwrap();
    }
    json.push_str("},\n  \"draws\": [\n");
    let mut first = true;
    for command in list.commands() {
        let (kind, dest, detail) = match *command {
            DrawCommand::ScaledImage { dest, source } => ("image", dest, format!("\"source\": [{}, {}]", source.width(), source.height())),
            DrawCommand::ScaledShape { dest, gvff, recolour } => ("shape", dest, format!("\"gvff_bytes\": {}, \"recoloured\": {}", gvff.len(), recolour.is_some())),
            _ => continue,
        };
        if !first { json.push_str(",\n"); }
        first = false;
        write!(json, "    {{\"kind\": \"{kind}\", \"dest\": [{}, {}, {}, {}], {detail}", dest.x(), dest.y(), dest.width(), dest.height()).unwrap();
        for (name, order) in orders {
            write!(json, ", \"{name}\": {}", compare(&owned, front, order, Some(dest)).json()).unwrap();
        }
        let owned_pixel = |x: usize, y: usize| -> [u8; 4] { let start = (y * WIDTH + x) * 4; owned[start..start + 4].try_into().unwrap() };
        write!(json, ", \"coverage\": {{\"owned\": {}, \"front\": {}}}", coverage(owned_pixel, dest), coverage(|x, y| front_pixel(front, FrontOrder::Abgr, x, y), dest)).unwrap();
        json.push('}');
    }
    json.push_str("\n  ]\n}\n");
    let mut front_rgba = vec![0u8; WIDTH * HEIGHT * 4];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let start = (y * WIDTH + x) * 4;
            front_rgba[start..start + 4].copy_from_slice(&front_pixel(front, FrontOrder::Abgr, x, y));
        }
    }
    std::fs::write(directory.join("owned.rgba"), &owned).unwrap();
    std::fs::write(directory.join("front-abgr-as-rgba.rgba"), &front_rgba).unwrap();
    std::fs::write(directory.join("summary.json"), json).unwrap();
}

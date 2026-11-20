extern crate std;
use super::*;
use super::super::commands::Rgba;
use super::super::resources::DataRegion;
use super::super::stock_objects::{LIST_BYTES, POOL_BYTES};
use std::vec;
use std::vec::Vec;

// Guest RAM: `LOW` covers the image ops copy 0x8e21fc, frame mode 0x8e2118, the node pool 0x8e22a4..0x8fa2a4,
// sentinels 0x8fa2a4 (12 bytes per group), count bytes 0x8fa3c4, the class tables 0x8fa3dc/0x8fa484/0x8fa530 and
// the display words 0x8fa5fc/0x8fa7e2. `HIGH` holds objects, VGImage records, pixels and GVFF blocks.
const LOW: u32 = 0x008e_2100;
const LOW_BYTES: usize = 0x18700;
const HIGH: u32 = 0x0100_0000;
const HIGH_BYTES: usize = 0x10000;
const GVFF_AT: u32 = HIGH + 0x8000;
// OpenVG context 0x88dd2c (I4:886680-886688) and its manager/registry/table-header/pool chain (I4:83a6c4-83a6cc,
// 834878-834880, 82b428-82b4b4). Image records sit at pool element base HIGH+0x200 with stride 0x400, so the
// record of image `index` is `block(index) + 0x200`.
const VG: u32 = 0x0088_dd00;
const MANAGER: u32 = HIGH + 0xf000;
const REGISTRY: u32 = HIGH + 0xf020;
const HEADER: u32 = HIGH + 0xf040;
const HANDLE_POOL: u32 = HIGH + 0xf060;
const STATES: u32 = HIGH + 0xf100;
const CHUNK: u32 = HIGH + 0xf200;
const CHUNK_STATES: u32 = HIGH + 0xf240;
fn image_handle(index: u32) -> u32 { 0x2000_0000 | 1 << 25 | index }
// First pixel as an OpenVG VG_sRGBA_8888 word 0x10203080 stored little-endian.
const FIRST_PIXEL: [u8; 4] = [0x80, 0x30, 0x20, 0x10];

fn node(slot: usize) -> u32 { 0x008e_22a4 + slot as u32 * 16 }
fn sentinel(group: u8) -> u32 { 0x008f_a2a4 + u32::from(group) * 12 }
fn block(index: u32) -> u32 { HIGH + index * 0x400 }
fn pool_address(bank: u8, slot: u8) -> u32 { 0x015e_7538 + u32::from(bank) * 0x6a00 + u32::from(slot) * 0xd4 }

#[derive(Clone, Copy)]
struct Placement { x: f32, y: f32, created: (u16, u16), scale: (f32, f32) }
fn place(x: f32, y: f32, created: (u16, u16)) -> Placement { Placement { x, y, created, scale: (1.0, 1.0) } }

struct Ram { vg: Vec<u8>, low: Vec<u8>, high: Vec<u8>, pool: Vec<u8>, lists: Vec<u8> }
impl Ram {
    fn new() -> Self {
        let mut ram = Self { vg: vec![0; 0x100], low: vec![0; LOW_BYTES], high: vec![0; HIGH_BYTES], pool: vec![0; POOL_BYTES], lists: vec![0; LIST_BYTES] };
        // Class tables keep the base getters of 0x7cfc94 and patch +0xa0 with the class draw (6d8d30-6d8d38, 6da644-6da64c).
        ram.ops(IMAGE_OPS, 0x006d_87dc);
        ram.ops(SHAPE_OPS, 0x006d_a0ac);
        for group in 0..24 { ram.group(group, &[]); }
        ram.put32(0x0088_dd2c + 0x1c, MANAGER);
        ram.put32(MANAGER + 0x1c, REGISTRY);
        ram.put32(REGISTRY + 0xc, HEADER);
        ram.put32(HEADER + 4, HANDLE_POOL);
        ram.put32(HANDLE_POOL, 2);
        ram.put32(HANDLE_POOL + 8, HIGH + 0x200);
        ram.put32(HANDLE_POOL + 0xc, 16);
        ram.put32(HANDLE_POOL + 0x20, 0x400);
        ram.put32(HANDLE_POOL + 0x30, STATES);
        ram
    }
    fn bytes(&mut self, address: u32, count: usize) -> &mut [u8] {
        if address >= HIGH { &mut self.high[(address - HIGH) as usize..][..count] }
        else if address >= LOW { &mut self.low[(address - LOW) as usize..][..count] }
        else { &mut self.vg[(address - VG) as usize..][..count] }
    }
    fn put8(&mut self, address: u32, value: u8) { self.bytes(address, 1)[0] = value; }
    fn put16(&mut self, address: u32, value: u16) { self.bytes(address, 2).copy_from_slice(&value.to_le_bytes()); }
    fn put32(&mut self, address: u32, value: u32) { self.bytes(address, 4).copy_from_slice(&value.to_le_bytes()); }
    fn putf(&mut self, address: u32, value: f32) { self.put32(address, value.to_bits()); }
    fn ops(&mut self, table: u32, draw: u32) {
        for (offset, getter) in BASE_GETTERS { self.put32(table + offset as u32, getter); }
        self.put32(table + 0xa0, draw);
    }
    /// Circular list from the sentinel: node {next +0, prev +4, object +8, in-use +0xc} (6d8f94-6d8fa8, 6d8f6c-6d8f70).
    fn group(&mut self, group: u8, entries: &[(usize, u32)]) {
        let head = sentinel(group);
        let at = |index: usize| entries.get(index).map_or(head, |entry| node(entry.0));
        self.put32(head, at(0));
        self.put32(head + 4, if entries.is_empty() { head } else { at(entries.len() - 1) });
        for (index, (slot, object)) in entries.iter().enumerate() {
            let address = node(*slot);
            self.put32(address, at(index + 1));
            self.put32(address + 4, if index == 0 { head } else { at(index - 1) });
            self.put32(address + 8, *object);
            self.put32(address + 0xc, 1);
        }
        self.put8(0x008f_a3c4 + u32::from(group), entries.len() as u8);
    }
    /// Base fields: x/y +0/+4, created size +8/+0xc, scale +0x14/+0x18, visible +0x47, ops +0xc0 (6d81e0-6d8264, 6d8140).
    fn base(&mut self, object: u32, ops: u32, placement: Placement) {
        self.put32(object + 0xc0, ops);
        self.put8(object + 0x47, 1);
        self.putf(object, placement.x);
        self.putf(object + 4, placement.y);
        self.putf(object + 8, f32::from(placement.created.0));
        self.putf(object + 0xc, f32::from(placement.created.1));
        self.putf(object + 0x14, placement.scale.0);
        self.putf(object + 0x18, placement.scale.1);
    }
    /// Image object with handle +0xc4 naming the VGImage record `block(index) + 0x200`, whose slot state is live
    /// (0x2200: type 2 in bits 15..12, handle bits 27..16 in 11..0); the record holds format +0x14, width/height
    /// +0x3c/+0x40, stride +0x4c and CPU pixels +0x68 (835ee0-836034).
    fn image(&mut self, index: u32, placement: Placement) -> u32 {
        let object = block(index);
        let record = object + 0x200;
        let pixels = object + 0x280;
        let (width, height) = placement.created;
        self.base(object, IMAGE_OPS, placement);
        self.put32(object + 0xc4, image_handle(index));
        self.put16(STATES + 2 * index, 0x2200);
        self.put32(record + 0x14, 1);
        self.put16(record + 0x3c, width);
        self.put16(record + 0x40, height);
        self.put16(record + 0x4c, width * 4);
        self.put32(record + 0x68, pixels);
        self.bytes(pixels, 4).copy_from_slice(&FIRST_PIXEL);
        object
    }
    /// Clip words +0x34..+0x40 and enable byte +0x46 (6d8350, 6d8320).
    fn clip(&mut self, object: u32, clip: [i32; 4]) {
        self.put8(object + 0x46, 1);
        for (index, value) in clip.iter().enumerate() { self.put32(object + 0x34 + 4 * index as u32, *value as u32); }
    }
    /// Shape object: type getter +0xc8 (6da668-6da670), paint +0xd4, type +0xe8, bounds x0 +0x30, y0 +0x28,
    /// x1 +0x2c, y1 +0x24 (6da700-6da718), 96-byte block +0xec.
    fn shape(&mut self, index: u32, placement: Placement, bounds: [f32; 4], shape_type: u32) -> u32 {
        let object = block(index);
        self.base(object, SHAPE_OPS, placement);
        self.put32(object + 0xc8, 0x006d_9fac);
        self.put32(object + 0xd4, 1);
        self.put32(object + 0xe8, shape_type);
        self.putf(object + 0x30, bounds[0]);
        self.putf(object + 0x28, bounds[1]);
        self.putf(object + 0x2c, bounds[2]);
        self.putf(object + 0x24, bounds[3]);
        for component in 0..24u32 { self.putf(object + 0xec + component * 4, component as f32 / 24.0); }
        object
    }
    /// Header: magic "GVFF", zero item/paint/ramp/dash counts at +0x14..+0x20, bounds +0x28..+0x34 (6da6b8-6da718).
    fn gvff(&mut self, address: u32, bounds: [f32; 4]) {
        self.put32(address, 0x4646_5647);
        for (index, value) in bounds.iter().enumerate() { self.putf(address + 40 + 4 * index as u32, *value); }
        self.put32(address + 56, 0xdead_beef);
    }
    /// Pool object: handle +4, content +0xc, kind +0x14, bank byte +0x5a; sole member of its bank list.
    fn own(&mut self, bank: u8, slot: u8, shape: u32, content: u32, kind: u32) {
        let start = usize::from(bank) * 0x6a00 + usize::from(slot) * 0xd4;
        let object = &mut self.pool[start..start + 0xd4];
        object[4..8].copy_from_slice(&shape.to_le_bytes());
        object[0xc..0x10].copy_from_slice(&content.to_le_bytes());
        object[0x14..0x18].copy_from_slice(&kind.to_le_bytes());
        object[0x5a] = bank;
        let address = pool_address(bank, slot);
        let list = usize::from(bank) * 8;
        self.lists[list..list + 4].copy_from_slice(&address.to_le_bytes());
        self.lists[list + 4..list + 8].copy_from_slice(&address.to_le_bytes());
    }
    fn regions(&self) -> [DataRegion<'_>; 3] {
        [DataRegion { base: DataAddress(VG), bytes: &self.vg }, DataRegion { base: DataAddress(LOW), bytes: &self.low }, DataRegion { base: DataAddress(HIGH), bytes: &self.high }]
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Out {
    Clip(PanelRect),
    ResetClip,
    Image { dest: SceneRect, size: (u16, u16), first: Rgba },
    Shape { dest: SceneRect, gvff: Vec<u8>, recolour: Option<Vec<u8>> },
}

fn render(ram: &Ram) -> Result<(SceneReport, Vec<Out>), SceneError> {
    let regions = ram.regions();
    let memory = ResourceMemory::new(&regions).unwrap();
    let pool = ObjectPool::parse(&ram.pool, &ram.lists).unwrap();
    let scene = Scene::decode(&memory, &pool)?;
    let checked = scene.check();
    let mut storage = [DrawCommand::ResetClip; 64];
    let mut buffer = CommandBuffer::new(&mut storage);
    let report = scene.lower(&mut buffer)?;
    assert_eq!(checked, Ok(report));
    let out = buffer.draw_list().commands().iter().map(|command| match *command {
        DrawCommand::Clip(rect) => Out::Clip(rect),
        DrawCommand::ResetClip => Out::ResetClip,
        DrawCommand::ScaledImage { dest, source } => Out::Image { dest, size: (source.width(), source.height()), first: source.rgba(0, 0).unwrap() },
        DrawCommand::ScaledShape { dest, gvff, recolour } => Out::Shape { dest, gvff: gvff.to_vec(), recolour: recolour.map(|block| block.0.to_vec()) },
        other => panic!("unexpected {other:?}"),
    }).collect();
    Ok((report, out))
}

fn refusal(ram: &Ram) -> SceneError { render(ram).unwrap_err() }
fn object_refusal(object: u32, refusal: ObjectRefusal) -> SceneError { SceneError::Object { object: VglibObject(DataAddress(object)), refusal } }
fn rect(x: i32, y: i32, width: u16, height: u16) -> SceneRect { SceneRect::new(x, y, width, height).unwrap() }
fn panel(x: u16, y: u16, width: u16, height: u16) -> PanelRect { PanelRect::new(PanelPoint::new(x, y).unwrap(), width, height).unwrap() }
fn group(index: u8) -> GroupIndex { GroupIndex::new(index).unwrap() }
const FIRST: Rgba = Rgba { red: 0x10, green: 0x20, blue: 0x30, alpha: 0x80 };

fn image_at(dest: SceneRect, size: (u16, u16)) -> Out { Out::Image { dest, size, first: FIRST } }

fn single_image(placement: Placement) -> (Ram, u32) {
    let mut ram = Ram::new();
    let object = ram.image(0, placement);
    ram.group(0, &[(3, object)]);
    (ram, object)
}

const SHAPE_BOUNDS: [f32; 4] = [0.0, 12.0, 20.0, 0.0];
fn single_shape(shape_type: u32) -> (Ram, u32) {
    let mut ram = Ram::new();
    let object = ram.shape(0, place(50.0, 60.0, (20, 12)), SHAPE_BOUNDS, shape_type);
    ram.gvff(GVFF_AT, SHAPE_BOUNDS);
    ram.own(3, 2, object, GVFF_AT, 5);
    ram.group(0, &[(3, object)]);
    (ram, object)
}

#[test]
fn groups_render_from_twenty_three_down_and_each_list_from_its_head() {
    let mut ram = Ram::new();
    let objects: Vec<u32> = [10.0, 20.0, 30.0, 40.0].iter().enumerate()
        .map(|(index, x)| ram.image(index as u32, place(*x, 0.0, (8, 4)))).collect();
    ram.group(23, &[(5, objects[0]), (2, objects[1])]);
    ram.group(7, &[(9, objects[2])]);
    ram.group(0, &[(1, objects[3])]);
    let (report, out) = render(&ram).unwrap();
    assert_eq!(report, SceneReport { objects: 4, visible: 4, drawn: 4, clipped: 0 });
    let expected: Vec<Out> = [10, 20, 30, 40].iter()
        .flat_map(|x| [Out::ResetClip, image_at(rect(*x, 0, 8, 4), (8, 4))]).collect();
    assert_eq!(out, expected);
}

#[test]
fn walk_reports_groups_and_objects_in_render_order() {
    let mut ram = Ram::new();
    let a = ram.image(0, place(0.0, 0.0, (8, 4)));
    let b = ram.image(1, place(0.0, 0.0, (8, 4)));
    ram.group(2, &[(40, a)]);
    ram.group(11, &[(7, b)]);
    let regions = ram.regions();
    let memory = ResourceMemory::new(&regions).unwrap();
    let pool = ObjectPool::parse(&ram.pool, &ram.lists).unwrap();
    let scene = Scene::decode(&memory, &pool).unwrap();
    let walk: Vec<_> = scene.objects().map(Result::unwrap).collect();
    assert_eq!(walk, vec![(group(11), VglibObject(DataAddress(b))), (group(2), VglibObject(DataAddress(a)))]);
    assert_eq!(scene.group_length(group(11)), 1);
    assert_eq!(scene.group_length(group(5)), 0);
}

#[test]
fn image_destination_and_clip_at_identity() {
    let (mut ram, object) = single_image(place(100.0, 40.0, (8, 4)));
    ram.clip(object, [96, 32, 200, 40]);
    let (_, out) = render(&ram).unwrap();
    assert_eq!(out, vec![Out::Clip(panel(96, 32, 200, 40)), image_at(rect(100, 40, 8, 4), (8, 4))]);
}

#[test]
fn image_destination_at_nine_eighths_horizontal_ratio() {
    // A pool object of 8 pixels placed by 0x1177a08's x*9/8 transform: target 9 wide, scale word 9/8.
    let (ram, _) = single_image(Placement { x: 225.0, y: 40.0, created: (8, 4), scale: (1.125, 1.0) });
    let (_, out) = render(&ram).unwrap();
    assert_eq!(out, vec![Out::ResetClip, image_at(rect(225, 40, 9, 4), (8, 4))]);
}

#[test]
fn shape_scissor_saturates_its_bottom_edge_and_image_scissor_does_not() {
    // Clip y 470, height 20: y-up bottom 480 - 20 - 470 = -10, which vcvt.u32 (6da47c) makes 0 and vcvt.s32 (6d8c94) keeps.
    let (mut ram, object) = single_image(place(0.0, 0.0, (8, 4)));
    ram.clip(object, [0, 470, 720, 20]);
    assert_eq!(render(&ram).unwrap().1[0], Out::Clip(panel(0, 470, 720, 10)));
    let (mut ram, object) = single_shape(1);
    ram.clip(object, [0, 470, 720, 20]);
    assert_eq!(render(&ram).unwrap().1[0], Out::Clip(panel(0, 460, 720, 20)));
}

#[test]
fn clip_outside_the_panel_draws_nothing_but_counts_as_visible() {
    let (mut ram, object) = single_image(place(0.0, 0.0, (8, 4)));
    ram.clip(object, [800, 0, 10, 10]);
    assert_eq!(render(&ram).unwrap(), (SceneReport { objects: 1, visible: 1, drawn: 0, clipped: 1 }, vec![]));
}

#[test]
fn hidden_and_resourceless_objects_draw_nothing() {
    let (mut ram, object) = single_image(place(0.0, 0.0, (8, 4)));
    ram.put8(object + 0x47, 0);
    assert_eq!(render(&ram).unwrap(), (SceneReport { objects: 1, visible: 0, drawn: 0, clipped: 0 }, vec![]));
    let (mut ram, object) = single_image(place(0.0, 0.0, (8, 4)));
    ram.put32(object + 0xc4, 0);
    assert_eq!(render(&ram).unwrap(), (SceneReport { objects: 1, visible: 1, drawn: 0, clipped: 0 }, vec![]));
}

#[test]
fn shape_resolves_its_gvff_through_the_pool_owner() {
    let (ram, _) = single_shape(2);
    let (_, out) = render(&ram).unwrap();
    let mut header = vec![0u8; 56];
    header[..4].copy_from_slice(b"GVFF");
    for (index, value) in SHAPE_BOUNDS.iter().enumerate() { header[40 + 4 * index..44 + 4 * index].copy_from_slice(&value.to_le_bytes()); }
    let block: Vec<u8> = (0..24u32).flat_map(|component| (component as f32 / 24.0).to_le_bytes()).collect();
    assert_eq!(out, vec![Out::ResetClip, Out::Shape { dest: rect(50, 60, 20, 12), gvff: header.clone(), recolour: Some(block) }]);
    let (ram, _) = single_shape(1);
    assert_eq!(render(&ram).unwrap().1[1], Out::Shape { dest: rect(50, 60, 20, 12), gvff: header, recolour: None });
}

#[test]
fn shape_owner_problems_are_refused() {
    let mut ram = Ram::new();
    let object = ram.shape(0, place(50.0, 60.0, (20, 12)), SHAPE_BOUNDS, 1);
    ram.gvff(GVFF_AT, SHAPE_BOUNDS);
    ram.group(0, &[(3, object)]);
    assert_eq!(refusal(&ram), object_refusal(object, ObjectRefusal::ShapeOwnerMissing));
    ram.own(3, 2, object, GVFF_AT, 2);
    assert_eq!(refusal(&ram), object_refusal(object, ObjectRefusal::ShapeOwnerKind(ObjectKind::Bitmap)));
    ram.own(3, 2, object, GVFF_AT + 4, 5);
    assert_eq!(refusal(&ram), object_refusal(object, ObjectRefusal::Shape(ShapeError::Magic)));
    let (mut ram, object) = single_shape(1);
    ram.putf(object + 0x30, 1.0);
    assert_eq!(refusal(&ram), object_refusal(object, ObjectRefusal::ShapeBounds));
    let reversed = [20.0, 12.0, 0.0, 0.0];
    let mut ram = Ram::new();
    let object = ram.shape(0, place(50.0, 60.0, (20, 12)), reversed, 1);
    ram.gvff(GVFF_AT, reversed);
    ram.own(3, 2, object, GVFF_AT, 5);
    ram.group(0, &[(3, object)]);
    assert_eq!(refusal(&ram), object_refusal(object, ObjectRefusal::ShapeOrientation));
}

#[test]
fn classes_follow_the_ops_word() {
    let mut ram = Ram::new();
    let image = ram.image(0, place(0.0, 0.0, (8, 4)));
    let shape = ram.shape(1, place(0.0, 0.0, (20, 12)), SHAPE_BOUNDS, 1);
    let path = block(2);
    ram.put32(path + 0xc0, PATH_LIST_OPS);
    let plane = block(3);
    ram.put32(plane + 0xc0, PLANE_EVENT_OPS);
    let unknown = block(4);
    ram.put32(unknown + 0xc0, 0x1234_5678);
    let regions = ram.regions();
    let memory = ResourceMemory::new(&regions).unwrap();
    let decode = |address: u32| SceneObject::decode(&memory, group(0), VglibObject(DataAddress(address))).map(|object| object.class);
    assert_eq!(decode(image), Ok(ObjectClass::Image));
    assert_eq!(decode(shape), Ok(ObjectClass::Shape));
    assert_eq!(decode(path), Err(ObjectRefusal::PathList));
    assert_eq!(decode(plane), Err(ObjectRefusal::PlaneEvent));
    assert_eq!(decode(unknown), Err(ObjectRefusal::UnknownClass(0x1234_5678)));
}

#[test]
fn a_changed_class_table_is_refused() {
    let (mut ram, object) = single_image(place(0.0, 0.0, (8, 4)));
    ram.put32(IMAGE_OPS + 0x04, 0x006d_8160);
    assert_eq!(refusal(&ram), object_refusal(object, ObjectRefusal::OpsTable));
}

#[test]
fn base_fields_decode_from_their_getter_offsets() {
    let (mut ram, object) = single_image(Placement { x: 7.0, y: 9.0, created: (8, 4), scale: (1.5, 2.0) });
    ram.clip(object, [1, 2, 3, 4]);
    ram.put8(object + 0x45, 2);
    ram.put8(object + 0x48, 2);
    ram.put8(object + 0x49, 17);
    ram.put8(object + 0x4c, 1);
    let regions = ram.regions();
    let memory = ResourceMemory::new(&regions).unwrap();
    let decoded = SceneObject::decode(&memory, group(0), VglibObject(DataAddress(object))).unwrap();
    assert_eq!(decoded.base, BaseFields {
        visible: true, position: ScenePoint { x: 7.0, y: 9.0 }, size: SceneSize { width: 8.0, height: 4.0 },
        scale: ScaleWords { x: 1.5, y: 2.0 }, parameter: 0.0, clip: Some(ObjectClip { x: 1, y: 2, width: 3, height: 4 }),
        coordinate_mode: 2, group: 17, matrix: false, mirror: false, output_mode: 1, blend: BlendMode::Multiply,
    });
}

#[test]
fn coordinate_mode_and_multiply_blend_do_not_move_an_unrotated_image() {
    let (mut ram, object) = single_image(place(5.0, 6.0, (8, 4)));
    ram.put8(object + 0x48, 2);
    ram.put8(object + 0x45, 2);
    assert_eq!(render(&ram).unwrap().1, vec![Out::ResetClip, image_at(rect(5, 6, 8, 4), (8, 4))]);
}

#[test]
fn unestablished_draw_state_is_refused() {
    let cases: [(fn(&mut Ram, u32), ObjectRefusal); 6] = [
        (|ram, object| ram.putf(object + 0x1c, 90.0), ObjectRefusal::Rotation),
        (|ram, object| { ram.put8(DISPLAY_MIRROR.0, 1); ram.put8(object + 0x4b, 1); }, ObjectRefusal::Mirror),
        (|ram, object| ram.put8(object + 0x4a, 1), ObjectRefusal::Matrix),
        (|ram, object| ram.put8(object + 0x45, 1), ObjectRefusal::ColourTransform),
        (|ram, _| ram.put32(DISPLAY_MODE.0, 3), ObjectRefusal::SurfaceScale { display_mode: 3, output_mode: 0 }),
        (|ram, object| { ram.put32(DISPLAY_MODE.0, 8); ram.put8(object + 0x4c, 1); }, ObjectRefusal::SurfaceScale { display_mode: 8, output_mode: 1 }),
    ];
    for (mutate, expected) in cases {
        let (mut ram, object) = single_image(place(0.0, 0.0, (8, 4)));
        mutate(&mut ram, object);
        assert_eq!(refusal(&ram), object_refusal(object, expected));
    }
}

#[test]
fn display_states_matching_the_panel_still_draw() {
    // Object mirror without display mirror, and display mode 8 with output mode 0 (720 x 480 at 6dd8c4-6dd8ec).
    let (mut ram, object) = single_image(place(0.0, 0.0, (8, 4)));
    ram.put8(object + 0x4b, 1);
    ram.put32(DISPLAY_MODE.0, 8);
    assert_eq!(render(&ram).unwrap().0.drawn, 1);
}

#[test]
fn image_storage_and_geometry_refusals() {
    let (mut ram, object) = single_image(place(0.0, 0.0, (8, 4)));
    ram.put32(object + 0x200 + 0x14, 2);
    assert_eq!(refusal(&ram), object_refusal(object, ObjectRefusal::ImageFormat));
    let (mut ram, object) = single_image(place(0.0, 0.0, (8, 4)));
    ram.put16(object + 0x200 + 0x3c, 9);
    ram.put16(object + 0x200 + 0x4c, 36);
    assert_eq!(refusal(&ram), object_refusal(object, ObjectRefusal::ImageSize));
    // 8 x 1.1 = 8.8 pixels.
    let (ram, object) = single_image(Placement { x: 0.0, y: 0.0, created: (8, 4), scale: (1.1, 1.0) });
    assert_eq!(refusal(&ram), object_refusal(object, ObjectRefusal::NonIntegralGeometry));
}

#[test]
fn image_handles_resolve_only_to_live_slots_of_the_tagged_pool() {
    let image = image_at(rect(0, 0, 8, 4), (8, 4));
    let (ram, _) = single_image(place(0.0, 0.0, (8, 4)));
    assert_eq!(render(&ram).unwrap().1, vec![Out::ResetClip, image.clone()]);
    // A freed slot clears the state's type nibble (82b2ec-82b2f8).
    let (mut ram, object) = single_image(place(0.0, 0.0, (8, 4)));
    ram.put16(STATES, 0x0200);
    assert_eq!(refusal(&ram), object_refusal(object, ObjectRefusal::ImageHandle(image_handle(0))));
    // The table header's tag must equal handle bits 24..16 (82b434-82b440).
    let (mut ram, object) = single_image(place(0.0, 0.0, (8, 4)));
    ram.put32(HEADER, 1);
    assert_eq!(refusal(&ram), object_refusal(object, ObjectRefusal::ImageHandle(image_handle(0))));
    // The pool's type word must equal handle bits 31..28 (82b448-82b450).
    let (mut ram, object) = single_image(place(0.0, 0.0, (8, 4)));
    ram.put32(HANDLE_POOL, 3);
    assert_eq!(refusal(&ram), object_refusal(object, ObjectRefusal::ImageHandle(image_handle(0))));
    // A cached handle returns the cached record without consulting slot state (82b458-82b464).
    let (mut ram, _) = single_image(place(0.0, 0.0, (8, 4)));
    ram.put16(STATES, 0);
    ram.put32(HANDLE_POOL + 0x34, image_handle(0));
    ram.put32(HANDLE_POOL + 0x38, block(0) + 0x200);
    assert_eq!(render(&ram).unwrap().1, vec![Out::ResetClip, image.clone()]);
    // A slot past the first block is found in a chunk {base, first, count, state array +0x10, next +0x14}
    // (82b4bc-82b538): slot 20 is chunk index 2, base + 2 x stride.
    let mut ram = Ram::new();
    let object = ram.image(1, place(0.0, 0.0, (8, 4)));
    ram.put32(object + 0xc4, image_handle(20));
    ram.put32(HANDLE_POOL + 0x40, CHUNK);
    ram.put32(CHUNK, block(1) + 0x200 - 2 * 0x400);
    ram.put32(CHUNK + 4, 18);
    ram.put32(CHUNK + 8, 4);
    ram.put32(CHUNK + 0x10, CHUNK_STATES);
    ram.put16(CHUNK_STATES + 4, 0x2200);
    ram.group(0, &[(3, object)]);
    assert_eq!(render(&ram).unwrap().1, vec![Out::ResetClip, image]);
}

#[test]
fn dual_render_frame_modes_are_refused() {
    let (mut ram, _) = single_image(place(0.0, 0.0, (8, 4)));
    ram.put32(FRAME_MODE.0, 8);
    assert_eq!(refusal(&ram), SceneError::DualRender(8));
}

#[test]
fn malformed_lists_are_refused() {
    let (mut ram, object) = single_image(place(0.0, 0.0, (8, 4)));
    // Group 4: node 1 -> node 2 -> node 1.
    ram.group(4, &[(1, object), (2, object)]);
    ram.put32(node(2), node(1));
    assert_eq!(refusal(&ram), SceneError::RepeatedNode { group: group(4), node: DataAddress(node(1)) });

    let (mut ram, _) = single_image(place(0.0, 0.0, (8, 4)));
    ram.put32(sentinel(6), 0x008f_a300);
    assert_eq!(refusal(&ram), SceneError::NodeOutsidePool { group: group(6), node: DataAddress(0x008f_a300) });
    ram.put32(sentinel(6), node(4) + 4);
    assert_eq!(refusal(&ram), SceneError::NodeOutsidePool { group: group(6), node: DataAddress(node(4) + 4) });

    let (mut ram, _) = single_image(place(0.0, 0.0, (8, 4)));
    ram.put8(0x008f_a3c4, 2);
    assert_eq!(refusal(&ram), SceneError::CountMismatch { group: group(0), count: 2, listed: 1 });

    let (mut ram, _) = single_image(place(0.0, 0.0, (8, 4)));
    ram.put32(node(3) + 8, 0);
    assert_eq!(refusal(&ram), SceneError::NullObject { group: group(0), node: DataAddress(node(3)) });

    let (mut ram, _) = single_image(place(0.0, 0.0, (8, 4)));
    ram.put32(node(3) + 0xc, 0);
    assert_eq!(refusal(&ram), SceneError::FreeNode { group: group(0), node: DataAddress(node(3)) });
}

#[test]
fn a_full_node_pool_cannot_copy_a_group() {
    let (mut ram, _) = single_image(place(0.0, 0.0, (8, 4)));
    for slot in 0..NODE_COUNT { ram.put32(node(slot) + 0xc, 1); }
    assert_eq!(refusal(&ram), SceneError::NodeCapacity { free: 0, needed: 1 });
}

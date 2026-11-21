extern crate std;
use super::*;
use super::super::resources::DataRegion;
use std::boxed::Box;
use std::cell::RefCell;
use std::vec;
use std::vec::Vec;

// Guest RAM: `LOW` covers the image ops copy 0x8e21fc, frame mode 0x8e2118, the node pool 0x8e22a4..0x8fa2a4,
// sentinels 0x8fa2a4 (12 bytes per group), count bytes 0x8fa3c4, the class tables 0x8fa3dc/0x8fa484/0x8fa530 and
// the display words 0x8fa5fc/0x8fa7e2. `HIGH` holds objects and shape items.
const LOW: u32 = 0x008e_2100;
const LOW_BYTES: usize = 0x18700;
const HIGH: u32 = 0x0100_0000;
const HIGH_BYTES: usize = 0x10000;

fn node(slot: usize) -> u32 { 0x008e_22a4 + slot as u32 * 16 }
fn sentinel(group: u8) -> u32 { 0x008f_a2a4 + u32::from(group) * 12 }
fn block(index: u32) -> u32 { HIGH + index * 0x400 }

#[derive(Clone, Copy)]
struct Place { x: f32, y: f32, size: (f32, f32), scale: (f32, f32) }
fn place(x: f32, y: f32, size: (f32, f32)) -> Place { Place { x, y, size, scale: (1.0, 1.0) } }

struct Ram { low: Vec<u8>, high: Vec<u8> }
impl Ram {
    fn new() -> Self {
        let mut ram = Self { low: vec![0; LOW_BYTES], high: vec![0; HIGH_BYTES] };
        ram.ops(IMAGE_OPS, 0x006d_87dc);
        ram.ops(SHAPE_OPS, 0x006d_a0ac);
        for group in 0..24 { ram.group(group, &[]); }
        ram
    }
    fn bytes(&mut self, address: u32, count: usize) -> &mut [u8] {
        if address >= HIGH { &mut self.high[(address - HIGH) as usize..][..count] } else { &mut self.low[(address - LOW) as usize..][..count] }
    }
    fn put8(&mut self, address: u32, value: u8) { self.bytes(address, 1)[0] = value; }
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
    fn base(&mut self, object: u32, ops: u32, place: Place) {
        self.put32(object + 0xc0, ops);
        self.put8(object + 0x47, 1);
        self.putf(object, place.x);
        self.putf(object + 4, place.y);
        self.putf(object + 8, place.size.0);
        self.putf(object + 0xc, place.size.1);
        self.putf(object + 0x14, place.scale.0);
        self.putf(object + 0x18, place.scale.1);
    }
    fn image(&mut self, index: u32, place: Place) -> u32 {
        let object = block(index);
        self.base(object, IMAGE_OPS, place);
        self.put32(object + 0xc4, 0x2200_0000 | index);
        object
    }
    fn clip(&mut self, object: u32, clip: [i32; 4]) {
        self.put8(object + 0x46, 1);
        for (index, value) in clip.iter().enumerate() { self.put32(object + 0x34 + 4 * index as u32, *value as u32); }
    }
    /// Shape object with paint word +0xd4, bounds y1 +0x24 and x0 +0x30, and `items` at +0xd8/+0xe0.
    fn shape(&mut self, index: u32, place: Place, items: &[[u32; 21]]) -> u32 {
        let object = block(index);
        self.base(object, SHAPE_OPS, place);
        self.put32(object + 0xd4, 1);
        self.putf(object + 0x24, 3.0);
        self.putf(object + 0x30, 2.0);
        let list = object + 0x200;
        self.put32(object + 0xd8, list);
        self.put32(object + 0xe0, items.len() as u32);
        for (index, item) in items.iter().enumerate() {
            for (slot, value) in item.iter().enumerate() { self.put32(list + (index * SHAPE_ITEM_BYTES + 4 * slot) as u32, *value); }
        }
        object
    }
    fn regions(&self) -> [DataRegion<'_>; 2] {
        [DataRegion { base: DataAddress(LOW), bytes: &self.low }, DataRegion { base: DataAddress(HIGH), bytes: &self.high }]
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Event { Call(VgCall), MakeCurrent, GetInteger(VgWord), CopyBuffers, RecordError, Visits(u32, u32), Yield, StockVisit(u32), StockWalk(u8) }

/// Guest RAM that other tasks change while the render yields.
struct Live(RefCell<Ram>);
impl SceneMemory for Live {
    fn read<R>(&self, f: impl FnOnce(&ResourceMemory<'_>) -> R) -> R {
        let ram = self.0.borrow();
        let regions = ram.regions();
        f(&ResourceMemory::new(&regions).unwrap())
    }
}

/// Records the render's calls. `on_yield` changes `live` at the first yield; the stock visitor counts and draws each
/// object it is given.
struct Recorder<'r> {
    events: Vec<Event>,
    current: bool,
    quality: i32,
    copied: bool,
    counters: (u32, u32),
    live: Option<&'r Live>,
    on_yield: Option<Box<dyn FnOnce(&mut Ram) + 'r>>,
}
impl<'r> Recorder<'r> {
    fn new() -> Self {
        Self { events: Vec::new(), current: true, quality: RENDERING_QUALITY_BETTER, copied: true, counters: (0, 0), live: None, on_yield: None }
    }
    fn changing(live: &'r Live, change: impl FnOnce(&mut Ram) + 'r) -> Self {
        Self { live: Some(live), on_yield: Some(Box::new(change)), ..Self::new() }
    }
    fn calls(&self) -> Vec<VgCall> {
        self.events.iter().filter_map(|event| match event { Event::Call(call) => Some(*call), _ => None }).collect()
    }
    fn drawn(&self) -> Vec<u32> {
        self.events.iter().filter_map(|event| match event { Event::Call(VgCall::DrawImage(handle)) => Some(*handle), _ => None }).collect()
    }
}
impl StageDevice for Recorder<'_> {
    fn call(&mut self, call: VgCall) { self.events.push(Event::Call(call)); }
    fn make_current(&mut self) -> bool { self.events.push(Event::MakeCurrent); self.current }
    fn get_integer(&mut self, parameter: VgWord) -> i32 { self.events.push(Event::GetInteger(parameter)); self.quality }
    fn copy_buffers(&mut self) -> bool { self.events.push(Event::CopyBuffers); self.copied }
    fn record_error(&mut self) { self.events.push(Event::RecordError); }
    fn set_visits(&mut self, visited: u32, result: u32) { self.counters = (visited, result); self.events.push(Event::Visits(visited, result)); }
    fn visits(&mut self) -> (u32, u32) { self.counters }
    fn yield_task(&mut self) {
        self.events.push(Event::Yield);
        if let (Some(live), Some(change)) = (self.live, self.on_yield.take()) { change(&mut *live.0.borrow_mut()); }
    }
    fn stock_visit(&mut self, object: VglibObject) {
        self.events.push(Event::StockVisit(object.0.0));
        self.counters = (self.counters.0 + 1, self.counters.1 | 1);
    }
    fn stock_walk(&mut self, group: GroupIndex) { self.events.push(Event::StockWalk(group.index())); }
}

fn check(ram: &Ram) -> Result<(), SceneError> {
    let regions = ram.regions();
    let memory = ResourceMemory::new(&regions).unwrap();
    Scene::decode(&memory)?.check()
}

fn render_floor(ram: &Ram, floor: f32, device: &mut Recorder<'_>) -> RenderOutcome {
    let regions = ram.regions();
    let memory = ResourceMemory::new(&regions).unwrap();
    render_stage(720, 480, floor, &memory, &mut [VglibObject(DataAddress(0)); 16], device)
}
fn render(ram: &Ram, device: &mut Recorder<'_>) -> RenderOutcome { render_floor(ram, 0.0, device) }
fn render_live(live: &Live, device: &mut Recorder<'_>) -> RenderOutcome {
    render_stage(720, 480, 0.0, live, &mut [VglibObject(DataAddress(0)); 16], device)
}

fn object_calls(ram: &Ram) -> Vec<VgCall> {
    check(ram).unwrap();
    let mut device = Recorder::new();
    render(ram, &mut device);
    let calls = device.calls();
    // Quality check, viewport and clear open the frame; Finish closes it.
    calls[2..calls.len() - 1].to_vec()
}

fn refusal(ram: &Ram) -> SceneError { check(ram).unwrap_err() }
fn object_refusal(object: u32, refusal: ObjectRefusal) -> SceneError { SceneError::Object { object: VglibObject(DataAddress(object)), refusal } }
fn group(index: u8) -> GroupIndex { GroupIndex::new(index).unwrap() }
fn seti(parameter: VgWord, value: i32) -> VgCall { VgCall::SetInteger { parameter, value } }
const IDENTITY: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

fn single_image(place: Place) -> (Ram, u32) {
    let mut ram = Ram::new();
    let object = ram.image(0, place);
    ram.group(0, &[(3, object)]);
    (ram, object)
}

fn image_calls(handle: u32, translate: [f32; 2], first: [f32; 9], scale: [f32; 2], height: f32, clip: Option<[i32; 4]>) -> Vec<VgCall> {
    let mut calls = vec![
        seti(MATRIX_MODE, 0x1401), seti(IMAGE_QUALITY, 0x1f00), seti(BLEND_MODE, 0x2001), seti(COLOR_TRANSFORM, 0),
        VgCall::LoadIdentity,
        VgCall::Translate { x: translate[0], y: translate[1] },
        VgCall::MultiplyMatrix(Matrix::Local(first)),
        VgCall::Rotate(0.0),
        VgCall::Scale { x: scale[0], y: scale[1] },
        VgCall::MultiplyMatrix(Matrix::Local([1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, height, 1.0])),
    ];
    if let Some(values) = clip { calls.extend([VgCall::SetIntegers { parameter: SCISSOR_RECTS, values }, seti(SCISSORING, 1)]); }
    calls.extend([seti(IMAGE_MODE, 4), VgCall::DrawImage(handle), seti(SCISSORING, 0), seti(COLOR_TRANSFORM, 0)]);
    calls
}

#[test]
fn groups_draw_from_twenty_three_down_and_each_list_from_its_head() {
    let mut ram = Ram::new();
    let objects: Vec<u32> = (0..4).map(|index| ram.image(index, place(0.0, 0.0, (8.0, 4.0)))).collect();
    ram.group(23, &[(5, objects[0]), (2, objects[1])]);
    ram.group(7, &[(9, objects[2])]);
    ram.group(0, &[(1, objects[3])]);
    let drawn: Vec<u32> = object_calls(&ram).into_iter().filter_map(|call| match call { VgCall::DrawImage(handle) => Some(handle), _ => None }).collect();
    assert_eq!(drawn, vec![0x2200_0000, 0x2200_0001, 0x2200_0002, 0x2200_0003]);
}

#[test]
fn walk_reports_groups_and_objects_in_render_order() {
    let mut ram = Ram::new();
    let a = ram.image(0, place(0.0, 0.0, (8.0, 4.0)));
    let b = ram.image(1, place(0.0, 0.0, (8.0, 4.0)));
    ram.group(2, &[(40, a)]);
    ram.group(11, &[(7, b)]);
    let regions = ram.regions();
    let memory = ResourceMemory::new(&regions).unwrap();
    let scene = Scene::decode(&memory).unwrap();
    let walk: Vec<_> = scene.objects().map(Result::unwrap).collect();
    assert_eq!(walk, vec![(group(11), VglibObject(DataAddress(b))), (group(2), VglibObject(DataAddress(a)))]);
    assert_eq!(scene.group_length(group(11)), 1);
    assert_eq!(scene.group_length(group(5)), 0);
}

#[test]
fn an_image_draws_at_its_y_up_position_with_its_scissor() {
    let (mut ram, object) = single_image(Place { x: 100.0, y: 40.0, size: (8.0, 4.0), scale: (1.5, 2.0) });
    ram.clip(object, [96, 32, 200, 40]);
    // Translate (100, 480 - 8 - 40); scissor bottom 480 - 40 - 32.
    assert_eq!(object_calls(&ram), image_calls(0x2200_0000, [100.0, 432.0], IDENTITY, [1.5, 2.0], 4.0, Some([96, 408, 200, 40])));
}

#[test]
fn surface_factors_scale_position_size_and_scissor() {
    let (mut ram, object) = single_image(place(100.0, 40.0, (8.0, 4.0)));
    ram.put32(DISPLAY_MODE.0, 3);
    ram.clip(object, [96, 32, 200, 40]);
    // 360 x 240 at 0.5: translate (50, 240 - 2 - 20), scissor (48, 240 - 20 - 16, 100, 20).
    assert_eq!(object_calls(&ram), image_calls(0x2200_0000, [50.0, 218.0], IDENTITY, [0.5, 0.5], 4.0, Some([48, 204, 100, 20])));
}

#[test]
fn display_and_object_mirror_flip_the_first_matrix() {
    let (mut ram, object) = single_image(place(100.0, 40.0, (8.0, 4.0)));
    ram.put8(object + 0x4b, 1);
    let unmirrored = object_calls(&ram);
    assert_eq!(unmirrored, image_calls(0x2200_0000, [100.0, 436.0], IDENTITY, [1.0, 1.0], 4.0, None));
    ram.put8(DISPLAY_MIRROR.0, 1);
    let mut first = IDENTITY;
    first[0] = -1.0;
    first[6] = 720.0 - 200.0;
    assert_eq!(object_calls(&ram), image_calls(0x2200_0000, [100.0, 436.0], first, [1.0, 1.0], 4.0, None));
}

#[test]
fn an_object_matrix_replaces_the_placement() {
    let (mut ram, object) = single_image(place(100.0, 40.0, (8.0, 4.0)));
    ram.put8(object + 0x4a, 1);
    assert_eq!(object_calls(&ram)[4..6], [VgCall::LoadIdentity, VgCall::LoadMatrix(DataAddress(object + 0x50))]);
    let mut ram = Ram::new();
    let shape = ram.shape(0, place(0.0, 0.0, (20.0, 12.0)), &[]);
    ram.put8(shape + 0x4a, 1);
    ram.group(0, &[(3, shape)]);
    assert_eq!(object_calls(&ram)[4..6], [VgCall::LoadIdentity, VgCall::MultiplyMatrix(Matrix::Guest(DataAddress(shape + 0x50)))]);
}

#[test]
fn shape_scissor_saturates_its_bottom_edge_and_image_scissor_does_not() {
    // Clip y 470, height 20: y-up bottom 480 - 20 - 470 = -10, which vcvt.u32 (6da47c) makes 0 and vcvt.s32 (6d8c94) keeps.
    let (mut ram, object) = single_image(place(0.0, 0.0, (8.0, 4.0)));
    ram.clip(object, [0, 470, 720, 20]);
    assert!(object_calls(&ram).contains(&VgCall::SetIntegers { parameter: SCISSOR_RECTS, values: [0, -10, 720, 20] }));
    let mut ram = Ram::new();
    let shape = ram.shape(0, place(0.0, 0.0, (20.0, 12.0)), &[]);
    ram.clip(shape, [0, 470, 720, 20]);
    ram.group(0, &[(3, shape)]);
    assert!(object_calls(&ram).contains(&VgCall::SetIntegers { parameter: SCISSOR_RECTS, values: [0, 0, 720, 20] }));
}

fn item(fill_rule: u32, modes: u32, fill_paint: u32, stroke_paint: u32, path: u32) -> [u32; 21] {
    let mut item = [0; 21];
    item[0] = fill_rule;
    item[1] = modes;
    item[2] = 0x1700;
    item[3] = 0x1800;
    item[4] = 4.0f32.to_bits();
    item[5] = 1.5f32.to_bits();
    item[6] = fill_paint;
    item[7] = stroke_paint;
    item[8] = path;
    item[12] = 0x29a;
    item
}

#[test]
fn shape_items_set_paints_and_strokes_as_their_change_flags_say() {
    let first = item(0x1901, 3, 0x51, 0x61, 0x71);
    let mut unchanged = item(0x1902, 3, 0x52, 0x62, 0x72);
    unchanged[11] = 1;
    let mut dashed = item(0x1902, 1, 0x53, 0x63, 0x73);
    dashed[9] = 1;
    dashed[12] = 0;
    dashed[13] = 2;
    dashed[14] = 0.5f32.to_bits();
    let hidden = item(0x1901, 0, 0x54, 0x64, 0x74);
    let mut ram = Ram::new();
    let shape = ram.shape(0, Place { x: 50.0, y: 60.0, size: (20.0, 12.0), scale: (2.0, 1.0) }, &[first, unchanged, dashed, hidden]);
    ram.put8(shape + 0x45, 2);
    ram.group(0, &[(3, shape)]);
    let stroke = |paint, dash: Option<(i32, u32)>| {
        let mut calls = vec![
            VgCall::SetFloat { parameter: STROKE_LINE_WIDTH, value: 1.5 }, seti(STROKE_CAP_STYLE, 0x1700),
            seti(STROKE_JOIN_STYLE, 0x1800), VgCall::SetFloat { parameter: STROKE_MITER_LIMIT, value: 4.0 },
        ];
        calls.extend(match dash {
            None => vec![VgCall::SetVector { parameter: STROKE_DASH_PATTERN, count: 0, values: DataAddress(0) }],
            Some((count, values)) => vec![VgCall::SetFloat { parameter: STROKE_DASH_PHASE, value: 0.5 },
                VgCall::SetVector { parameter: STROKE_DASH_PATTERN, count, values: DataAddress(values) }],
        });
        calls.push(VgCall::SetPaint { paint, modes: 1 });
        calls
    };
    let list = shape + 0x200;
    let mut expected = vec![
        seti(MATRIX_MODE, 0x1400), seti(BLEND_MODE, 0x2005), seti(COLOR_TRANSFORM, 0), seti(FILL_RULE, 0x1901),
        VgCall::LoadIdentity,
        VgCall::Translate { x: 50.0, y: 480.0 - 12.0 - 60.0 },
        VgCall::MultiplyMatrix(Matrix::Local(IDENTITY)),
        VgCall::Translate { x: -(2.0 * 2.0), y: -3.0 },
        VgCall::Scale { x: 2.0, y: 1.0 },
        seti(BLEND_MODE, 0x2000), seti(FILL_RULE, 0x1901), VgCall::SetPaint { paint: 0x51, modes: 2 },
    ];
    expected.extend(stroke(0x61, None));
    expected.extend([VgCall::DrawPath { path: 0x71, modes: 3 }, seti(BLEND_MODE, 0x2001), VgCall::DrawPath { path: 0x72, modes: 3 }, seti(BLEND_MODE, 0x2000)]);
    expected.extend(stroke(0x63, Some((2, list + 2 * SHAPE_ITEM_BYTES as u32 + 0x3c))));
    expected.extend([VgCall::DrawPath { path: 0x73, modes: 1 }, seti(BLEND_MODE, 0x2000),
        seti(SCISSORING, 0), seti(COLOR_TRANSFORM, 0), VgCall::SetFloat { parameter: STROKE_LINE_WIDTH, value: 0.0 }]);
    assert_eq!(object_calls(&ram), expected);
}

#[test]
fn the_frame_selects_clears_visits_and_copies() {
    let mut ram = Ram::new();
    let objects: Vec<u32> = (0..5).map(|index| ram.image(index, place(0.0, 0.0, (8.0, 4.0)))).collect();
    let hidden = ram.image(5, place(0.0, 0.0, (8.0, 4.0)));
    ram.put8(hidden + 0x47, 0);
    ram.put32(objects[1] + 0xc4, 0);
    ram.group(0, &[(1, objects[0]), (2, hidden), (3, objects[1]), (4, objects[2]), (5, objects[3]), (6, objects[4])]);
    let mut device = Recorder::new();
    assert_eq!(render(&ram, &mut device), RenderOutcome::Rendered(1));
    let frame: Vec<Event> = device.events.iter().filter(|event| !matches!(event, Event::Call(call) if !matches!(call,
        VgCall::SetVector { parameter: VIEWPORT, .. } | VgCall::Clear { .. } | VgCall::DrawImage(_) | VgCall::Finish))).cloned().collect();
    let draw = |index: u32| Event::Call(VgCall::DrawImage(0x2200_0000 | index));
    assert_eq!(frame, vec![
        Event::MakeCurrent, Event::GetInteger(RENDERING_QUALITY),
        Event::Call(VgCall::SetVector { parameter: VIEWPORT, count: 4, values: VIEWPORT_VALUES }),
        Event::Call(VgCall::Clear { x: 0, y: 0, width: 720, height: 480 }), Event::Visits(0, 0),
        draw(0), Event::Visits(1, 1), Event::Yield,
        Event::Visits(2, 1),
        draw(2), Event::Visits(3, 1),
        draw(3), Event::Visits(4, 1), Event::Yield,
        draw(4), Event::Visits(5, 1), Event::Yield,
        Event::Call(VgCall::Finish), Event::CopyBuffers,
    ]);
}

#[test]
fn frame_errors_follow_the_stock_render() {
    let (ram, _) = single_image(place(0.0, 0.0, (8.0, 4.0)));
    let mut device = Recorder { current: false, ..Recorder::new() };
    assert_eq!(render(&ram, &mut device), RenderOutcome::SurfaceRefused);
    assert_eq!(device.events, vec![Event::MakeCurrent]);
    let mut device = Recorder { quality: 0x1201, copied: false, ..Recorder::new() };
    render(&ram, &mut device);
    assert_eq!(device.events[2], Event::Call(seti(RENDERING_QUALITY, RENDERING_QUALITY_BETTER)));
    assert_eq!(device.events[device.events.len() - 2..], [Event::CopyBuffers, Event::RecordError]);
}

#[test]
fn a_positive_floor_makes_an_undrawn_frame_rendered() {
    assert_eq!(render_floor(&Ram::new(), 0.0, &mut Recorder::new()), RenderOutcome::Rendered(0));
    assert_eq!(render_floor(&Ram::new(), 0.25, &mut Recorder::new()), RenderOutcome::Rendered(1));
    let (mut ram, object) = single_image(place(0.0, 0.0, (8.0, 4.0)));
    ram.put32(object + 0xc4, 0);
    assert_eq!(render(&ram, &mut Recorder::new()), RenderOutcome::Rendered(0));
}

#[test]
fn classes_follow_the_ops_word() {
    let mut ram = Ram::new();
    let image = ram.image(0, place(0.0, 0.0, (8.0, 4.0)));
    let shape = ram.shape(1, place(0.0, 0.0, (20.0, 12.0)), &[]);
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
    let (mut ram, object) = single_image(place(0.0, 0.0, (8.0, 4.0)));
    ram.put32(IMAGE_OPS + 0x04, 0x006d_8160);
    assert_eq!(refusal(&ram), object_refusal(object, ObjectRefusal::OpsTable));
}

#[test]
fn rotation_and_colour_transform_are_refused() {
    let (mut ram, object) = single_image(place(0.0, 0.0, (8.0, 4.0)));
    ram.putf(object + 0x1c, 90.0);
    assert_eq!(refusal(&ram), object_refusal(object, ObjectRefusal::Rotation));
    let (mut ram, object) = single_image(place(0.0, 0.0, (8.0, 4.0)));
    ram.put8(object + 0x45, 1);
    assert_eq!(refusal(&ram), object_refusal(object, ObjectRefusal::ColourTransform));
}

#[test]
fn hidden_objects_are_not_visits_and_resourceless_ones_draw_nothing() {
    let (mut ram, object) = single_image(place(0.0, 0.0, (8.0, 4.0)));
    ram.put8(object + 0x47, 0);
    let mut device = Recorder::new();
    render(&ram, &mut device);
    assert!(!device.events.iter().any(|event| matches!(event, Event::Visits(1, _))));
    assert_eq!(object_calls(&ram), vec![]);
    let (mut ram, object) = single_image(place(0.0, 0.0, (8.0, 4.0)));
    ram.put32(object + 0xc4, 0);
    let mut device = Recorder::new();
    render(&ram, &mut device);
    assert!(device.events.contains(&Event::Visits(1, 0)));
    assert_eq!(object_calls(&ram), vec![]);
}

#[test]
fn dual_render_frame_modes_are_refused() {
    let (mut ram, _) = single_image(place(0.0, 0.0, (8.0, 4.0)));
    ram.put32(FRAME_MODE.0, 8);
    assert_eq!(refusal(&ram), SceneError::DualRender(8));
}

#[test]
fn malformed_lists_are_refused() {
    let (mut ram, object) = single_image(place(0.0, 0.0, (8.0, 4.0)));
    ram.group(4, &[(1, object), (2, object)]);
    ram.put32(node(2), node(1));
    assert_eq!(refusal(&ram), SceneError::RepeatedNode { group: group(4), node: DataAddress(node(1)) });

    let (mut ram, _) = single_image(place(0.0, 0.0, (8.0, 4.0)));
    ram.put32(sentinel(6), 0x008f_a300);
    assert_eq!(refusal(&ram), SceneError::NodeOutsidePool { group: group(6), node: DataAddress(0x008f_a300) });
    ram.put32(sentinel(6), node(4) + 4);
    assert_eq!(refusal(&ram), SceneError::NodeOutsidePool { group: group(6), node: DataAddress(node(4) + 4) });

    let (mut ram, _) = single_image(place(0.0, 0.0, (8.0, 4.0)));
    ram.put8(0x008f_a3c4, 2);
    assert_eq!(refusal(&ram), SceneError::CountMismatch { group: group(0), count: 2, listed: 1 });

    let (mut ram, _) = single_image(place(0.0, 0.0, (8.0, 4.0)));
    ram.put32(node(3) + 8, 0);
    assert_eq!(refusal(&ram), SceneError::NullObject { group: group(0), node: DataAddress(node(3)) });

    let (mut ram, _) = single_image(place(0.0, 0.0, (8.0, 4.0)));
    ram.put32(node(3) + 0xc, 0);
    assert_eq!(refusal(&ram), SceneError::FreeNode { group: group(0), node: DataAddress(node(3)) });
}

#[test]
fn a_full_node_pool_cannot_copy_a_group() {
    let (mut ram, _) = single_image(place(0.0, 0.0, (8.0, 4.0)));
    for slot in 0..NODE_COUNT { ram.put32(node(slot) + 0xc, 1); }
    assert_eq!(refusal(&ram), SceneError::NodeCapacity { free: 0, needed: 1 });
    // Mid-frame the group goes to stock's walk, which fails the copy as stock does; empty groups need no node.
    let mut device = Recorder::new();
    render(&ram, &mut device);
    assert_eq!(device.events.iter().filter(|event| matches!(event, Event::StockWalk(_))).cloned().collect::<Vec<_>>(), vec![Event::StockWalk(0)]);
    assert_eq!(device.drawn(), vec![]);
}

#[test]
fn a_list_longer_than_the_copy_goes_to_the_stock_walk() {
    let mut ram = Ram::new();
    let entries: Vec<(usize, u32)> = (0..17).map(|index| (index as usize + 1, ram.image(index, place(0.0, 0.0, (8.0, 4.0))))).collect();
    ram.group(0, &entries);
    let other = ram.image(20, place(0.0, 0.0, (8.0, 4.0)));
    ram.group(1, &[(30, other)]);
    let mut device = Recorder::new();
    render(&ram, &mut device);
    assert!(device.events.contains(&Event::StockWalk(0)));
    assert_eq!(device.drawn(), vec![0x2200_0014]);
}

/// Five images in group 0; the render yields after the first, fourth and fifth visit.
fn five_images() -> (Ram, Vec<u32>) {
    let mut ram = Ram::new();
    let objects: Vec<u32> = (0..5).map(|index| ram.image(index, place(0.0, 0.0, (8.0, 4.0)))).collect();
    ram.group(0, &objects.iter().enumerate().map(|(index, object)| (index + 1, *object)).collect::<Vec<_>>());
    (ram, objects)
}

#[test]
fn visibility_is_read_when_each_object_is_visited() {
    let (ram, objects) = five_images();
    let live = Live(RefCell::new(ram));
    let (hide, show) = (objects[3], objects[1]);
    live.0.borrow_mut().put8(show + 0x47, 0);
    let mut device = Recorder::changing(&live, move |ram: &mut Ram| { ram.put8(hide + 0x47, 0); ram.put8(show + 0x47, 1); });
    assert_eq!(render_live(&live, &mut device), RenderOutcome::Rendered(1));
    assert_eq!(device.drawn(), vec![0x2200_0000, 0x2200_0001, 0x2200_0002, 0x2200_0004]);
}

#[test]
fn an_object_refused_mid_frame_is_drawn_by_the_stock_visitor() {
    let (ram, objects) = five_images();
    let live = Live(RefCell::new(ram));
    let rotated = objects[1];
    let mut device = Recorder::changing(&live, move |ram: &mut Ram| ram.putf(rotated + 0x1c, 90.0));
    render_live(&live, &mut device);
    let walk: Vec<Event> = device.events.iter().filter(|event| matches!(event, Event::StockVisit(_) | Event::Visits(..) | Event::Call(VgCall::DrawImage(_)))).cloned().collect();
    let draw = |index: u32| Event::Call(VgCall::DrawImage(0x2200_0000 | index));
    assert_eq!(walk, vec![
        Event::Visits(0, 0), draw(0), Event::Visits(1, 1),
        Event::StockVisit(rotated),
        draw(2), Event::Visits(3, 1), draw(3), Event::Visits(4, 1), draw(4), Event::Visits(5, 1),
    ]);
}

#[test]
fn each_group_list_is_copied_when_the_walk_reaches_it() {
    let (mut ram, objects) = five_images();
    let late = ram.image(9, place(0.0, 0.0, (8.0, 4.0)));
    let live = Live(RefCell::new(ram));
    // After the first visit: drop the last object from group 0, whose copy still holds it, and list one in group 7,
    // which the walk has already passed.
    let kept = objects[..4].to_vec();
    let mut device = Recorder::changing(&live, move |ram: &mut Ram| {
        ram.group(0, &kept.iter().enumerate().map(|(index, object)| (index + 1, *object)).collect::<Vec<_>>());
        ram.group(7, &[(40, late)]);
    });
    render_live(&live, &mut device);
    assert_eq!(device.drawn(), vec![0x2200_0000, 0x2200_0001, 0x2200_0002, 0x2200_0003, 0x2200_0004]);

    let (mut ram, objects) = five_images();
    let late = ram.image(9, place(0.0, 0.0, (8.0, 4.0)));
    let first = ram.image(10, place(0.0, 0.0, (8.0, 4.0)));
    ram.group(23, &[(50, first)]);
    ram.group(0, &[(1, objects[0])]);
    let live = Live(RefCell::new(ram));
    let mut device = Recorder::changing(&live, move |ram: &mut Ram| ram.group(7, &[(40, late)]));
    render_live(&live, &mut device);
    assert_eq!(device.drawn(), vec![0x2200_000a, 0x2200_0009, 0x2200_0000]);
}

#[test]
fn a_null_object_ends_only_its_group() {
    let mut ram = Ram::new();
    let objects: Vec<u32> = (0..3).map(|index| ram.image(index, place(0.0, 0.0, (8.0, 4.0)))).collect();
    ram.group(5, &[(1, objects[0]), (2, 0), (3, objects[1])]);
    ram.group(4, &[(4, objects[2])]);
    let mut device = Recorder::new();
    render(&ram, &mut device);
    assert_eq!(device.drawn(), vec![0x2200_0000, 0x2200_0002]);
}

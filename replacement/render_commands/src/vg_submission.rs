//! The OpenVG and EGL calls VGLIB's stage-0 render `0x6d724c` makes into the retained OpenVG client, which turns
//! them into the 104-byte jobs task 71 queues for the vector engine (reports/display-render-contracts/vglib-frame.md,
//! "Vector-engine submission"). Addresses are image-4 entries (`lz/id4.dis`).

use super::resources::DataAddress;
use super::stock_objects::VglibObject;
use super::vglib_scene::GroupIndex;

/// An OpenVG parameter or value word as the draws pass it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VgWord(pub u32);

pub const MATRIX_MODE: VgWord = VgWord(0x1100);
pub const FILL_RULE: VgWord = VgWord(0x1101);
pub const IMAGE_MODE: VgWord = VgWord(0x1102);
pub const RENDERING_QUALITY: VgWord = VgWord(0x1103);
pub const BLEND_MODE: VgWord = VgWord(0x1104);
pub const IMAGE_QUALITY: VgWord = VgWord(0x1105);
pub const SCISSOR_RECTS: VgWord = VgWord(0x1106);
pub const STROKE_LINE_WIDTH: VgWord = VgWord(0x1110);
pub const STROKE_CAP_STYLE: VgWord = VgWord(0x1111);
pub const STROKE_JOIN_STYLE: VgWord = VgWord(0x1112);
pub const STROKE_MITER_LIMIT: VgWord = VgWord(0x1113);
pub const STROKE_DASH_PATTERN: VgWord = VgWord(0x1114);
pub const STROKE_DASH_PHASE: VgWord = VgWord(0x1115);
pub const VIEWPORT: VgWord = VgWord(0x1121);
pub const SCISSORING: VgWord = VgWord(0x1131);
pub const COLOR_TRANSFORM: VgWord = VgWord(0x1170);
/// Rendering quality the frame's surface selection requires (`I4:6d78f0-6d7910`).
pub const RENDERING_QUALITY_BETTER: i32 = 0x1202;

/// One call whose return value the draws do not read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VgCall {
    /// `0x858c04(parameter, value)`.
    SetInteger { parameter: VgWord, value: i32 },
    /// `0x858b78(parameter, value)`; the value travels as its bits in r1.
    SetFloat { parameter: VgWord, value: f32 },
    /// `0x858c90(parameter, count, values)` with values in guest memory.
    SetVector { parameter: VgWord, count: i32, values: DataAddress },
    /// `0x858d24(parameter, 4, values)`, the scissor rectangle from the draw's frame.
    SetIntegers { parameter: VgWord, values: [i32; 4] },
    /// `0x85a1b0(x, y, width, height)`.
    Clear { x: i32, y: i32, width: i32, height: i32 },
    /// `0x858748()`.
    LoadIdentity,
    /// `0x8587c4(matrix)`.
    LoadMatrix(DataAddress),
    /// `0x8588cc(matrix)`.
    MultiplyMatrix(Matrix),
    /// `0x858950(tx, ty)`.
    Translate { x: f32, y: f32 },
    /// `0x858af4(degrees)`.
    Rotate(f32),
    /// `0x8589dc(sx, sy)`.
    Scale { x: f32, y: f32 },
    /// `0x85afbc(paint, modes)`.
    SetPaint { paint: u32, modes: u32 },
    /// `0x85ab88(path, modes)`.
    DrawPath { path: u32, modes: u32 },
    /// `0x85b930(image)`.
    DrawImage(u32),
    /// `0x8586d8()`, after the traversal.
    Finish,
}

/// A 3x3 matrix argument: nine floats in the draw's frame, or an object's own matrix in guest memory.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Matrix { Local([f32; 9]), Guest(DataAddress) }

/// The retained OpenVG client, EGL and VGLIB state the stage-0 render drives.
pub trait StageDevice {
    fn call(&mut self, call: VgCall);
    /// `0x88bf94(R+0x84, surface, surface, R+0x88, attributes)`; false when it returns 0.
    fn make_current(&mut self) -> bool;
    /// `0x858e60(parameter)`.
    fn get_integer(&mut self, parameter: VgWord) -> i32;
    /// `0x88d620(R+0x84, surface, object)`; false when it returns 0.
    fn copy_buffers(&mut self) -> bool;
    /// `0x6d7230`: `R+0x50 = 0x887280()`.
    fn record_error(&mut self);
    /// Writes `R+0x158` and `R+0x15c`.
    fn set_visits(&mut self, visited: u32, result: u32);
    /// Reads `R+0x158` and `R+0x15c`.
    fn visits(&mut self) -> (u32, u32);
    /// `0x723ce8(0)`.
    fn yield_task(&mut self);
    /// Stock's visitor `0x6d7324(&-1, object)`, which counts, draws and delays through `R+0x158`/`R+0x15c` itself.
    fn stock_visit(&mut self, object: VglibObject);
    /// Stock's group walk `0x6d909c(sentinel, 0x6d7324, &-1)`: copy the list, then the stock visitor per object.
    fn stock_walk(&mut self, group: GroupIndex);
}

/// The attribute list `vglib_select_surface` copies from `0x774b10` (`I4:6d789c-6d78bc`).
pub const SURFACE_ATTRIBUTES: [u32; 11] = [
    0x3fd8, 0x0021_c000, 0x3fd9, 0x42c2_4000, 0x3fda, 0x42c2_4000, 0x3fdb, 0x42d3_2000, 0x3fdc, 0x42d3_2000, 0x3038,
];
/// `R+0x148`, the viewport VGLIB copies before a frame (`I4:6d766c-6d7670`).
pub const VIEWPORT_VALUES: DataAddress = DataAddress(0x008e_2098 + 0x148);

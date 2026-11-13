#![forbid(unsafe_code)]

//! MVREC, the ISP unit with two identical register banks.

use crate::driver::{admit, Bank, Client, Isp, Operation, Refusal, Register, Unit};

const UNIT: Unit = Unit::Mvrec;

const DIMENSIONS: u16 = 0x04;
const GEOMETRY: u16 = 0x08;
const GEOMETRY_SECOND: u16 = 0x0c;
const OFFSETS: [u16; 3] = [0x10, 0x14, 0x18];
const WINDOW: u16 = 0x1c;
const WINDOW_REPEAT: u16 = 0x20;
const TABLE: u16 = 0x30;
const SECONDARY_DIMENSIONS: u16 = 0xb0;
const SECONDARY_LAYOUT: u16 = 0xb4;
const READBACK: u16 = 0xb8;
const ADDRESS: u16 = 0xc0;
const ADDRESS_STRIDE: u16 = 0xc4;
const NIBBLE: u16 = 0xf4;

const COORDINATE: u32 = 0x1fff;

fn write<I: Isp>(isp: &mut I, bank: Bank, offset: u16, value: u32) { isp.write(Register::Mvrec(bank, offset), value) }

fn read<I: Isp>(isp: &mut I, bank: Bank, offset: u16) -> u32 { isp.read(Register::Mvrec(bank, offset)) }

/// Two sizes of 1..=0x1000, stored less one in 12-bit halves.
pub fn set_dimensions<I: Isp>(isp: &mut I, client: Client, bank: Bank, size: [u16; 2]) -> Result<(), Refusal> {
    admit(isp, UNIT, client, Operation(5), &size.map(|value| (1..=0x1000).contains(&value)))?;
    let [first, second] = size.map(|value| u32::from(value - 1));
    write(isp, bank, DIMENSIONS, second << 16 | first);
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Geometry { pub enabled: bool, pub first: u16, pub second: u16 }

pub(crate) const SET_GEOMETRY: Operation = Operation(7);

pub fn set_geometry<I: Isp>(isp: &mut I, bank: Bank, geometry: Geometry) {
    isp.note(UNIT, SET_GEOMETRY);
    write(isp, bank, GEOMETRY, u32::from(geometry.enabled) << 31 | u32::from(geometry.first));
    write(isp, bank, GEOMETRY_SECOND, u32::from(geometry.second));
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Offset { First, Second, Third }

/// Two coordinates of at most 0x1fff, the first in the low half.
pub fn set_offset<I: Isp>(isp: &mut I, client: Client, bank: Bank, offset: Offset, values: [u32; 2]) -> Result<(), Refusal> {
    let (register, operation) = match offset {
        Offset::First => (OFFSETS[0], Operation(9)),
        Offset::Second => (OFFSETS[1], Operation(11)),
        Offset::Third => (OFFSETS[2], Operation(13)),
    };
    admit(isp, UNIT, client, operation, &values.map(|value| value <= COORDINATE))?;
    write(isp, bank, register, values[1] << 16 | values[0]);
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Window {
    /// Each below 0x8000, the first in the low half.
    pub position: [u16; 2],
    /// At most 8.
    pub repeat: u8,
    /// At most 32.
    pub count: u8,
}

pub fn set_window<I: Isp>(isp: &mut I, client: Client, bank: Bank, window: Window) -> Result<(), Refusal> {
    let checks = [window.position[0] < 0x8000, window.position[1] < 0x8000, window.repeat <= 8, window.count <= 32];
    admit(isp, UNIT, client, Operation(15), &checks)?;
    write(isp, bank, WINDOW, u32::from(window.position[1]) << 16 | u32::from(window.position[0]));
    write(isp, bank, WINDOW_REPEAT, u32::from(window.count) << 16 | u32::from(window.repeat));
    Ok(())
}

/// The window as the bank holds it, with the repeat and count fields read back as 4 and 6 bits.
pub fn window<I: Isp>(isp: &mut I, bank: Bank) -> Window {
    isp.note(UNIT, Operation(16));
    let position = read(isp, bank, WINDOW);
    let repeat = read(isp, bank, WINDOW_REPEAT);
    Window {
        position: [(position & 0x7fff) as u16, (position >> 16 & 0x7fff) as u16],
        repeat: (repeat & 0xf) as u8,
        count: (repeat >> 16 & 0x3f) as u8,
    }
}

pub fn load_table<I: Isp>(isp: &mut I, bank: Bank, entries: &[u32; 31]) {
    isp.note(UNIT, Operation(17));
    for (index, entry) in (0u16..).zip(entries) {
        write(isp, bank, TABLE + 4 * index, entry & 0x1_ffff);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecondaryGeometry {
    /// A multiple of 8 in 8..=0x800.
    pub width: u16,
    /// 1..=0x800.
    pub height: u16,
    /// A multiple of 8 below 0x800.
    pub offset: u16,
    /// A multiple of 8 in 8..=0x800.
    pub stride: u16,
}

pub fn set_secondary_geometry<I: Isp>(isp: &mut I, client: Client, bank: Bank, geometry: SecondaryGeometry) -> Result<(), Refusal> {
    let eighths = |value: u16| value & !0xff8 == 0;
    let checks = [
        (8..=0x800).contains(&geometry.width) && eighths(geometry.width),
        (1..=0x800).contains(&geometry.height),
        (8..=0x800).contains(&geometry.stride) && eighths(geometry.stride),
        geometry.offset <= 0x7f8 && eighths(geometry.offset),
    ];
    admit(isp, UNIT, client, Operation(19), &checks)?;
    let less_one = |value: u16| u32::from(value - 1);
    write(isp, bank, SECONDARY_DIMENSIONS, less_one(geometry.height) << 16 | less_one(geometry.width));
    write(isp, bank, SECONDARY_LAYOUT, u32::from(geometry.offset) << 16 | less_one(geometry.stride));
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadbackSource { First, Second, Third, Fourth }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadbackMode { First, Second, Third, Fourth, Fifth }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Readback {
    pub enabled: bool,
    /// Below 0x100.
    pub region: u16,
    pub source: ReadbackSource,
    pub mode: ReadbackMode,
}

pub(crate) const SET_READBACK: Operation = Operation(0x15);

pub fn set_readback<I: Isp>(isp: &mut I, client: Client, bank: Bank, readback: Readback) -> Result<(), Refusal> {
    admit(isp, UNIT, client, SET_READBACK, &[readback.region < 0x100])?;
    let source = match readback.source {
        ReadbackSource::First => 0,
        ReadbackSource::Second => 1,
        ReadbackSource::Third => 2,
        ReadbackSource::Fourth => 3,
    };
    let mode = match readback.mode {
        ReadbackMode::First => 0,
        ReadbackMode::Second => 1,
        ReadbackMode::Third => 2,
        ReadbackMode::Fourth => 3,
        ReadbackMode::Fifth => 4,
    };
    let word = u32::from(readback.enabled) << 24 | u32::from(readback.region) << 16 | source << 4 | mode;
    write(isp, bank, READBACK, word);
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Address {
    /// 16-byte aligned, at most 0x3ffffff0.
    pub address: u32,
    /// A multiple of 8, at most 0x1ff8.
    pub stride: u16,
}

pub fn set_address<I: Isp>(isp: &mut I, client: Client, bank: Bank, address: Address) -> Result<(), Refusal> {
    let checks = [address.address & 0xc000_000f == 0, address.stride & !0x1ff8 == 0];
    admit(isp, UNIT, client, Operation(23), &checks)?;
    write(isp, bank, ADDRESS, address.address);
    write(isp, bank, ADDRESS_STRIDE, u32::from(address.stride));
    Ok(())
}

/// Programs `low`; `checked` is validated and not programmed. Both at most 15.
pub fn set_nibble<I: Isp>(isp: &mut I, client: Client, bank: Bank, low: u8, checked: u8) -> Result<(), Refusal> {
    admit(isp, UNIT, client, Operation(30), &[low <= 15, checked <= 15])?;
    write(isp, bank, NIBBLE, u32::from(low));
    Ok(())
}

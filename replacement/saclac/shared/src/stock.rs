//! The stock boundary: X-M1 1.50 addresses of the memory and entries the shared layer uses.

use core::ptr::{read_volatile, write_volatile};

use xm1_saclac_interface::stock::{callback_word, reply_target_code, StockCallbackWord, PARAMETER_BLOCKS};
use xm1_saclac_interface::{ClientCallback, ReplyTarget};

use crate::commands::{ClientArguments, Clients, CommandMemory, StoredCommand};
use crate::display::{Display, DisplayRoute, Outputs};
use crate::image_registers::{pack_window_limits, RegisterGroup, RegisterSnapshot, WindowLimits};
use crate::lens::{LensBounds, LensControl, LensNotices, LensProbe, Notice, ProbeState, Rendezvous};
use crate::lens_com::{LensComClient, RequestNumber};
use crate::settings::{
    halfword, AdjustmentKey, Adjustments, Aspect, CaptureMode, ChannelRemap, Conversions, RemapArguments, RemappedWord,
    WindowVariant,
};

/// An argument of a retained entry: a word, or the address of a byte of the caller's buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Argument {
    Word(u32),
    Buffer(usize),
}

/// The most arguments a retained entry `Guest` enters takes.
const MOST_ARGUMENTS: usize = 17;

/// The machine the boundary reads, writes and enters.
pub trait Machine {
    fn read_u8(&self, address: u32) -> u8;
    fn read_u16(&self, address: u32) -> u16;
    fn read_u32(&self, address: u32) -> u32;
    fn write_u8(&mut self, address: u32, value: u8);
    fn write_u16(&mut self, address: u32, value: u16) {
        for (offset, byte) in value.to_le_bytes().into_iter().enumerate() { self.write_u8(address + offset as u32, byte); }
    }
    fn write_u32(&mut self, address: u32, value: u32);
    /// Enters the retained entry at `address` with `arguments` in r0.. and returns r0.
    fn enter(&self, address: u32, arguments: &[usize]) -> u32;
    /// Enters the retained entry at `address` with `arguments`, which may read and write `buffer`.
    fn enter_with(&self, address: u32, arguments: &[Argument], buffer: &mut [u8]) -> u32 {
        let base = buffer.as_mut_ptr() as usize;
        let mut words = [0; MOST_ARGUMENTS];
        for (word, argument) in words.iter_mut().zip(arguments) {
            *word = match *argument {
                Argument::Word(value) => value as usize,
                Argument::Buffer(offset) => base + offset,
            };
        }
        self.enter(address, &words[..arguments.len()])
    }
}

/// The X-M1 1.50 machine.
#[derive(Clone, Copy)]
pub struct Guest;

impl Machine for Guest {
    fn read_u8(&self, address: u32) -> u8 { unsafe { read_volatile(address as usize as *const u8) } }

    fn read_u16(&self, address: u32) -> u16 { unsafe { read_volatile(address as usize as *const u16) } }

    fn read_u32(&self, address: u32) -> u32 { unsafe { read_volatile(address as usize as *const u32) } }

    fn write_u8(&mut self, address: u32, value: u8) { unsafe { write_volatile(address as usize as *mut u8, value) } }

    fn write_u16(&mut self, address: u32, value: u16) { unsafe { write_volatile(address as usize as *mut u16, value) } }

    fn write_u32(&mut self, address: u32, value: u32) { unsafe { write_volatile(address as usize as *mut u32, value) } }

    fn enter(&self, address: u32, arguments: &[usize]) -> u32 {
        let entry = address as usize;
        unsafe {
            match arguments {
                [] => core::mem::transmute::<usize, extern "C" fn() -> u32>(entry)(),
                [a] => core::mem::transmute::<usize, extern "C" fn(u32) -> u32>(entry)(*a as u32),
                [a, b] => core::mem::transmute::<usize, extern "C" fn(u32, u32) -> u32>(entry)(*a as u32, *b as u32),
                [a, b, c] => {
                    core::mem::transmute::<usize, extern "C" fn(u32, u32, u32) -> u32>(entry)(*a as u32, *b as u32, *c as u32)
                }
                [a, b, c, d] => core::mem::transmute::<usize, extern "C" fn(u32, u32, u32, u32) -> u32>(entry)(
                    *a as u32,
                    *b as u32,
                    *c as u32,
                    *d as u32,
                ),
                [a, b, c, d, e] => core::mem::transmute::<usize, extern "C" fn(u32, u32, u32, u32, u32) -> u32>(entry)(
                    *a as u32,
                    *b as u32,
                    *c as u32,
                    *d as u32,
                    *e as u32,
                ),
                [a, b, c, d, e, f] => {
                    core::mem::transmute::<usize, extern "C" fn(u32, u32, u32, u32, u32, u32) -> u32>(entry)(
                        *a as u32,
                        *b as u32,
                        *c as u32,
                        *d as u32,
                        *e as u32,
                        *f as u32,
                    )
                }
                [a, b, c, d, e, f, g] => {
                    core::mem::transmute::<usize, extern "C" fn(u32, u32, u32, u32, u32, u32, u32) -> u32>(entry)(
                        *a as u32,
                        *b as u32,
                        *c as u32,
                        *d as u32,
                        *e as u32,
                        *f as u32,
                        *g as u32,
                    )
                }
                [a, b, c, d, e, f, g, h] => core::mem::transmute::<usize, extern "C" fn(u32, u32, u32, u32, u32, u32, u32, u32) -> u32>(
                    entry,
                )(*a as u32, *b as u32, *c as u32, *d as u32, *e as u32, *f as u32, *g as u32, *h as u32),
                [a, b, c, d, e, f, g, h, i] => core::mem::transmute::<
                    usize,
                    extern "C" fn(u32, u32, u32, u32, u32, u32, u32, u32, u32) -> u32,
                >(entry)(*a as u32, *b as u32, *c as u32, *d as u32, *e as u32, *f as u32, *g as u32, *h as u32, *i as u32),
                [a, b, c, d, e, f, g, h, i, j] => core::mem::transmute::<
                    usize,
                    extern "C" fn(u32, u32, u32, u32, u32, u32, u32, u32, u32, u32) -> u32,
                >(entry)(*a as u32, *b as u32, *c as u32, *d as u32, *e as u32, *f as u32, *g as u32, *h as u32, *i as u32, *j as u32),
                [a, b, c, d, e, f, g, h, i, j, k, l] => core::mem::transmute::<
                    usize,
                    extern "C" fn(u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32) -> u32,
                >(entry)(*a as u32, *b as u32, *c as u32, *d as u32, *e as u32, *f as u32, *g as u32, *h as u32, *i as u32, *j as u32, *k as u32, *l as u32),
                [a, b, c, d, e, f, g, h, i, j, k, l, m] => core::mem::transmute::<
                    usize,
                    extern "C" fn(u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32) -> u32,
                >(entry)(
                    *a as u32, *b as u32, *c as u32, *d as u32, *e as u32, *f as u32, *g as u32, *h as u32, *i as u32, *j as u32,
                    *k as u32, *l as u32, *m as u32,
                ),
                [a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q] => core::mem::transmute::<
                    usize,
                    extern "C" fn(u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32) -> u32,
                >(entry)(
                    *a as u32, *b as u32, *c as u32, *d as u32, *e as u32, *f as u32, *g as u32, *h as u32, *i as u32, *j as u32,
                    *k as u32, *l as u32, *m as u32, *n as u32, *o as u32, *p as u32, *q as u32,
                ),
                _ => panic!("no retained entry takes {} arguments", arguments.len()),
            }
        }
    }
}

/// The shared layer's ports over a machine's memory and entries.
pub struct Stock<M> {
    machine: M,
}

impl<M: Machine> Stock<M> {
    pub const fn new(machine: M) -> Self { Self { machine } }
}

/// One pending byte per command code (0xef9aec).
const PENDING_FLAGS: u32 = 0x169_cf24;

impl<M: Machine> Stock<M> {
    fn block_address(command: StoredCommand, index: usize) -> u32 {
        PARAMETER_BLOCKS[usize::from(command.code())].address + 4 * index as u32
    }
}

impl<M: Machine> CommandMemory for Stock<M> {
    fn word(&self, command: StoredCommand, index: usize) -> u32 { self.machine.read_u32(Self::block_address(command, index)) }

    fn set_word(&mut self, command: StoredCommand, index: usize, value: u32) {
        self.machine.write_u32(Self::block_address(command, index), value);
    }

    fn set_pending(&mut self, target: ReplyTarget, pending: bool) {
        self.machine.write_u8(PENDING_FLAGS + u32::from(reply_target_code(target)), u8::from(pending));
    }
}

impl<M: Machine> Clients for Stock<M> {
    fn enter(&mut self, callback: ClientCallback, arguments: ClientArguments) {
        let word = callback_word(callback);
        match arguments {
            ClientArguments::Status(status) => self.machine.enter(word, &[status.0 as usize]),
            ClientArguments::Reply { status, words: [first, second, third, fourth] } => {
                self.machine.enter(word, &[status.0 as usize, first as usize, second as usize, third as usize, fourth as usize])
            }
        };
    }
}

const LENS_RUNTIME: u32 = 0x94_2ab0;
const LENS_STEP: u32 = LENS_RUNTIME + 0x98;
const LENS_PROBES: [u32; 3] = [0x76_d9a8, 0x76_da38, 0x76_dba4];
const LENS_VALUE: u32 = 0x76_da48;
const LENS_PREFERS_RELATIVE: u32 = 0x76_b8b4;
const LENS_ZONE_LIMIT: u32 = 0x77_2f28;

impl<M: Machine> LensControl for Stock<M> {
    fn probe(&mut self, probe: LensProbe) -> ProbeState {
        let address = match probe {
            LensProbe::First => LENS_PROBES[0],
            LensProbe::Second => LENS_PROBES[1],
            LensProbe::Third => LENS_PROBES[2],
        };
        match self.machine.enter(address, &[]) {
            2 => ProbeState::Overridden,
            1 => ProbeState::Reported,
            _ => ProbeState::Clear,
        }
    }

    fn value(&mut self) -> u32 { self.machine.enter(LENS_VALUE, &[]) }

    fn step(&mut self) -> u32 { self.machine.read_u32(LENS_STEP) }

    fn prefers_relative(&mut self) -> bool { self.machine.enter(LENS_PREFERS_RELATIVE, &[]) != 0 }

    fn zone_limit(&mut self, zone: u8) -> i16 { self.machine.enter(LENS_ZONE_LIMIT, &[usize::from(zone)]) as i16 }
}

/// The lens control block other tasks write the notices into.
const LENS_NOTICES: u32 = 0x95_4dbc;

impl<M: Machine> LensNotices for Stock<M> {
    fn notice(&self, notice: Notice) -> u32 {
        let offset = match notice {
            Notice::First => 0x4,
            Notice::Second => 0x8,
            Notice::Third => 0xc,
        };
        self.machine.read_u32(LENS_NOTICES + offset)
    }

    fn clear_notices(&mut self) {
        for offset in [0x4, 0x8, 0xc] {
            self.machine.write_u32(LENS_NOTICES + offset, 0);
        }
    }

    fn notifier_enabled(&self) -> bool { self.machine.read_u32(LENS_NOTICES + 0x20) != 0 }

    fn notifier(&self) -> Option<StockCallbackWord> {
        Some(self.machine.read_u32(LENS_NOTICES + 0x1c)).filter(|word| *word != 0).map(StockCallbackWord)
    }

    /// The notifier takes the address of the detail word followed by the bounds (0xefba28..0xefba3c).
    fn enter_notifier(&mut self, notifier: StockCallbackWord, detail: u32, bounds: LensBounds) {
        let [first, second, third] = bounds.0;
        let record = [detail, first, second, third];
        self.machine.enter(notifier.0, &[record.as_ptr() as usize]);
    }
}

/// The task a lens synchronization releases (0xf472d4) and the callback lensdc enters, which
/// sets event 2 on it (0xf46be0).
const SYNCHRONIZING_TASK: u32 = 0x16c_2214;
const SYNCHRONIZED: StockCallbackWord = StockCallbackWord(0xf4_6be0);
const CURRENT_TASK: u32 = 0x72_3cb8;
const WAIT: u32 = 0x72_43a8;

impl<M: Machine> Rendezvous for Stock<M> {
    fn arm(&mut self) -> StockCallbackWord {
        let task = self.machine.enter(CURRENT_TASK, &[]);
        self.machine.write_u32(SYNCHRONIZING_TASK, task);
        SYNCHRONIZED
    }

    fn wait(&mut self) {
        let mut flags = 0u32;
        self.machine.enter(WAIT, &[2, &mut flags as *mut u32 as usize, u32::MAX as usize]);
    }
}

const ADJUSTMENT_BYTE: u32 = 0x69_f8fc;
const LENS_ADJUSTMENT_KEY: AdjustmentKey = AdjustmentKey(0x3e_0243);
const UNSELECTED_MODES_KEY: u32 = 0x887c;

impl<M: Machine> Adjustments for Stock<M> {
    fn byte(&self, key: AdjustmentKey) -> u8 { self.machine.enter(ADJUSTMENT_BYTE, &[key.0 as usize]) as u8 }

    fn lens_adjustment(&self) -> u16 { halfword(self, LENS_ADJUSTMENT_KEY) }

    fn admits_unselected_mode(&self) -> bool {
        self.machine.enter(ADJUSTMENT_BYTE, &[UNSELECTED_MODES_KEY as usize]) as u8 != 0
    }
}

const REMAP: u32 = 0x74_947c;

impl<M: Machine> ChannelRemap for Stock<M> {
    fn remap(&mut self, arguments: RemapArguments) -> RemappedWord {
        let [a, b, c, d, e, f, g] = arguments.0;
        let result = self
            .machine
            .enter(REMAP, &[a as usize, b as usize, c as usize, d as usize, e as usize, f as usize, g as usize]);
        RemappedWord(result as u16)
    }
}

const CONVERSION_VARIANT: u32 = 0x73_958c;
const CONVERSION_VALUE: u32 = 0x73_4540;
const CONVERSION_ALTERNATE: u32 = 0x73_469c;

const fn aspect_index(aspect: Aspect) -> u32 {
    match aspect {
        Aspect::ThreeFour => 0,
        Aspect::TwoThree => 1,
        Aspect::NineSixteen => 2,
        Aspect::Square => 3,
    }
}

impl<M: Machine> Conversions for Stock<M> {
    fn window_variant(&mut self, mode: CaptureMode, aspect: Aspect) -> Option<WindowVariant> {
        let variant = self.machine.enter(CONVERSION_VARIANT, &[usize::from(mode.get()), aspect_index(aspect) as usize]);
        (variant != 0xff).then_some(WindowVariant(variant as u8))
    }

    fn value(&mut self, variant: WindowVariant, aspect: Aspect) -> u32 {
        self.machine.enter(CONVERSION_VALUE, &[usize::from(variant.0), aspect_index(aspect) as usize])
    }

    fn alternate(&mut self, variant: WindowVariant, aspect: Aspect) -> u32 {
        self.machine.enter(CONVERSION_ALTERNATE, &[usize::from(variant.0), aspect_index(aspect) as usize])
    }
}

/// The descriptor table and descriptor count each group restores through 0x6deb08
/// (0x693df8..0x694060).
const RESTORE_DESCRIPTORS: [(u32, u32); 15] = [
    (0x82_158c, 3),
    (0x82_15b0, 9),
    (0x82_161c, 2),
    (0x82_1634, 3),
    (0x82_1658, 2),
    (0x82_1670, 6),
    (0x82_16b8, 3),
    (0x82_16dc, 1),
    (0x82_16e8, 2),
    (0x82_1700, 2),
    (0x82_1718, 7),
    (0x82_176c, 2),
    (0x82_1784, 1),
    (0x82_1790, 10),
    (0x82_1808, 3),
];

/// Each group's packer, entered with the snapshot then the configuration (0x69408c..0x6942bc);
/// group 2's is owned.
const PACKERS: [Option<u32>; 15] = [
    Some(0x69_13a8),
    Some(0x69_191c),
    None,
    Some(0x69_1f74),
    Some(0x69_2190),
    Some(0x69_23d0),
    Some(0x69_26e0),
    Some(0x69_2a88),
    Some(0x69_2b8c),
    Some(0x69_2ce8),
    Some(0x69_2e14),
    Some(0x69_322c),
    Some(0x69_3390),
    Some(0x69_34dc),
    Some(0x69_3aec),
];

/// The register snapshot the retained pipeline passes at 0xda21cc and 0xda2354 (0x14c6b24).
const SNAPSHOT: u32 = 0x14c_6b24;
const RESTORE: u32 = 0x6d_eb08;

/// A configuration the retained pipeline hands the register layer (0xda21cc, 0xda2354).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StockConfiguration(u32);

impl StockConfiguration {
    /// Wraps the pointer the retained pipeline passes at 0xda21cc.
    pub const fn from_pipeline(address: u32) -> Self { Self(address) }
}

impl<M: Machine> RegisterSnapshot for Stock<M> {
    type Configuration = StockConfiguration;

    fn restore(&mut self, group: RegisterGroup) {
        let (descriptors, count) = RESTORE_DESCRIPTORS[group.index()];
        self.machine.enter(RESTORE, &[SNAPSHOT as usize, descriptors as usize, count as usize]);
    }

    fn window_limits(&self, configuration: StockConfiguration) -> WindowLimits {
        WindowLimits([0x3e, 0x40, 0x42].map(|offset| self.machine.read_u16(configuration.0 + offset) as i16))
    }

    fn window_words(&self) -> [u32; 2] {
        [self.machine.read_u32(SNAPSHOT + 0x3c), self.machine.read_u32(SNAPSHOT + 0x40)]
    }

    fn set_window_words(&mut self, [low, high]: [u32; 2]) {
        self.machine.write_u32(SNAPSHOT + 0x3c, low);
        self.machine.write_u32(SNAPSHOT + 0x40, high);
    }

    fn pack(&mut self, group: RegisterGroup, configuration: StockConfiguration) {
        match PACKERS[group.index()] {
            Some(packer) => {
                self.machine.enter(packer, &[SNAPSHOT as usize, configuration.0 as usize]);
            }
            None => {
                let words = pack_window_limits(self.window_limits(configuration), self.window_words());
                self.set_window_words(words);
            }
        }
    }
}

/// The client callback word and request number lenscom's completion 0x6b6f9c reads back.
const LENS_COM_CLIENT: u32 = 0x8e_1f3c;
const LENS_COM_COMPLETION: StockCallbackWord = StockCallbackWord(0x6b_6f9c);
const LENS_COM_UNPOSTED: u32 = 0x6b_92cc;

impl<M: Machine> LensComClient for Stock<M> {
    fn remember(&mut self, callback: Option<StockCallbackWord>, number: RequestNumber) {
        self.machine.write_u32(LENS_COM_CLIENT + 0x8, callback.map_or(0, |callback| callback.0));
        self.machine.write_u32(LENS_COM_CLIENT + 0xc, u32::from(number.get()));
    }

    fn completion(&self) -> StockCallbackWord { LENS_COM_COMPLETION }

    fn complete(&mut self, callback: StockCallbackWord, number: RequestNumber) {
        self.machine.enter(callback.0, &[u32::from(number.get()) as usize]);
    }

    /// Appends lenscom's failure text with codes 1, 0x27, 0 (0x6bbee8..0x6bbef4).
    fn report_unposted(&mut self) { self.machine.enter(LENS_COM_UNPOSTED, &[1, 0x27, 0]); }
}

const DISPLAY_ROUTE: u32 = 0x6c_68c0;
const DISPLAY_STATUS: u32 = 0x6c_4bec;
const STATUS_BYTES: usize = 60;
const STATUS_GATE: usize = 7;
const STATUS_OUTPUTS: core::ops::Range<usize> = 16..20;

impl<M: Machine> Stock<M> {
    /// The display driver's 60-byte status record (0x6c4c08).
    fn status(&self) -> [u8; STATUS_BYTES] {
        let mut status = [0u8; STATUS_BYTES];
        self.machine.enter(DISPLAY_STATUS, &[status.as_mut_ptr() as usize]);
        status
    }
}

impl<M: Machine> Display for Stock<M> {
    fn route(&self) -> DisplayRoute {
        let mut route = 0u8;
        self.machine.enter(DISPLAY_ROUTE, &[&mut route as *mut u8 as usize]);
        DisplayRoute(route)
    }

    fn gate_open(&self) -> bool { self.status()[STATUS_GATE] != 0 }

    /// Bit k is set when status byte 16 + k is nonzero (0x6c693c..0x6c6960).
    fn outputs(&self) -> Outputs {
        let status = self.status();
        Outputs(status[STATUS_OUTPUTS].iter().enumerate().fold(0, |bits, (bit, byte)| {
            if *byte != 0 { bits | 1 << bit } else { bits }
        }))
    }
}

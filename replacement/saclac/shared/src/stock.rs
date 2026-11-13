//! The stock boundary: X-M1 1.50 addresses of the memory and entries the shared layer uses.

use core::ptr::{read_volatile, write_volatile};

use xm1_saclac_interface::stock::{callback_word, command_code, StockCallbackWord, PARAMETER_BLOCKS};
use xm1_saclac_interface::{ClientCallback, CommandKind};

use crate::commands::{ClientArguments, Clients, CommandMemory, StoredCommand};
use crate::display::{Display, DisplayRoute, DisplayStatus, STATUS_BYTES};
use crate::image_registers::{RegisterGroup, RegisterSnapshot, WindowLimits};
use crate::lens::{LensBounds, LensControl, LensNotices, LensProbe, Notice, ProbeState, Rendezvous};
use crate::lens_com::{LensComClient, RequestNumber};
use crate::settings::{AdjustmentKey, Adjustments, Aspect, CaptureMode, ChannelRemap, Channels, Conversions, WindowVariant};

fn read_u16(address: u32) -> u16 { unsafe { read_volatile(address as usize as *const u16) } }

fn read_u32(address: u32) -> u32 { unsafe { read_volatile(address as usize as *const u32) } }

fn write_u8(address: u32, value: u8) { unsafe { write_volatile(address as usize as *mut u8, value) } }

fn write_u32(address: u32, value: u32) { unsafe { write_volatile(address as usize as *mut u32, value) } }

macro_rules! entry {
    ($address:expr, fn($($argument:ty),*) $(-> $result:ty)?) => {{
        let entry: extern "C" fn($($argument),*) $(-> $result)? = unsafe { core::mem::transmute($address as usize as *const ()) };
        entry
    }};
}

/// One pending byte per command code (0xef9aec).
const PENDING_FLAGS: u32 = 0x169_cf24;

pub struct StockCommands;

impl CommandMemory for StockCommands {
    fn word(&self, command: StoredCommand, index: usize) -> u32 {
        let block = PARAMETER_BLOCKS[usize::from(command_code(command.kind()))].address;
        read_u32(block + 4 * index as u32)
    }

    fn set_pending(&mut self, command: CommandKind, pending: bool) {
        write_u8(PENDING_FLAGS + u32::from(command_code(command)), u8::from(pending));
    }
}

pub struct StockClients;

impl Clients for StockClients {
    fn enter(&mut self, callback: ClientCallback, arguments: ClientArguments) {
        let word = callback_word(callback);
        match arguments {
            ClientArguments::Status(status) => entry!(word, fn(i32))(status.0),
            ClientArguments::Reply { status, words: [first, second, third, fourth] } => {
                entry!(word, fn(i32, u32, u32, u32, u32))(status.0, first, second, third, fourth)
            }
        }
    }
}

const LENS_RUNTIME: u32 = 0x94_2ab0;
const LENS_STEP: u32 = LENS_RUNTIME + 0x98;

pub struct StockLensControl;

impl LensControl for StockLensControl {
    fn probe(&mut self, probe: LensProbe) -> ProbeState {
        let address: u32 = match probe {
            LensProbe::First => 0x76_d9a8,
            LensProbe::Second => 0x76_da38,
            LensProbe::Third => 0x76_dba4,
        };
        match entry!(address, fn() -> u32)() {
            2 => ProbeState::Overridden,
            1 => ProbeState::Reported,
            _ => ProbeState::Clear,
        }
    }

    fn value(&mut self) -> u32 { entry!(0x76_da48u32, fn() -> u32)() }

    fn step(&mut self) -> u32 { read_u32(LENS_STEP) }

    /// 0x76dac4 forwards to 0x76b8b4.
    fn prefers_relative(&mut self) -> bool { entry!(0x76_b8b4u32, fn() -> u32)() != 0 }

    fn zone_limit(&mut self, zone: u8) -> i16 { entry!(0x77_2f28u32, fn(u32) -> i32)(u32::from(zone)) as i16 }
}

/// The lens control block other tasks write the notices into.
const LENS_NOTICES: u32 = 0x95_4dbc;

pub struct StockLensNotices;

impl LensNotices for StockLensNotices {
    fn notice(&self, notice: Notice) -> u32 {
        let offset = match notice {
            Notice::First => 0x4,
            Notice::Second => 0x8,
            Notice::Third => 0xc,
        };
        read_u32(LENS_NOTICES + offset)
    }

    fn clear_notices(&mut self) {
        for offset in [0x4, 0x8, 0xc] {
            write_u32(LENS_NOTICES + offset, 0);
        }
    }

    fn notifier_enabled(&self) -> bool { read_u32(LENS_NOTICES + 0x20) != 0 }

    fn notifier(&self) -> Option<StockCallbackWord> {
        Some(read_u32(LENS_NOTICES + 0x1c)).filter(|word| *word != 0).map(StockCallbackWord)
    }

    /// The notifier takes the address of the detail word followed by the bounds (0xefba28..0xefba3c).
    fn enter_notifier(&mut self, notifier: StockCallbackWord, detail: u32, bounds: LensBounds) {
        let [first, second, third] = bounds.0;
        let record = [detail, first, second, third];
        entry!(notifier.0, fn(*const [u32; 4]))(&record);
    }
}

/// The task a lens synchronization releases (0xf472d4) and the callback lensdc enters, which
/// sets event 2 on it (0xf46be0).
const SYNCHRONIZING_TASK: u32 = 0x16c_2214;
const SYNCHRONIZED: StockCallbackWord = StockCallbackWord(0xf4_6be0);

pub struct StockRendezvous;

impl Rendezvous for StockRendezvous {
    fn arm(&mut self) -> StockCallbackWord {
        write_u32(SYNCHRONIZING_TASK, entry!(0x72_3cb8u32, fn() -> u32)());
        SYNCHRONIZED
    }

    fn wait(&mut self) {
        let mut flags = 0u32;
        entry!(0x72_43a8u32, fn(u32, *mut u32, u32) -> i32)(2, &mut flags, u32::MAX);
    }
}

pub struct StockAdjustments;

impl Adjustments for StockAdjustments {
    fn byte(&self, key: AdjustmentKey) -> u8 { entry!(0x69_f8fcu32, fn(u32) -> u32)(key.0) as u8 }
}

pub struct StockChannels;

impl Channels for StockChannels {
    fn is_valid(&self, channel: i32) -> bool { entry!(0x74_a214u32, fn(i32) -> u32)(channel) != 0 }
}

pub struct StockChannelRemap;

impl ChannelRemap for StockChannelRemap {
    fn remap(&mut self, [a, b, c, d, e, f, g]: [u32; 7]) -> u32 {
        entry!(0x74_947cu32, fn(u32, u32, u32, u32, u32, u32, u32) -> u32)(a, b, c, d, e, f, g)
    }
}

const fn aspect_index(aspect: Aspect) -> u32 {
    match aspect {
        Aspect::ThreeFour => 0,
        Aspect::TwoThree => 1,
        Aspect::NineSixteen => 2,
        Aspect::Square => 3,
    }
}

pub struct StockConversions;

impl Conversions for StockConversions {
    fn window_variant(&mut self, mode: CaptureMode, aspect: Aspect) -> Option<WindowVariant> {
        let variant = entry!(0x73_958cu32, fn(u32, u32) -> u32)(u32::from(mode.get()), aspect_index(aspect));
        (variant != 0xff).then_some(WindowVariant(variant as u8))
    }

    fn value(&mut self, variant: WindowVariant, aspect: Aspect) -> u32 {
        entry!(0x73_4540u32, fn(u32, u32) -> u32)(u32::from(variant.0), aspect_index(aspect))
    }

    fn alternate(&mut self, variant: WindowVariant, aspect: Aspect) -> u32 {
        entry!(0x73_469cu32, fn(u32, u32) -> u32)(u32::from(variant.0), aspect_index(aspect))
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

/// The address of a configuration the pipeline packs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StockConfiguration(pub u32);

/// The pipeline's register snapshot at `snapshot` (0x14c6b24 for 0xda21cc and 0xda2354).
pub struct StockRegisterSnapshot {
    pub snapshot: u32,
}

impl RegisterSnapshot for StockRegisterSnapshot {
    type Configuration = StockConfiguration;

    fn restore(&mut self, group: RegisterGroup) {
        let (descriptors, count) = RESTORE_DESCRIPTORS[group.index()];
        entry!(0x6d_eb08u32, fn(u32, u32, u32))(self.snapshot, descriptors, count);
    }

    fn window_limits(&self, configuration: StockConfiguration) -> WindowLimits {
        WindowLimits([0x3e, 0x40, 0x42].map(|offset| read_u16(configuration.0 + offset) as i16))
    }

    fn window_words(&self) -> [u32; 2] { [read_u32(self.snapshot + 0x3c), read_u32(self.snapshot + 0x40)] }

    fn set_window_words(&mut self, [low, high]: [u32; 2]) {
        write_u32(self.snapshot + 0x3c, low);
        write_u32(self.snapshot + 0x40, high);
    }

    fn pack_retained(&mut self, group: RegisterGroup, configuration: StockConfiguration) {
        if let Some(packer) = PACKERS[group.index()] {
            entry!(packer, fn(u32, u32))(self.snapshot, configuration.0);
        }
    }
}

/// The client callback word and request number lenscom's completion 0x6b6f9c reads back.
const LENS_COM_CLIENT: u32 = 0x8e_1f3c;
const LENS_COM_COMPLETION: StockCallbackWord = StockCallbackWord(0x6b_6f9c);

pub struct StockLensComClient;

impl LensComClient for StockLensComClient {
    fn remember(&mut self, callback: Option<StockCallbackWord>, number: RequestNumber) {
        write_u32(LENS_COM_CLIENT + 0x8, callback.map_or(0, |callback| callback.0));
        write_u32(LENS_COM_CLIENT + 0xc, u32::from(number.get()));
    }

    fn completion(&self) -> StockCallbackWord { LENS_COM_COMPLETION }

    fn complete(&mut self, callback: StockCallbackWord, number: RequestNumber) { entry!(callback.0, fn(u32))(u32::from(number.get())) }

    /// Appends lenscom's failure text with codes 1, 0x27, 0 (0x6bbee8..0x6bbef4).
    fn report_unposted(&mut self) { entry!(0x6b_92ccu32, fn(u32, u32, u32))(1, 0x27, 0) }
}

pub struct StockDisplay;

impl Display for StockDisplay {
    fn route(&self) -> DisplayRoute {
        let mut route = 0u8;
        entry!(0x6c_68c0u32, fn(*mut u8) -> u32)(&mut route);
        DisplayRoute(route)
    }

    fn status(&self) -> DisplayStatus {
        let mut status = [0u8; STATUS_BYTES];
        entry!(0x6c_4becu32, fn(*mut [u8; STATUS_BYTES]) -> u32)(&mut status);
        DisplayStatus(status)
    }
}

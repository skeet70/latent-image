//! X-M1 1.50 firmware facts the driver reads from guest RAM: the retained menu
//! node and the calibrated ADC key table.

use crate::run::{Address, NodeName};
use fujimod_emulator::session::Session;
use fujimod_emulator_devices::parts::pins::Pin;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RouteKey {
    Up,
    Down,
    Left,
    Right,
    Ok,
    Back,
    Q,
}

impl RouteKey {
    /// Probe key names; MENU and OK share one key, as do DISP and BACK.
    pub fn parse(name: &str) -> Result<Self, String> {
        Ok(match name {
            "up" => Self::Up,
            "down" => Self::Down,
            "left" => Self::Left,
            "right" => Self::Right,
            "ok" | "menu" => Self::Ok,
            "back" | "disp" => Self::Back,
            "q" => Self::Q,
            _ => return Err(format!("unknown X-M1 key {name}")),
        })
    }
}

/// One sub-command dial step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DialTurn {
    Left,
    Right,
}

impl DialTurn {
    /// GPIO levels of (pin 8, pin 9) the step drives in order, ending released.
    pub fn phases(self) -> [(bool, bool); 4] {
        match self {
            DialTurn::Left => [(true, false), (false, false), (false, true), (true, true)],
            DialTurn::Right => [(false, true), (false, false), (true, false), (true, true)],
        }
    }
}

/// A full press drives S2 without holding S1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutterStage {
    Half,
    Full,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PhysicalShutterPosition {
    Released,
    Half,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    Key(RouteKey),
    Dial(DialTurn),
    Shutter(ShutterStage),
    Play,
}

impl Press {
    pub fn parse(name: &str) -> Result<Self, String> {
        Ok(match name {
            "dial-left" => Press::Dial(DialTurn::Left),
            "dial-right" => Press::Dial(DialTurn::Right),
            "half-press" => Press::Shutter(ShutterStage::Half),
            "full-press" => Press::Shutter(ShutterStage::Full),
            "play" => Press::Play,
            _ => Press::Key(RouteKey::parse(name)?),
        })
    }
}

/// The sub-command dial scanner over two GPIO pads.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subcommand {
    /// Descriptor {1, port<<16|pin, port<<16|pin, 0}.
    pub descriptor: u64,
    /// Codes indexed by current*4 + previous sampled phase.
    pub transitions: u64,
    /// Word that reads 1 once the scanner is registered.
    pub registered: u64,
    /// Byte holding the last sampled phase, pin 8 level << 1 | pin 9 level.
    pub sampled: u64,
    pub port: u8,
    pub pins: [u8; 2],
    pub left_code: u32,
    pub right_code: u32,
}

/// One analog key row of the firmware's control table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdcKey {
    pub key: RouteKey,
    pub code: u32,
    pub row: u64,
    pub channel: usize,
    /// Calibration-block offsets of the pressed interval's bounds.
    pub low_tag: u16,
    pub high_tag: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyTable {
    pub base: Address,
    pub row_bytes: u64,
    /// Row byte that reads zero once the firmware has debounced a release.
    pub released_offset: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FirmwareFacts {
    /// Current-node record: descriptor pointer at +0, cursor at +8.
    pub current_menu: Address,
    /// Word holding the loaded calibration block's address.
    pub calibration_pointer: Address,
    /// Word that reads 1 once the key scanner is registered.
    pub scanner_registered: Address,
    pub key_table: KeyTable,
    pub released_sample: u16,
    pub keys: Vec<AdcKey>,
    pub mode_dial: ModeDial,
    pub dial_push: GpioKey,
    pub play: GpioBinding,
    pub shutter: Shutter,
    pub subcommand: Subcommand,
    pub render: crate::render::RenderFacts,
}

/// Control-table kind 2.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpioKey {
    pub code: u32,
    pub row: u64,
    pub port: u8,
    pub pin: u8,
    pub pressed_high: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(try_from = "u64")]
pub struct TableRow(u64);

const KEY_ROWS: u64 = 36;

impl TableRow {
    pub fn get(self) -> u64 {
        self.0
    }
}

impl TryFrom<u64> for TableRow {
    type Error = String;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        if value < KEY_ROWS {
            Ok(Self(value))
        } else {
            Err(format!("control-table row {value} is outside 0..{KEY_ROWS}"))
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub enum GpioCode {
    #[serde(rename = "s1")]
    S1,
    #[serde(rename = "s2")]
    S2,
    #[serde(rename = "play")]
    Play,
}

impl GpioCode {
    fn value(self) -> u32 {
        match self {
            Self::S1 => 7,
            Self::S2 => 8,
            Self::Play => 11,
        }
    }
}

/// A class-4 row's upper halfword.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct InterruptIndex(u16);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpioBinding {
    pub code: GpioCode,
    pub row: TableRow,
    pub pad: Pin,
    pub pressed_high: bool,
}

impl GpioBinding {
    fn gpio(&self) -> Result<GpioKey, String> {
        self.pad.validate()?;
        Ok(GpioKey {
            code: self.code.value(),
            row: self.row.get(),
            port: self.pad.port,
            pin: self.pad.pin,
            pressed_high: self.pressed_high,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShutterBinding {
    pub code: GpioCode,
    pub row: TableRow,
    pub pad: Pin,
    pub pressed_high: bool,
    pub interrupt_index: InterruptIndex,
}

impl ShutterBinding {
    fn gpio(&self) -> Result<GpioKey, String> {
        self.pad.validate()?;
        Ok(GpioKey {
            code: self.code.value(),
            row: self.row.get(),
            port: self.pad.port,
            pin: self.pad.pin,
            pressed_high: self.pressed_high,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shutter {
    pub half: ShutterBinding,
    pub full: ShutterBinding,
}

/// Class-4 lower halfword of both shutter rows.
const SHUTTER_RECORD_LOWER: u16 = 1;

impl GpioKey {
    fn checked(&self, session: &Session, table: &KeyTable) -> Result<GpioKey, String> {
        let record = bytes::<28>(session, table.base.0 + self.row * table.row_bytes)?;
        let word = |offset| u32::from_le_bytes([record[offset], record[offset + 1], record[offset + 2], record[offset + 3]]);
        let halfword = |offset| u16::from_le_bytes([record[offset], record[offset + 1]]);
        if word(0) != self.code
            || record[9] != 2
            || word(12) != u32::from(self.port) << 16 | u32::from(self.pin)
            || halfword(0x12) != u16::from(self.pressed_high)
        {
            return Err(format!("GPIO key row {} differs from its declared binding", self.row));
        }
        Ok(*self)
    }
}

/// Mode-dial detents in the bridge protocol's names.
#[derive(Clone, Copy, Debug, Deserialize, serde::Serialize, PartialEq, Eq)]
pub enum ModeDialPosition {
    S,
    P,
    #[serde(rename = "SR+")]
    SrAuto,
    #[serde(rename = "AUTO")]
    Auto,
    #[serde(rename = "Adv.")]
    Advanced,
    #[serde(rename = "SP")]
    Scene,
    Sports,
    Landscape,
    Portrait,
    C,
    M,
    A,
}

impl ModeDialPosition {
    pub fn parse(name: &str) -> Result<Self, String> {
        serde_json::from_value(serde_json::Value::String(name.to_owned()))
            .map_err(|_| format!("unknown mode-dial position {name}"))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeDialRow {
    pub position: ModeDialPosition,
    pub code: u32,
    pub row: u64,
    pub low_tag: u16,
    pub high_tag: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeDial {
    pub channel: usize,
    /// Positions the session runs past; the others reach unmodelled devices.
    pub running: Vec<ModeDialPosition>,
    pub rows: Vec<ModeDialRow>,
}

/// A position's loaded calibrated interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeDialWindow {
    pub position: ModeDialPosition,
    pub low: u16,
    pub high: u16,
}

impl ModeDialWindow {
    pub fn midpoint(&self) -> u16 {
        (self.low + self.high) / 2
    }
}

/// The firmware's retained current menu node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuState {
    pub menu_id: u32,
    pub setting_id: u32,
    pub cursor: u32,
}

fn bytes<const N: usize>(session: &Session, address: u64) -> Result<[u8; N], String> {
    let mut value = [0; N];
    session.board.read_physical(address, &mut value)?;
    Ok(value)
}

fn word(session: &Session, address: u64) -> Result<u32, String> {
    bytes::<4>(session, address).map(u32::from_le_bytes)
}

fn half(session: &Session, address: u64) -> Result<u16, String> {
    bytes::<2>(session, address).map(u16::from_le_bytes)
}

impl FirmwareFacts {
    pub fn menu(&self, session: &Session) -> Result<Option<MenuState>, String> {
        let pointer = word(session, self.current_menu.0)?;
        if pointer == 0 {
            return Ok(None);
        }
        Ok(Some(MenuState {
            menu_id: word(session, u64::from(pointer))?,
            setting_id: word(session, u64::from(pointer) + 8)?,
            cursor: word(session, self.current_menu.0 + 8)?,
        }))
    }

    pub fn binding(&self, key: RouteKey) -> Result<&AdcKey, String> {
        self.keys.iter().find(|binding| binding.key == key).ok_or_else(|| format!("{key:?} has no ADC row"))
    }

    fn calibration(&self, session: &Session) -> Result<u64, String> {
        let base = u64::from(word(session, self.calibration_pointer.0)?);
        if base == 0 {
            return Err("X-M1 calibration is not loaded".into());
        }
        Ok(base)
    }

    /// Loaded windows in row order; the firmware keeps the first row that contains a sample.
    fn mode_dial_windows(&self, session: &Session) -> Result<Vec<ModeDialWindow>, String> {
        let base = self.calibration(session)?;
        self.mode_dial
            .rows
            .iter()
            .map(|row| {
                let record = bytes::<28>(session, self.key_table.base.0 + row.row * self.key_table.row_bytes)?;
                let field = |at: usize| u32::from_le_bytes([record[at], record[at + 1], record[at + 2], record[at + 3]]);
                if field(0) != row.code
                    || record[8] != 1
                    || record[9] != 1
                    || usize::try_from(field(12)).ok() != Some(self.mode_dial.channel)
                    || u16::from_le_bytes([record[16], record[17]]) != row.low_tag
                    || u16::from_le_bytes([record[18], record[19]]) != row.high_tag
                {
                    return Err(format!("mode-dial row {} differs from its declared binding", row.row));
                }
                let low = half(session, base + u64::from(row.low_tag))?;
                let high = half(session, base + u64::from(row.high_tag))?;
                if low > high || high > self.released_sample {
                    return Err(format!("mode-dial row {} has no window in the 10-bit range", row.row));
                }
                Ok(ModeDialWindow { position: row.position, low, high })
            })
            .collect()
    }

    pub fn mode_dial_window(&self, session: &Session, position: ModeDialPosition) -> Result<ModeDialWindow, String> {
        if !self.mode_dial.running.contains(&position) {
            return Err(format!("{position:?} stops the emulator at an unmodelled device"));
        }
        let windows = self.mode_dial_windows(session)?;
        let window = *windows
            .iter()
            .find(|window| window.position == position)
            .ok_or_else(|| format!("{position:?} has no mode-dial row"))?;
        let first = windows.iter().find(|candidate| (candidate.low..=candidate.high).contains(&window.midpoint()));
        if first.map(|candidate| candidate.position) != Some(position) {
            return Err(format!("{position:?} midpoint selects an earlier row"));
        }
        Ok(window)
    }

    /// The loaded pressed interval of one key, checked against its table row.
    fn interval(&self, session: &Session, binding: &AdcKey) -> Result<(u16, u16), String> {
        let record = bytes::<28>(session, self.key_table.base.0 + binding.row * self.key_table.row_bytes)?;
        let code = u32::from_le_bytes(record[0..4].try_into().map_err(|_| "key row")?);
        let channel = u32::from_le_bytes(record[12..16].try_into().map_err(|_| "key row")?);
        let low_tag = u16::from_le_bytes([record[16], record[17]]);
        let high_tag = u16::from_le_bytes([record[18], record[19]]);
        if code != binding.code
            || record[9] != 1
            || usize::try_from(channel).ok() != Some(binding.channel)
            || low_tag != binding.low_tag
            || high_tag != binding.high_tag
        {
            return Err(format!("key row {} differs from its declared ADC binding", binding.row));
        }
        let base = u64::from(word(session, self.calibration_pointer.0)?);
        if base == 0 {
            return Err("X-M1 calibration is not loaded".into());
        }
        let low = half(session, base + u64::from(low_tag))?;
        let high = half(session, base + u64::from(high_tag))?;
        if low > high || high >= self.released_sample {
            return Err(format!("key row {} has no pressed interval below release", binding.row));
        }
        Ok((low, high))
    }

    pub fn scanner_registered(&self, session: &Session) -> Result<bool, String> {
        Ok(word(session, self.scanner_registered.0)? == 1)
    }

    /// The ADC keys whose calibrated interval holds their channel's sample in
    /// `samples`; none before the scanner registers.
    pub fn asserted_keys(&self, session: &Session, samples: &[u16]) -> Result<Vec<RouteKey>, String> {
        if !self.scanner_registered(session)? {
            return Ok(Vec::new());
        }
        let mut asserted = Vec::new();
        for binding in &self.keys {
            let (low, high) = self.interval(session, binding)?;
            let sample = *samples.get(binding.channel).ok_or("ADC channel is absent")?;
            if (low..=high).contains(&sample) {
                asserted.push(binding.key);
            }
        }
        Ok(asserted)
    }

    /// The calibrated midpoint of `key`, refused while the scanner is not
    /// registered or when it also falls in another key's interval.
    pub fn pressed_sample(&self, session: &Session, key: RouteKey) -> Result<(usize, u16), String> {
        if !self.scanner_registered(session)? {
            return Err("X-M1 input scanner is not registered".into());
        }
        let binding = self.binding(key)?;
        let (low, high) = self.interval(session, binding)?;
        let midpoint = (low + high) / 2;
        for other in self.keys.iter().filter(|other| other.channel == binding.channel && other.key != key) {
            let (other_low, other_high) = self.interval(session, other)?;
            if (other_low..=other_high).contains(&midpoint) {
                return Err(format!("{key:?} midpoint also asserts {:?}", other.key));
            }
        }
        Ok((binding.channel, midpoint))
    }

    /// The dial-push row, checked against its declared pad and level.
    pub fn dial_push(&self, session: &Session) -> Result<GpioKey, String> {
        self.dial_push.checked(session, &self.key_table)
    }

    pub fn play(&self, session: &Session) -> Result<GpioKey, String> {
        self.play.gpio()?.checked(session, &self.key_table)
    }

    pub fn shutter(&self, session: &Session, stage: ShutterStage) -> Result<GpioKey, String> {
        let key = match stage {
            ShutterStage::Half => &self.shutter.half,
            ShutterStage::Full => &self.shutter.full,
        };
        let pad = key.gpio()?;
        let record = bytes::<28>(session, self.key_table.base.0 + key.row.get() * self.key_table.row_bytes)?;
        let word = |offset| u32::from_le_bytes([record[offset], record[offset + 1], record[offset + 2], record[offset + 3]]);
        let halfword = |offset| u16::from_le_bytes([record[offset], record[offset + 1]]);
        if word(0) != key.code.value() || record[9] != 4 || word(12) != u32::from(pad.pin)
            || halfword(16) != SHUTTER_RECORD_LOWER || halfword(18) != key.interrupt_index.0
        {
            return Err(format!("{stage:?} shutter row differs from its declared binding"));
        }
        Ok(pad)
    }

    pub fn released(&self, session: &Session, key: RouteKey) -> Result<bool, String> {
        let binding = self.binding(key)?;
        let address = self.key_table.base.0 + binding.row * self.key_table.row_bytes + self.key_table.released_offset;
        Ok(bytes::<1>(session, address)?[0] == 0)
    }

    /// Refuses a dial step unless the scanner's descriptor, transition table and registration
    /// match the declared pads and codes.
    pub fn subcommand_ready(&self, session: &Session) -> Result<(), String> {
        let dial = &self.subcommand;
        let pad = |pin: u8| u32::from(dial.port) << 16 | u32::from(pin);
        let descriptor = bytes::<16>(session, dial.descriptor)?;
        let field = |at: usize| u32::from_le_bytes([descriptor[at], descriptor[at + 1], descriptor[at + 2], descriptor[at + 3]]);
        if [field(0), field(4), field(8), field(12)] != [1, pad(dial.pins[0]), pad(dial.pins[1]), 0] {
            return Err("sub-command GPIO descriptor differs from its declared pads".into());
        }
        let table = bytes::<64>(session, dial.transitions)?;
        let codes: Vec<u32> = table.chunks(4).map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]])).collect();
        let mut expected = vec![0; 16];
        expected[1] = dial.right_code;
        expected[4] = dial.left_code;
        if codes != expected {
            return Err("sub-command transition table differs from its declared codes".into());
        }
        if word(session, dial.registered)? != 1 {
            return Err("sub-command scanner is not registered".into());
        }
        Ok(())
    }

    pub fn subcommand_sampled(&self, session: &Session) -> Result<u8, String> {
        Ok(bytes::<1>(session, self.subcommand.sampled)?[0])
    }
}

pub fn set_adc(session: &mut Session, adc: &NodeName, channel: usize, sample: u16) -> Result<(), String> {
    let mut samples = session.adc_samples(&adc.0)?;
    *samples.get_mut(channel).ok_or("ADC channel is absent")? = sample;
    session.set_adc_samples(&adc.0, &samples)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_map_to_typed_inputs_and_unknown_ones_refuse() {
        assert_eq!(Press::parse("play").unwrap(), Press::Play);
        assert_eq!(Press::parse("full-press").unwrap(), Press::Shutter(ShutterStage::Full));
        assert_eq!(Press::parse("half-press").unwrap(), Press::Shutter(ShutterStage::Half));
        assert_eq!(Press::parse("menu").unwrap(), Press::Key(RouteKey::Ok));
        assert_eq!(Press::parse("disp").unwrap(), Press::Key(RouteKey::Back));
        assert!(Press::parse("power").is_err());
        assert!(Press::parse("playback").is_err());
        assert!(Press::parse("full_press").is_err());
    }
}

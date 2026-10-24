//! X-M1 1.50 firmware facts the driver reads from guest RAM: the retained menu
//! node and the calibrated ADC key table.

use crate::run::{Address, NodeName};
use fujimod_emulator::session::Session;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
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

    fn binding(&self, key: RouteKey) -> Result<&AdcKey, String> {
        self.keys.iter().find(|binding| binding.key == key).ok_or_else(|| format!("{key:?} has no ADC row"))
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

    /// The calibrated midpoint of `key`, refused while the scanner is not
    /// registered or when it also falls in another key's interval.
    pub fn pressed_sample(&self, session: &Session, key: RouteKey) -> Result<(usize, u16), String> {
        if word(session, self.scanner_registered.0)? != 1 {
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

    pub fn released(&self, session: &Session, key: RouteKey) -> Result<bool, String> {
        let binding = self.binding(key)?;
        let address = self.key_table.base.0 + binding.row * self.key_table.row_bytes + self.key_table.released_offset;
        Ok(bytes::<1>(session, address)?[0] == 0)
    }
}

pub fn set_adc(session: &mut Session, adc: &NodeName, channel: usize, sample: u16) -> Result<(), String> {
    let mut samples = session.adc_samples(&adc.0)?;
    *samples.get_mut(channel).ok_or("ADC channel is absent")? = sample;
    session.set_adc_samples(&adc.0, &samples)
}

use super::{CountryCode, RateBand, RateGroup};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TableIndex(u8);
impl TableIndex {
    pub const fn parse(value: u8) -> Option<Self> { if value < 15 { Some(Self(value)) } else { None } }
    pub const fn wire(self) -> u8 { self.0 }
}

#[derive(Clone, Copy)]
enum Selection { Country, Indexed(TableIndex), Custom }
#[derive(Clone, Copy)]
enum SelectedTable { Indexed(TableIndex), Custom }

pub struct RegulatoryInput {
    selection: Selection,
    custom: [RateGroup; 6],
    flags: [u8; 2],
}
impl RegulatoryInput {
    pub const fn country(flags: [u8; 2]) -> Self {
        Self { selection: Selection::Country, custom: [EMPTY; 6], flags }
    }
    pub fn from_eeprom(mode: u32, index: u8, custom_groups: [[u8; 6]; 6], flags: [u8; 2]) -> Self {
        let selection = match (mode, TableIndex::parse(index)) {
            (1, Some(index)) => Selection::Indexed(index),
            (2, _) => Selection::Custom,
            _ => Selection::Country,
        };
        Self {
            selection,
            custom: custom_groups.map(|bytes| RateGroup {
                index: bytes[0], band: RateBand::from_wire(bytes[1]),
                step: bytes[2], first: bytes[3], count: bytes[4],
            }),
            flags,
        }
    }
}

pub struct SelectedRates<'a> {
    pub groups: &'a [RateGroup; 6],
    pub flags: [u8; 2],
}

pub struct RegulatorySelector { cached: Option<SelectedTable> }
impl Default for RegulatorySelector {
    fn default() -> Self { Self::new() }
}
impl RegulatorySelector {
    pub const fn new() -> Self { Self { cached: None } }
    pub fn invalidate(&mut self) { self.cached = None; }
    pub fn select<'a>(&mut self, country: CountryCode, input: &'a RegulatoryInput) -> SelectedRates<'a> {
        let selection = *self.cached.get_or_insert_with(|| match input.selection {
            Selection::Country => SelectedTable::Indexed(country_table(country)),
            Selection::Indexed(index) => SelectedTable::Indexed(index),
            Selection::Custom => SelectedTable::Custom,
        });
        let groups = match selection {
            SelectedTable::Indexed(index) => &TABLES[index.0 as usize].groups,
            SelectedTable::Custom => &input.custom,
        };
        SelectedRates { groups, flags: input.flags }
    }
}

fn country_table(country: CountryCode) -> TableIndex {
    TableIndex(TABLES.iter().position(|table| table.countries.iter().any(|candidate| *candidate == country.0)).unwrap_or(14) as u8)
}

struct CountryTable { countries: &'static [u16], groups: [RateGroup; 6] }
const fn group(index: u8, band: RateBand, step: u8, first: u8, count: u8) -> RateGroup {
    RateGroup { index, band, step, first, count }
}
const EMPTY: RateGroup = group(0, RateBand::Unused, 0, 0, 0);
const LOW_11: RateGroup = group(81, RateBand::TwoGhz, 1, 1, 11);
const LOW_13: RateGroup = group(81, RateBand::TwoGhz, 1, 1, 13);
const HIGH_36: RateGroup = group(115, RateBand::FiveGhz, 4, 36, 4);
const HIGH_52: RateGroup = group(118, RateBand::FiveGhz, 4, 52, 4);
const HIGH_100: RateGroup = group(121, RateBand::FiveGhz, 4, 100, 11);
const HIGH_149: RateGroup = group(125, RateBand::FiveGhz, 4, 149, 5);
const DISABLED_36: RateGroup = group(115, RateBand::Unused, 0, 0, 0);
const DISABLED_52: RateGroup = group(118, RateBand::Unused, 0, 0, 0);
const DISABLED_100: RateGroup = group(121, RateBand::Unused, 0, 0, 0);
const DISABLED_149: RateGroup = group(125, RateBand::Unused, 0, 0, 0);

// The table order is the EEPROM selector ABI at 0x012e99e8.
const TABLES: [CountryTable; 15] = [
    CountryTable { countries: &[0x5553, 0x4253, 0x4242, 0x424f, 0x444d, 0x444f, 0x4854, 0x5052, 0x5448, 0x5457, 0x4149, 0x4147, 0x4157, 0x424d, 0x4355, 0x4744, 0x4759, 0x4a4d, 0x4b4e, 0x4c43, 0x5643, 0x5454, 0x5352], groups: [LOW_11, HIGH_36, HIGH_52, HIGH_100, HIGH_149, EMPTY] },
    CountryTable { countries: &[0x4252, 0x4543, 0x484b, 0x4d58, 0x5045, 0x4352, 0x4d44, 0x4e49, 0x505a], groups: [LOW_13, HIGH_36, HIGH_52, HIGH_100, HIGH_149, EMPTY] },
    CountryTable { countries: &[0x434f, 0x5059], groups: [LOW_13, HIGH_36, HIGH_52, HIGH_100, DISABLED_149, EMPTY] },
    CountryTable { countries: &[0x4a50], groups: [LOW_13, group(82, RateBand::TwoGhz, 1, 14, 1), HIGH_36, HIGH_52, HIGH_100, DISABLED_149] },
    CountryTable { countries: &[0x434e, 0x5559, 0x4d41], groups: [LOW_13, DISABLED_36, HIGH_52, HIGH_100, HIGH_149, EMPTY] },
    CountryTable { countries: &[0x4154], groups: [LOW_13, DISABLED_36, HIGH_52, DISABLED_100, HIGH_149, EMPTY] },
    CountryTable { countries: &[0x4155, 0x4e5a], groups: [LOW_11, HIGH_36, HIGH_52, HIGH_100, group(125, RateBand::FiveGhz, 4, 149, 4), EMPTY] },
    CountryTable { countries: &[0x5255], groups: [LOW_13, HIGH_36, HIGH_52, HIGH_100, group(125, RateBand::FiveGhz, 4, 149, 4), EMPTY] },
    CountryTable { countries: &[0x4944, 0x484e, 0x5047, 0x504b], groups: [LOW_13, DISABLED_36, DISABLED_52, DISABLED_100, group(125, RateBand::FiveGhz, 4, 149, 4), EMPTY] },
    CountryTable { countries: &[0x4341], groups: [LOW_11, HIGH_36, HIGH_52, group(121, RateBand::FiveGhz, 4, 100, 5), group(121, RateBand::FiveGhz, 4, 132, 5), HIGH_149] },
    CountryTable { countries: &[0x434c, 0x494e, 0x5341, 0x5347, 0x5645, 0x4d59, 0x564e, 0x4547], groups: [LOW_13, HIGH_36, HIGH_52, DISABLED_100, HIGH_149, EMPTY] },
    CountryTable { countries: &[0x494c, 0x5541], groups: [LOW_13, HIGH_36, HIGH_52, DISABLED_100, DISABLED_149, EMPTY] },
    CountryTable { countries: &[0x4a4f, 0x4b57], groups: [LOW_13, HIGH_36, DISABLED_52, DISABLED_100, DISABLED_149, EMPTY] },
    CountryTable { countries: &[0x4b52, 0x4f4d], groups: [LOW_13, HIGH_36, HIGH_52, group(121, RateBand::FiveGhz, 4, 100, 8), HIGH_149, EMPTY] },
    CountryTable { countries: &[0x4555], groups: [LOW_13, HIGH_36, HIGH_52, HIGH_100, group(125, RateBand::FiveGhz, 4, 149, 7), EMPTY] },
];

#[cfg(test)]
#[path = "vendor_wireless_adapter_regulatory_tests.rs"]
mod tests;

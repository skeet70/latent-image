//! The menu value transaction: the row token to settings value commit the
//! stage consumer posts after the engine accepts an event.
//!
//! The IMAGE SIZE property is a composite: its token selects a size/aspect
//! pair that is converted into the two ordinary properties. Effects the caller
//! still owns are returned as named retained effects. Tables come from the
//! pinned image-4 bytes via `//replacement/modal_runtime:menu-value-table`;
//! this module names no stock address.

#![allow(unexpected_cfgs, unused_attributes)]
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), allow(dead_code))]

use core::fmt;

/// The IMAGE SIZE composite writes its size and aspect properties and captures
/// the retained properties on the entry.
pub const MAX_WRITES: usize = 5;

/// The IMAGE SIZE composite's retained effects.
pub const MAX_RETAINED: usize = 5;

macro_rules! word {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
        pub struct $name(u32);

        impl $name {
            pub const fn new(raw: u32) -> Self {
                Self(raw)
            }

            pub const fn get(self) -> u32 {
                self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, concat!(stringify!($name), "({:#x})"), self.0)
            }
        }
    };
}

word!(
    /// The values-array row word the stage consumer posts.
    RowToken
);
word!(
    /// A conversion table's stored column.
    ChoiceCode
);
word!(
    /// The pair table's size dimension.
    SizeCode
);
word!(
    /// The pair table's aspect dimension.
    AspectCode
);

/// A settings property the setter writes.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct PropertyId(u16);

impl PropertyId {
    /// The largest property `0x631a00` accepts.
    pub const CONVERTER_BOUND: Self = Self::new(0x286);

    pub const fn new(raw: u16) -> Self {
        Self(raw)
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

impl fmt::Debug for PropertyId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "PropertyId({:#x})", self.0)
    }
}

/// One row of a property's conversion table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConversionRow {
    pub stored: ChoiceCode,
    pub column1: ChoiceCode,
    pub effective: ChoiceCode,
}

/// The column a conversion matches on and the column it returns; the
/// discriminants are the stock `0x631a00` arguments.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConversionMode {
    Ordinal = 0,
    Stored = 1,
    Column1 = 2,
    Effective = 3,
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Column {
    Ordinal = 0,
    Stored = 1,
    Column1 = 2,
    Effective = 3,
}

/// One IMAGE SIZE row's pair.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImageSizePair {
    pub size: SizeCode,
    pub aspect: AspectCode,
}

/// The two ordinary properties the IMAGE SIZE composite writes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImageSizeRule {
    pub size_property: PropertyId,
    pub aspect_property: PropertyId,
    pub change_property: PropertyId,
    pub change_property_1f: PropertyId,
    pub marker_property: PropertyId,
}

/// The source-bound conversion and pair tables.
#[derive(Clone, Copy)]
pub struct ValueTables {
    conversions: &'static [(PropertyId, &'static [ConversionRow])],
    image_sizes: &'static [(RowToken, ImageSizePair)],
    image_size: Option<ImageSizeRule>,
}

impl ValueTables {
    pub const fn new(
        conversions: &'static [(PropertyId, &'static [ConversionRow])],
        image_sizes: &'static [(RowToken, ImageSizePair)],
        image_size: Option<ImageSizeRule>,
    ) -> Self {
        Self { conversions, image_sizes, image_size }
    }

    /// The conversion table of `property`; `None` for a null or out-of-bound
    /// pointer.
    pub fn conversion(&self, property: PropertyId) -> Option<&'static [ConversionRow]> {
        if property > PropertyId::CONVERTER_BOUND {
            return None;
        }
        self.conversions
            .binary_search_by_key(&property, |(candidate, _)| *candidate)
            .ok()
            .map(|index| self.conversions[index].1)
    }

    pub fn image_size_pair(&self, token: RowToken) -> Option<ImageSizePair> {
        self.image_sizes
            .binary_search_by_key(&token, |(candidate, _)| *candidate)
            .ok()
            .map(|index| self.image_sizes[index].1)
    }

    pub fn image_size_rule(&self) -> Option<ImageSizeRule> {
        self.image_size
    }

    /// The transaction a posted `(property, token)` pair selects.
    pub fn target(&self, property: PropertyId, token: RowToken) -> ValueTarget {
        match self.image_size {
            Some(rule) if rule.size_property == property => ValueTarget::ImageSize { token },
            _ => ValueTarget::Ordinary { property, token },
        }
    }
}

/// The properties a transaction can change.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueTarget {
    Ordinary { property: PropertyId, token: RowToken },
    ImageSize { token: RowToken },
}

/// One property write.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PropertyWrite {
    pub property: PropertyId,
    pub value: ChoiceCode,
}

/// An ordered set of at most [`MAX_WRITES`] property writes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PropertyWrites {
    entries: [Option<PropertyWrite>; MAX_WRITES],
    len: usize,
}

impl PropertyWrites {
    pub fn iter(&self) -> impl Iterator<Item = &PropertyWrite> + '_ {
        self.entries.iter().take(self.len).filter_map(Option::as_ref)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn push(&mut self, write: PropertyWrite) {
        self.entries[self.len] = Some(write);
        self.len += 1;
    }
}

/// A stock consumer effect the composite arm still performs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetainedEffect {
    /// The size working cell `0x00956888` takes the pair's size.
    SizeCell,
    /// The aspect working cell `0x00956898` takes the pair's aspect.
    AspectCell,
    /// The current-property marker names the composite property.
    CurrentPropertyMarker,
    /// The display update, including its size-family property arm.
    DisplayUpdate,
    /// The size-family change marker property is recomputed.
    SizeFamilyChangeMarker,
}

/// An ordered set of at most [`MAX_RETAINED`] retained effects.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RetainedEffects {
    entries: [Option<RetainedEffect>; MAX_RETAINED],
    len: usize,
}

impl RetainedEffects {
    pub fn iter(&self) -> impl Iterator<Item = RetainedEffect> + '_ {
        self.entries.iter().take(self.len).filter_map(|effect| *effect)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn push(&mut self, effect: RetainedEffect) {
        self.entries[self.len] = Some(effect);
        self.len += 1;
    }
}

/// A resolved commit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Commit {
    pub writes: PropertyWrites,
    pub retained: RetainedEffects,
    pub image_size: Option<ImageSizePair>,
}

/// Why a target did not resolve.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refusal {
    MissingTable(PropertyId),
    NoConversion { property: PropertyId, value: u32, mode: ConversionMode },
    UnofferedRow(RowToken),
    MissingImageSizeRule,
}

/// The settings service the transaction reads and writes.
pub trait Settings {
    fn read(&self, property: PropertyId) -> ChoiceCode;
    fn write(&mut self, property: PropertyId, value: ChoiceCode);
}

/// The first row whose `mode` column equals `value`, returning its `column`
/// column. The `0x7ffffffe` sentinel is not a value.
pub fn convert(table: &[ConversionRow], value: u32, mode: ConversionMode, column: Column) -> Option<ChoiceCode> {
    let (ordinal, row) = table
        .iter()
        .enumerate()
        .find(|(ordinal, row)| column_value(row, mode as u8, *ordinal as u32) == value)?;
    let result = column_value(row, column as u8, ordinal as u32);
    if result == 0x7fff_fffe {
        None
    } else {
        Some(ChoiceCode::new(result))
    }
}

fn column_value(row: &ConversionRow, column: u8, ordinal: u32) -> u32 {
    match column {
        0 => ordinal,
        1 => row.stored.get(),
        2 => row.column1.get(),
        3 => row.effective.get(),
        _ => ordinal,
    }
}

pub fn resolve(tables: &ValueTables, target: ValueTarget) -> Result<Commit, Refusal> {
    match target {
        ValueTarget::Ordinary { property, token } => resolve_ordinary(tables, property, token),
        ValueTarget::ImageSize { token } => resolve_image_size(tables, token),
    }
}

fn resolve_ordinary(tables: &ValueTables, property: PropertyId, token: RowToken) -> Result<Commit, Refusal> {
    let table = tables.conversion(property).ok_or(Refusal::MissingTable(property))?;
    let value = convert(table, token.get(), ConversionMode::Ordinal, Column::Stored).ok_or(Refusal::NoConversion {
        property,
        value: token.get(),
        mode: ConversionMode::Ordinal,
    })?;
    let mut writes = PropertyWrites::default();
    writes.push(PropertyWrite { property, value });
    Ok(Commit { writes, retained: RetainedEffects::default(), image_size: None })
}

fn resolve_image_size(tables: &ValueTables, token: RowToken) -> Result<Commit, Refusal> {
    let rule = tables.image_size_rule().ok_or(Refusal::MissingImageSizeRule)?;
    let pair = tables.image_size_pair(token).ok_or(Refusal::UnofferedRow(token))?;
    let size = convert_key(tables, rule.size_property, pair.size.get())?;
    let aspect = convert_key(tables, rule.aspect_property, pair.aspect.get())?;
    let mut writes = PropertyWrites::default();
    writes.push(PropertyWrite { property: rule.size_property, value: size });
    writes.push(PropertyWrite { property: rule.aspect_property, value: aspect });
    let mut retained = RetainedEffects::default();
    for effect in [
        RetainedEffect::SizeCell,
        RetainedEffect::AspectCell,
        RetainedEffect::CurrentPropertyMarker,
        RetainedEffect::DisplayUpdate,
        RetainedEffect::SizeFamilyChangeMarker,
    ] {
        retained.push(effect);
    }
    Ok(Commit { writes, retained, image_size: Some(pair) })
}

/// The stock 0xf6 arm's change condition: the working size, aspect and third
/// cells differ from their stored cells.
pub fn cells_differ(working: [ChoiceCode; 3], stored: [ChoiceCode; 3]) -> bool {
    working != stored
}

fn convert_key(tables: &ValueTables, property: PropertyId, key: u32) -> Result<ChoiceCode, Refusal> {
    let table = tables.conversion(property).ok_or(Refusal::MissingTable(property))?;
    convert(table, key, ConversionMode::Effective, Column::Stored).ok_or(Refusal::NoConversion {
        property,
        value: key,
        mode: ConversionMode::Effective,
    })
}

/// Resolves and applies `target` to `settings` in stock order.
pub fn commit(tables: &ValueTables, target: ValueTarget, settings: &mut impl Settings) -> Result<Commit, Refusal> {
    let result = resolve(tables, target)?;
    for write in result.writes.iter() {
        settings.write(write.property, write.value);
    }
    Ok(result)
}

/// The entry choice captured at open, and the selections committed until the
/// picker ends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Transaction {
    target: ValueTarget,
    entry: PropertyWrites,
}

impl Transaction {
    /// Captures the values of the properties `target` can change.
    pub fn begin(tables: &ValueTables, target: ValueTarget, settings: &impl Settings) -> Result<Self, Refusal> {
        let entry = match target {
            ValueTarget::Ordinary { property, .. } => {
                tables.conversion(property).ok_or(Refusal::MissingTable(property))?;
                let mut entry = PropertyWrites::default();
                entry.push(PropertyWrite { property, value: settings.read(property) });
                entry
            }
            ValueTarget::ImageSize { .. } => {
                let rule = tables.image_size_rule().ok_or(Refusal::MissingImageSizeRule)?;
                for property in [rule.size_property, rule.aspect_property] {
                    tables.conversion(property).ok_or(Refusal::MissingTable(property))?;
                }
                let mut entry = PropertyWrites::default();
                for property in [
                    rule.size_property,
                    rule.aspect_property,
                    rule.change_property,
                    rule.change_property_1f,
                    rule.marker_property,
                ] {
                    entry.push(PropertyWrite { property, value: settings.read(property) });
                }
                entry
            }
        };
        Ok(Self { target, entry })
    }

    pub fn entry(&self) -> PropertyWrites {
        self.entry
    }

    pub fn select(
        &self,
        tables: &ValueTables,
        token: RowToken,
        settings: &mut impl Settings,
    ) -> Result<Commit, Refusal> {
        commit(tables, self.at(token), settings)
    }

    /// Restores only the properties this transaction captured.
    pub fn back(&self, settings: &mut impl Settings) -> PropertyWrites {
        for write in self.entry.iter() {
            settings.write(write.property, write.value);
        }
        self.entry
    }

    /// No property is written; the committed values stay.
    pub fn confirm(self) {}

    fn at(&self, token: RowToken) -> ValueTarget {
        match self.target {
            ValueTarget::Ordinary { property, .. } => ValueTarget::Ordinary { property, token },
            ValueTarget::ImageSize { .. } => ValueTarget::ImageSize { token },
        }
    }
}

#[cfg(test)]
mod tests;

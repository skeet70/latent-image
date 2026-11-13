use std::collections::BTreeMap;
use std::vec::Vec;

use super::*;

const SIZE: PropertyId = PropertyId::new(0xf6);
const ASPECT: PropertyId = PropertyId::new(0xf9);
const OTHER: PropertyId = PropertyId::new(0x40);

/// The intended IMAGE SIZE pairs: raw token to `(size, aspect)`.
const PAIRS: [(u32, u32, u32); 12] = [
    (0, 0, 0),
    (1, 0, 1),
    (2, 0, 2),
    (3, 0, 3),
    (4, 1, 0),
    (5, 1, 1),
    (6, 1, 2),
    (7, 1, 3),
    (8, 2, 0),
    (9, 2, 1),
    (10, 2, 2),
    (11, 2, 3),
];

const OFFERED: [u32; 9] = [1, 2, 3, 5, 6, 7, 9, 10, 11];

static SIZE_ROWS: &[ConversionRow] = &[
    ConversionRow { stored: ChoiceCode::new(0), column1: ChoiceCode::new(0), effective: ChoiceCode::new(0) },
    ConversionRow { stored: ChoiceCode::new(1), column1: ChoiceCode::new(1), effective: ChoiceCode::new(1) },
    ConversionRow { stored: ChoiceCode::new(2), column1: ChoiceCode::new(2), effective: ChoiceCode::new(2) },
];

static ASPECT_ROWS: &[ConversionRow] = &[
    ConversionRow { stored: ChoiceCode::new(0), column1: ChoiceCode::new(0), effective: ChoiceCode::new(0) },
    ConversionRow { stored: ChoiceCode::new(1), column1: ChoiceCode::new(1), effective: ChoiceCode::new(1) },
    ConversionRow { stored: ChoiceCode::new(2), column1: ChoiceCode::new(2), effective: ChoiceCode::new(2) },
    ConversionRow { stored: ChoiceCode::new(3), column1: ChoiceCode::new(3), effective: ChoiceCode::new(3) },
];

static OTHER_ROWS: &[ConversionRow] = &[
    ConversionRow { stored: ChoiceCode::new(10), column1: ChoiceCode::new(0), effective: ChoiceCode::new(0) },
    ConversionRow { stored: ChoiceCode::new(20), column1: ChoiceCode::new(0), effective: ChoiceCode::new(0) },
    ConversionRow { stored: ChoiceCode::new(30), column1: ChoiceCode::new(0), effective: ChoiceCode::new(0) },
];

static CONVERSIONS: &[(PropertyId, &[ConversionRow])] =
    &[(OTHER, OTHER_ROWS), (SIZE, SIZE_ROWS), (ASPECT, ASPECT_ROWS)];

static IMAGE_SIZES: [(RowToken, ImageSizePair); 12] = [
    (RowToken::new(0), ImageSizePair { size: SizeCode::new(0), aspect: AspectCode::new(0) }),
    (RowToken::new(1), ImageSizePair { size: SizeCode::new(0), aspect: AspectCode::new(1) }),
    (RowToken::new(2), ImageSizePair { size: SizeCode::new(0), aspect: AspectCode::new(2) }),
    (RowToken::new(3), ImageSizePair { size: SizeCode::new(0), aspect: AspectCode::new(3) }),
    (RowToken::new(4), ImageSizePair { size: SizeCode::new(1), aspect: AspectCode::new(0) }),
    (RowToken::new(5), ImageSizePair { size: SizeCode::new(1), aspect: AspectCode::new(1) }),
    (RowToken::new(6), ImageSizePair { size: SizeCode::new(1), aspect: AspectCode::new(2) }),
    (RowToken::new(7), ImageSizePair { size: SizeCode::new(1), aspect: AspectCode::new(3) }),
    (RowToken::new(8), ImageSizePair { size: SizeCode::new(2), aspect: AspectCode::new(0) }),
    (RowToken::new(9), ImageSizePair { size: SizeCode::new(2), aspect: AspectCode::new(1) }),
    (RowToken::new(10), ImageSizePair { size: SizeCode::new(2), aspect: AspectCode::new(2) }),
    (RowToken::new(11), ImageSizePair { size: SizeCode::new(2), aspect: AspectCode::new(3) }),
];

static TABLES: ValueTables = ValueTables::new(
    CONVERSIONS,
    &IMAGE_SIZES,
    Some(ImageSizeRule {
        size_property: SIZE,
        aspect_property: ASPECT,
        change_property: PropertyId::new(0x161),
        change_property_1f: PropertyId::new(0x165),
        marker_property: PropertyId::new(0x1e5),
    }),
);

#[derive(Default)]
struct FakeSettings {
    values: BTreeMap<u16, u32>,
    writes: Vec<(u16, u32)>,
}

impl FakeSettings {
    fn with(entries: &[(u16, u32)]) -> Self {
        Self { values: entries.iter().copied().collect(), writes: Vec::new() }
    }

    fn value(&self, property: u16) -> u32 {
        *self.values.get(&property).unwrap_or(&0)
    }

    fn take_writes(&mut self) -> Vec<(u16, u32)> {
        core::mem::take(&mut self.writes)
    }
}

impl Settings for FakeSettings {
    fn read(&self, property: PropertyId) -> ChoiceCode {
        ChoiceCode::new(self.value(property.get()))
    }

    fn write(&mut self, property: PropertyId, value: ChoiceCode) {
        self.values.insert(property.get(), value.get());
        self.writes.push((property.get(), value.get()));
    }
}

fn writes(commit: &Commit) -> Vec<(u16, u32)> {
    commit.writes.iter().map(|write| (write.property.get(), write.value.get())).collect()
}

fn retained(commit: &Commit) -> Vec<RetainedEffect> {
    commit.retained.iter().collect()
}

fn image_size(token: u32) -> ValueTarget {
    TABLES.target(SIZE, RowToken::new(token))
}

#[test]
fn image_size_entry_choice_is_the_pair_of_token_one() {
    let mut settings = FakeSettings::with(&[(SIZE.get(), 0), (ASPECT.get(), 1), (OTHER.get(), 7)]);
    let transaction = Transaction::begin(&TABLES, image_size(1), &settings).unwrap();
    assert_eq!(
        transaction.entry().iter().map(|write| (write.property.get(), write.value.get())).collect::<Vec<_>>(),
        vec![(SIZE.get(), 0), (ASPECT.get(), 1), (0x161, 0), (0x165, 0), (0x1e5, 0)]
    );
    let commit = transaction.select(&TABLES, RowToken::new(1), &mut settings).unwrap();
    assert_eq!(writes(&commit), vec![(SIZE.get(), 0), (ASPECT.get(), 1)]);
    assert_eq!(settings.take_writes(), vec![(SIZE.get(), 0), (ASPECT.get(), 1)]);
    assert_eq!(settings.value(OTHER.get()), 7);
}

#[test]
fn image_size_down_previews_the_next_token() {
    let mut settings = FakeSettings::with(&[(SIZE.get(), 0), (ASPECT.get(), 1)]);
    let transaction = Transaction::begin(&TABLES, image_size(1), &settings).unwrap();
    let commit = transaction.select(&TABLES, RowToken::new(2), &mut settings).unwrap();
    assert_eq!(writes(&commit), vec![(SIZE.get(), 0), (ASPECT.get(), 2)]);
    assert_eq!(settings.value(ASPECT.get()), 2);
    assert_eq!(
        retained(&commit),
        vec![
            RetainedEffect::SizeCell,
            RetainedEffect::AspectCell,
            RetainedEffect::CurrentPropertyMarker,
            RetainedEffect::DisplayUpdate,
            RetainedEffect::SizeFamilyChangeMarker,
        ]
    );
    assert_eq!(commit.image_size, Some(ImageSizePair { size: SizeCode::new(0), aspect: AspectCode::new(2) }));
}

#[test]
fn image_size_back_restores_only_the_captured_properties() {
    let mut settings = FakeSettings::with(&[(SIZE.get(), 0), (ASPECT.get(), 1), (OTHER.get(), 7)]);
    let transaction = Transaction::begin(&TABLES, image_size(1), &settings).unwrap();
    transaction.select(&TABLES, RowToken::new(2), &mut settings).unwrap();
    settings.write(OTHER, ChoiceCode::new(9));
    let restored = transaction.back(&mut settings);
    assert_eq!(
        restored.iter().map(|write| (write.property.get(), write.value.get())).collect::<Vec<_>>(),
        vec![(SIZE.get(), 0), (ASPECT.get(), 1), (0x161, 0), (0x165, 0), (0x1e5, 0)]
    );
    assert_eq!(settings.value(SIZE.get()), 0);
    assert_eq!(settings.value(ASPECT.get()), 1);
    assert_eq!(settings.value(OTHER.get()), 9);
    assert_eq!(settings.take_writes().last(), Some(&(0x1e5, 0)));
}

#[test]
fn image_size_confirm_keeps_the_selection_without_a_write() {
    let mut settings = FakeSettings::with(&[(SIZE.get(), 0), (ASPECT.get(), 1)]);
    let transaction = Transaction::begin(&TABLES, image_size(1), &settings).unwrap();
    transaction.select(&TABLES, RowToken::new(2), &mut settings).unwrap();
    settings.take_writes();
    transaction.confirm();
    assert!(settings.take_writes().is_empty());
    assert_eq!(settings.value(ASPECT.get()), 2);
}

#[test]
fn every_offered_token_commits_its_pair() {
    for token in OFFERED {
        let (_, size, aspect) = PAIRS[token as usize];
        let mut settings = FakeSettings::with(&[(SIZE.get(), 0), (ASPECT.get(), 1)]);
        let transaction = Transaction::begin(&TABLES, image_size(1), &settings).unwrap();
        let commit = transaction.select(&TABLES, RowToken::new(token), &mut settings).unwrap();
        assert_eq!(writes(&commit), vec![(SIZE.get(), size), (ASPECT.get(), aspect)], "token {token}");
        assert_eq!(settings.value(SIZE.get()), size);
        assert_eq!(settings.value(ASPECT.get()), aspect);
    }
}

#[test]
fn image_size_unoffered_token_refuses_without_writing() {
    let mut settings = FakeSettings::with(&[(SIZE.get(), 0), (ASPECT.get(), 1)]);
    let transaction = Transaction::begin(&TABLES, image_size(1), &settings).unwrap();
    settings.take_writes();
    assert_eq!(
        transaction.select(&TABLES, RowToken::new(12), &mut settings),
        Err(Refusal::UnofferedRow(RowToken::new(12)))
    );
    assert!(settings.take_writes().is_empty());
}

#[test]
fn ordinary_rule_commits_the_stored_code_of_the_row_ordinal() {
    let mut settings = FakeSettings::with(&[(OTHER.get(), 10)]);
    let target = TABLES.target(OTHER, RowToken::new(1));
    assert_eq!(target, ValueTarget::Ordinary { property: OTHER, token: RowToken::new(1) });
    let commit = commit(&TABLES, target, &mut settings).unwrap();
    assert_eq!(writes(&commit), vec![(OTHER.get(), 20)]);
    assert!(commit.retained.is_empty());
    assert_eq!(commit.image_size, None);
    assert_eq!(settings.value(OTHER.get()), 20);
}

#[test]
fn ordinary_rule_leaves_other_properties_alone() {
    let mut settings = FakeSettings::with(&[(OTHER.get(), 10), (SIZE.get(), 2), (ASPECT.get(), 3)]);
    let target = TABLES.target(OTHER, RowToken::new(2));
    commit(&TABLES, target, &mut settings).unwrap();
    assert_eq!(settings.take_writes(), vec![(OTHER.get(), 30)]);
    assert_eq!(settings.value(SIZE.get()), 2);
    assert_eq!(settings.value(ASPECT.get()), 3);
}

#[test]
fn ordinary_rule_refuses_a_property_without_a_table() {
    let mut settings = FakeSettings::with(&[(0x41, 5)]);
    let target = TABLES.target(PropertyId::new(0x41), RowToken::new(0));
    assert_eq!(commit(&TABLES, target, &mut settings), Err(Refusal::MissingTable(PropertyId::new(0x41))));
    assert!(settings.take_writes().is_empty());
}

#[test]
fn conversion_refuses_the_sentinel_result() {
    static SENTINEL: &[ConversionRow] = &[ConversionRow {
        stored: ChoiceCode::new(0x7fff_fffe),
        column1: ChoiceCode::new(0),
        effective: ChoiceCode::new(0),
    }];
    assert_eq!(convert(SENTINEL, 0, ConversionMode::Ordinal, Column::Stored), None);
}

#[test]
fn conversion_columns_match_the_stock_arguments() {
    static ROWS: &[ConversionRow] = &[ConversionRow {
        stored: ChoiceCode::new(5),
        column1: ChoiceCode::new(6),
        effective: ChoiceCode::new(7),
    }];
    assert_eq!(convert(ROWS, 0, ConversionMode::Ordinal, Column::Stored), Some(ChoiceCode::new(5)));
    assert_eq!(convert(ROWS, 5, ConversionMode::Stored, Column::Column1), Some(ChoiceCode::new(6)));
    assert_eq!(convert(ROWS, 6, ConversionMode::Column1, Column::Effective), Some(ChoiceCode::new(7)));
    assert_eq!(convert(ROWS, 7, ConversionMode::Effective, Column::Ordinal), Some(ChoiceCode::new(0)));
    assert_eq!(convert(ROWS, 9, ConversionMode::Effective, Column::Stored), None);
}

#[test]
fn cells_differ_compares_each_dimension() {
    let stored = [ChoiceCode::new(0), ChoiceCode::new(1), ChoiceCode::new(0)];
    assert!(!cells_differ(stored, stored));
    for index in 0..3 {
        let mut working = stored;
        working[index] = ChoiceCode::new(2);
        assert!(cells_differ(working, stored));
    }
}

#[test]
fn conversion_stops_at_the_first_matching_row() {
    static ROWS: &[ConversionRow] = &[
        ConversionRow {
            stored: ChoiceCode::new(0x7fff_fffe),
            column1: ChoiceCode::new(0),
            effective: ChoiceCode::new(0),
        },
        ConversionRow { stored: ChoiceCode::new(0), column1: ChoiceCode::new(0), effective: ChoiceCode::new(0) },
    ];
    assert_eq!(convert(ROWS, 0, ConversionMode::Effective, Column::Stored), None);
}

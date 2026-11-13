use crate::summary::{summarize, AreaEntry, AreaStatistics, RegionMean, FIELDS, GRID};

fn entry(id: u32, value: impl Fn(usize, usize) -> u32, weight: u32) -> AreaEntry {
    let mut entry = AreaEntry { id, fields: [5; FIELDS], ..AreaEntry::default() };
    for row in 0..GRID {
        for column in 0..GRID {
            entry.values[row][column] = value(row, column);
            entry.weights[row][column] = weight;
        }
    }
    entry
}

#[test]
fn regions_average_the_centre_cell_column_row_inner_three_and_inner_five() {
    let mut statistics = AreaStatistics { present: 1, layout: 2, ..AreaStatistics::default() };
    statistics.entries[0] = entry(1, |row, column| if row == 7 || column == 7 { 1_000_000 } else { (row * 10 + column) as u32 }, 4);
    let summary = summarize(&statistics);
    let detail = summary.detail.unwrap();
    let means = detail.entries[0].unwrap().means;
    let value = |mean: RegionMean| mean.value;
    assert_eq!(value(means[0]), 33);
    assert_eq!(value(means[1]), (23 + 33 + 43) / 3);
    assert_eq!(value(means[2]), (32 + 33 + 34) / 3);
    assert_eq!(value(means[3]), 33);
    assert_eq!(value(means[4]), 33);
    assert!(means.iter().all(|mean| mean.weight == 4));
    assert_eq!(detail.layout, 2);
    assert_eq!(detail.entries[0].unwrap().values[48], 66);
}

#[test]
fn an_asymmetric_grid_tells_the_regions_apart() {
    let mut statistics = AreaStatistics { present: 1, ..AreaStatistics::default() };
    statistics.entries[0] = entry(1, |row, column| if (row, column) == (1, 1) { 50 } else if (row, column) == (2, 3) { 9 } else { 0 }, 0);
    let means = summarize(&statistics).detail.unwrap().entries[0].unwrap().means;
    assert_eq!(means.map(|mean| mean.value), [0, 3, 0, 1, (50 + 9) / 25]);
}

#[test]
fn absent_entries_and_absent_statistics_write_nothing_of_their_own() {
    let mut statistics = AreaStatistics { present: 1, ..AreaStatistics::default() };
    statistics.entries[1] = entry(2, |_, _| 1, 1);
    let detail = summarize(&statistics).detail.unwrap();
    assert!(detail.entries[0].is_none());
    assert!(detail.entries[1].is_some());
    assert_eq!(detail.ids, [0, 2, 0, 0, 0]);
    statistics.present = 0;
    assert!(summarize(&statistics).detail.is_none());
}

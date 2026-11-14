#![forbid(unsafe_code)]

//! The area statistics summary a half press writes into the client's frame.

pub const ENTRIES: usize = 5;
pub const GRID: usize = 8;
pub const SUMMARY_SIDE: usize = 7;
pub const CELLS: usize = SUMMARY_SIDE * SUMMARY_SIDE;
pub const FIELDS: usize = 11;

/// One statistics entry as the record layer holds it. Entries with a zero id are absent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AreaEntry {
    pub id: u32,
    pub values: [[u32; GRID]; GRID],
    pub weights: [[u32; GRID]; GRID],
    /// The entry's own words, in the order the summary lists them.
    pub fields: [u32; FIELDS],
}

impl Default for AreaEntry {
    fn default() -> Self { Self { id: 0, values: [[0; GRID]; GRID], weights: [[0; GRID]; GRID], fields: [0; FIELDS] } }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AreaStatistics {
    pub present: u32,
    pub layout: u8,
    pub entries: [AreaEntry; ENTRIES],
}

/// Means over one region of the summary grid; zero for an empty region.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RegionMean { pub value: u32, pub weight: u32 }

/// Regions in the order the summary lists them: the centre cell, the centre column, the centre
/// row, the inner 3x3 and the inner 5x5 of the 7x7 grid.
pub const REGIONS: usize = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntrySummary {
    pub values: [u32; CELLS],
    pub weights: [u32; CELLS],
    pub means: [RegionMean; REGIONS],
    pub fields: [u32; FIELDS],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameSummary {
    pub present: u32,
    /// Absent when `present` is zero; then only the tags and `present` are written.
    pub detail: Option<SummaryDetail>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SummaryDetail {
    pub layout: u8,
    pub ids: [u32; ENTRIES],
    /// `None` for an absent entry, whose part of the frame is left as it was.
    pub entries: [Option<EntrySummary>; ENTRIES],
}

fn regions(row: usize, column: usize) -> [bool; REGIONS] {
    let inner5 = (1..=5).contains(&row) && (1..=5).contains(&column);
    let inner3 = (2..=4).contains(&row) && (2..=4).contains(&column);
    [inner3 && row == 3 && column == 3, inner3 && column == 3, inner3 && row == 3, inner3, inner5]
}

fn summarize_entry(entry: &AreaEntry) -> EntrySummary {
    let mut values = [0; CELLS];
    let mut weights = [0; CELLS];
    let mut sums = [(0u64, 0u64, 0u32); REGIONS];
    for row in 0..SUMMARY_SIDE {
        for column in 0..SUMMARY_SIDE {
            let (value, weight) = (entry.values[row][column], entry.weights[row][column]);
            values[row * SUMMARY_SIDE + column] = value;
            weights[row * SUMMARY_SIDE + column] = weight;
            for (sum, inside) in sums.iter_mut().zip(regions(row, column)) {
                if inside {
                    sum.0 += u64::from(value);
                    sum.1 += u64::from(weight);
                    sum.2 += 1;
                }
            }
        }
    }
    let means = sums.map(|(value, weight, count)| match count {
        0 => RegionMean::default(),
        count => RegionMean { value: (value / u64::from(count)) as u32, weight: (weight / u64::from(count)) as u32 },
    });
    EntrySummary { values, weights, means, fields: entry.fields }
}

pub fn summarize(statistics: &AreaStatistics) -> FrameSummary {
    if statistics.present == 0 { return FrameSummary { present: 0, detail: None }; }
    let entries = statistics.entries.map(|entry| (entry.id != 0).then(|| summarize_entry(&entry)));
    FrameSummary {
        present: statistics.present,
        detail: Some(SummaryDetail { layout: statistics.layout, ids: statistics.entries.map(|entry| entry.id), entries }),
    }
}

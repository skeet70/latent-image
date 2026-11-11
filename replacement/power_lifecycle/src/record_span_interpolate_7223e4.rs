//! Span interpolation between two parallel lines, image-4 body 0x007223e4.
//!
//! `probe` accepts a candidate span and returns a positive value to stop a
//! scan; `report` receives the spans the interpolation settles on.

use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};

pub const STACK_ENTRIES: usize = 33;

pub trait SpanInterpolate {
    fn probe(&mut self, x_first: i32, x_second: i32, y_first: i32, y_second: i32) -> i32;
    fn report(&mut self, x_first: i32, x_second: i32, y_first: i32, y_second: i32);
}

fn coordinate(step: i32, row: i32, origin: i32) -> i32 { step.wrapping_mul(row).wrapping_add(origin) }

/// The image-4 body at 0x007223e4.
pub fn helper_7223e4<M: GuestAddressSpace, E: SpanInterpolate>(
    context: &mut RecordContext<M, E>,
    x: i32,
    rows: i32,
    x_step: i32,
    y: i32,
    y_step: i32,
) -> Result<(), MemoryError> {
    let mut lows = [0i32; STACK_ENTRIES];
    let mut highs = [0i32; STACK_ENTRIES];
    let mut low = 0i32;
    let mut high = rows.wrapping_sub(1);
    let mut depth = 0usize;
    loop {
        if low >= high {
            if depth == 0 { return Ok(()); }
            depth -= 1;
            low = lows[depth];
            high = highs[depth];
            continue;
        }
        let difference = high.wrapping_sub(low);
        let mut mid = low.wrapping_add(difference.wrapping_add(((difference as u32) >> 31) as i32) >> 1);
        let mut start = low;
        let mut x_start = coordinate(x_step, start, x);
        let mut y_start = coordinate(y_step, start, y);
        let mut back = high;
        loop {
            while start <= high {
                let x_mid = coordinate(x_step, mid, x);
                let y_mid = coordinate(y_step, mid, y);
                if context.effects.probe(x_start, x_mid, y_start, y_mid) >= 0 { break; }
                start = start.wrapping_add(1);
                x_start = x_start.wrapping_add(x_step);
                y_start = y_start.wrapping_add(y_step);
            }
            let x_mid = coordinate(x_step, mid, x);
            let y_mid = coordinate(y_step, mid, y);
            let mut x_back = coordinate(x_step, back, x);
            let mut y_back = coordinate(y_step, back, y);
            if back >= low {
                loop {
                    if context.effects.probe(x_mid, x_back, y_mid, y_back) > 0 { break; }
                    back = back.wrapping_sub(1);
                    x_back = x_back.wrapping_sub(x_step);
                    y_back = y_back.wrapping_sub(y_step);
                    if low > back { break; }
                }
            }
            if start > high || start >= back || low > back {
                break;
            }
            context.effects.report(x_start, coordinate(x_step, back, x), y_start, coordinate(y_step, back, y));
            if start == mid {
                mid = back;
            } else if back == mid {
                mid = start;
            }
            back = back.wrapping_sub(1);
            start = start.wrapping_add(1);
            x_start = x_start.wrapping_add(x_step);
            y_start = y_start.wrapping_add(y_step);
        }
        if high >= start && mid != start {
            context.effects.report(
                coordinate(x_step, start, x),
                coordinate(x_step, mid, x),
                coordinate(y_step, start, y),
                coordinate(y_step, mid, y),
            );
            mid = start;
        }
        if start.wrapping_sub(low) >= high.wrapping_sub(back) {
            lows[depth] = low;
            highs[depth] = mid.wrapping_sub(1);
            depth += 1;
            low = mid.wrapping_add(1);
        } else {
            lows[depth] = mid.wrapping_add(1);
            highs[depth] = high;
            depth += 1;
            high = mid.wrapping_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, GuestAddress, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    #[derive(Default)]
    struct Script {
        probes: Vec<(i32, i32, i32, i32)>,
        results: Vec<i32>,
        reports: Vec<(i32, i32, i32, i32)>,
    }

    impl SpanInterpolate for Script {
        fn probe(&mut self, x_first: i32, x_second: i32, y_first: i32, y_second: i32) -> i32 {
            self.probes.push((x_first, x_second, y_first, y_second));
            self.results.get(self.probes.len() - 1).copied().unwrap_or(0)
        }
        fn report(&mut self, x_first: i32, x_second: i32, y_first: i32, y_second: i32) {
            self.reports.push((x_first, x_second, y_first, y_second));
        }
    }

    fn run(rows: i32, results: Vec<i32>) -> Script {
        let mut memory = [0u8; 8];
        let descriptor = RegionDescriptor::new(GuestAddress::new(0x1000), ByteCount(8), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut memory).unwrap()];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let mut context = RecordContext::new(space, Script { results, ..Script::default() });
        assert_eq!(helper_7223e4(&mut context, 0, rows, 1, 0, 1), Ok(()));
        context.effects
    }

    #[test]
    fn a_single_row_span_returns_without_probing() {
        let script = run(0, vec![]);
        assert!(script.probes.is_empty());
        assert!(script.reports.is_empty());
        let script = run(1, vec![]);
        assert!(script.probes.is_empty());
    }

    #[test]
    fn exhausted_scans_descend_without_reporting() {
        let script = run(2, vec![-1; 16]);
        assert_eq!(script.probes, vec![(0, 0, 0, 0), (1, 0, 1, 0), (0, 1, 0, 1), (0, 0, 0, 0)]);
        assert!(script.reports.is_empty());
    }

    #[test]
    fn accepted_scans_report_the_span_between_the_two_scans() {
        let script = run(2, vec![0, 1, 0, 1]);
        assert_eq!(script.probes, vec![(0, 0, 0, 0), (0, 1, 0, 1), (1, 1, 1, 1), (1, 0, 1, 0)]);
        assert_eq!(script.reports, vec![(0, 1, 0, 1)]);
    }

    #[test]
    fn a_skipped_span_still_reports_the_settled_remainder() {
        let script = run(4, vec![-1, 0, 1, 0, 1, 0, 0, 0]);
        assert_eq!(
            script.probes,
            vec![(0, 1, 0, 1), (1, 1, 1, 1), (1, 3, 1, 3), (2, 3, 2, 3), (3, 2, 3, 2), (0, 0, 0, 0), (0, 1, 0, 1), (0, 0, 0, 0)]
        );
        assert_eq!(script.reports, vec![(1, 3, 1, 3), (2, 3, 2, 3)]);
    }
}

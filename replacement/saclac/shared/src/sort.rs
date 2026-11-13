#![forbid(unsafe_code)]

//! The in-place sort of two arrays that move together (0x7223e4).

use core::cmp::Ordering;

/// Items addressed by index; a swap exchanges the two items in both arrays.
pub trait PairedItems {
    fn compare(&mut self, first: usize, second: usize) -> Ordering;
    fn swap(&mut self, first: usize, second: usize);
}

const PENDING: usize = 2 * usize::BITS as usize;

/// Sorts the first `count` items ascending.
pub fn sort(items: &mut impl PairedItems, count: usize) {
    let mut pending = [(0isize, 0isize); PENDING];
    let mut depth = 0;
    let (mut low, mut high) = (0isize, count as isize - 1);
    loop {
        while low >= high {
            if depth == 0 {
                return;
            }
            depth -= 1;
            (low, high) = pending[depth];
        }
        let mut pivot = low + (high - low) / 2;
        let (mut left, mut right) = (low, high);
        loop {
            while left <= high && items.compare(left as usize, pivot as usize) == Ordering::Less {
                left += 1;
            }
            while right >= low && items.compare(pivot as usize, right as usize) != Ordering::Greater {
                right -= 1;
            }
            if left > high || left >= right {
                break;
            }
            items.swap(left as usize, right as usize);
            if left == pivot {
                pivot = right;
            } else if right == pivot {
                pivot = left;
            }
            left += 1;
            right -= 1;
        }
        if left <= high && left != pivot {
            items.swap(left as usize, pivot as usize);
            pivot = left;
        }
        if left - low < high - right {
            pending[depth] = (pivot + 1, high);
            high = pivot - 1;
        } else {
            pending[depth] = (low, pivot - 1);
            low = pivot + 1;
        }
        depth += 1;
    }
}

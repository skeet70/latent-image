use core::cmp::Ordering;

use crate::sort::*;

struct Pairs {
    keys: Vec<i32>,
    labels: Vec<u32>,
}

impl PairedItems for Pairs {
    fn compare(&mut self, first: usize, second: usize) -> Ordering { self.keys[first].cmp(&self.keys[second]) }

    fn swap(&mut self, first: usize, second: usize) {
        self.keys.swap(first, second);
        self.labels.swap(first, second);
    }
}

fn pairs(keys: &[i32]) -> Pairs { Pairs { keys: keys.to_vec(), labels: (0..keys.len() as u32).collect() } }

fn sorted(keys: &[i32]) -> Pairs {
    let mut items = pairs(keys);
    sort(&mut items, keys.len());
    items
}

#[test]
fn keys_end_ascending_with_their_labels() {
    let mut seed = 0x1234_5678u32;
    for length in 0..64 {
        let keys: Vec<i32> = (0..length)
            .map(|_| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                (seed >> 16) as i32 % 9 - 4
            })
            .collect();
        let items = sorted(&keys);
        assert!(items.keys.windows(2).all(|pair| pair[0] <= pair[1]), "{keys:?}");
        let mut labels = items.labels.clone();
        labels.sort_unstable();
        assert_eq!(labels, (0..length as u32).collect::<Vec<_>>());
        assert!(items.labels.iter().zip(&items.keys).all(|(label, key)| keys[*label as usize] == *key));
    }
}

#[test]
fn only_the_first_count_items_move() {
    let mut items = pairs(&[3, 2, 1, 0]);
    sort(&mut items, 3);
    assert_eq!(items.keys, [1, 2, 3, 0]);
}

#[test]
fn equal_keys_can_leave_their_original_order() {
    assert_eq!(sorted(&[1, 1, 0]).labels, [2, 1, 0]);
    assert_eq!(sorted(&[2, 1, 2]).labels, [1, 0, 2]);
}

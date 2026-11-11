//! Owned station-order queue for X-M1 1.50.
//!
//! Source pin: 55436fc85c0560dd252b0410af6f2427fe198cd3.
//! The selected queue operations are 0x012dd798, 0x012dd8a0,
//! 0x012dd948 and 0x012dd9e8. Their indexed extents match the
//! authenticated overlay47 bytes.

#[path = "wireless_station.rs"]
pub mod station;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StationOrderKind(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StationQueueError {
    AllocationHalt,
}

#[derive(Clone, Copy)]
struct OrderSlot {
    kind: StationOrderKind,
    next: Option<usize>,
}

pub struct StationOrderQueue<const N: usize> {
    slots: [OrderSlot; N],
    first: Option<usize>,
    last: Option<usize>,
    free: Option<usize>,
    count: usize,
}

impl<const N: usize> Default for StationOrderQueue<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> StationOrderQueue<N> {
    pub const fn new() -> Self {
        let mut slots = [OrderSlot { kind: StationOrderKind(0), next: None }; N];
        let mut index = 0;
        while index < N {
            slots[index].next = if index + 1 < N { Some(index + 1) } else { None };
            index += 1;
        }
        Self {
            slots,
            first: None,
            last: None,
            free: if N == 0 { None } else { Some(0) },
            count: 0,
        }
    }

    pub const fn len(&self) -> usize {
        self.count
    }

    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    // 0x012dd948 halts when its twelve-byte node allocation fails.
    pub fn append(&mut self, kind: StationOrderKind) -> Result<(), StationQueueError> {
        let index = self.free.ok_or(StationQueueError::AllocationHalt)?;
        self.free = self.slots[index].next;
        self.slots[index] = OrderSlot { kind, next: None };
        match self.last {
            Some(last) => self.slots[last].next = Some(index),
            None => self.first = Some(index),
        }
        self.last = Some(index);
        self.count += 1;
        Ok(())
    }

    pub fn contains(&self, kind: StationOrderKind) -> bool {
        let mut cursor = self.first;
        while let Some(index) = cursor {
            if self.slots[index].kind == kind {
                return true;
            }
            cursor = self.slots[index].next;
        }
        false
    }

    pub fn remove_first(&mut self, kind: StationOrderKind) -> bool {
        let mut previous: Option<usize> = None;
        let mut cursor = self.first;
        while let Some(index) = cursor {
            let next = self.slots[index].next;
            if self.slots[index].kind == kind {
                match previous {
                    Some(previous) => self.slots[previous].next = next,
                    None => self.first = next,
                }
                if self.last == Some(index) {
                    self.last = previous;
                }
                self.release(index);
                return true;
            }
            previous = cursor;
            cursor = next;
        }
        false
    }

    pub fn pop(&mut self) -> Option<StationOrderKind> {
        let index = self.first?;
        let kind = self.slots[index].kind;
        self.first = self.slots[index].next;
        if self.first.is_none() {
            self.last = None;
        }
        self.release(index);
        Some(kind)
    }

    pub fn clear(&mut self) {
        while self.pop().is_some() {}
    }

    fn release(&mut self, index: usize) {
        self.slots[index].next = self.free;
        self.free = Some(index);
        self.count -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{StationOrderKind, StationOrderQueue, StationQueueError};

    #[test]
    fn removing_one_match_preserves_other_orders_and_duplicates() {
        let mut queue = StationOrderQueue::<5>::new();
        for value in [2, 1, 3, 1, 0] {
            queue.append(StationOrderKind(value)).unwrap();
        }
        assert!(queue.contains(StationOrderKind(1)));
        assert_eq!(queue.len(), 5);
        assert!(queue.remove_first(StationOrderKind(1)));
        assert_eq!(queue.len(), 4);
        assert!(queue.contains(StationOrderKind(1)));
        assert!(!queue.remove_first(StationOrderKind(9)));
        for value in [2, 3, 1, 0] {
            assert_eq!(queue.pop(), Some(StationOrderKind(value)));
        }
        assert_eq!(queue.pop(), None);
        assert!(queue.is_empty());
    }

    #[test]
    fn exhausted_allocation_leaves_existing_orders_unchanged() {
        let mut queue = StationOrderQueue::<2>::new();
        queue.append(StationOrderKind(3)).unwrap();
        queue.append(StationOrderKind(1)).unwrap();
        assert_eq!(queue.append(StationOrderKind(2)), Err(StationQueueError::AllocationHalt));
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.pop(), Some(StationOrderKind(3)));
        queue.append(StationOrderKind(2)).unwrap();
        assert_eq!(queue.pop(), Some(StationOrderKind(1)));
        assert_eq!(queue.pop(), Some(StationOrderKind(2)));
    }

    #[test]
    fn clearing_releases_every_order_for_reuse() {
        let mut queue = StationOrderQueue::<3>::new();
        for value in [0, 1, 2] {
            queue.append(StationOrderKind(value)).unwrap();
        }
        queue.clear();
        assert_eq!(queue.len(), 0);
        assert!(!queue.contains(StationOrderKind(1)));
        for value in [3, 2, 1] {
            queue.append(StationOrderKind(value)).unwrap();
        }
        for value in [3, 2, 1] {
            assert_eq!(queue.pop(), Some(StationOrderKind(value)));
        }
    }

    #[test]
    fn removing_tail_then_appending_preserves_fifo_order() {
        let mut queue = StationOrderQueue::<2>::new();
        queue.append(StationOrderKind(0)).unwrap();
        queue.append(StationOrderKind(1)).unwrap();
        assert!(queue.remove_first(StationOrderKind(1)));
        queue.append(StationOrderKind(2)).unwrap();
        assert_eq!(queue.pop(), Some(StationOrderKind(0)));
        assert_eq!(queue.pop(), Some(StationOrderKind(2)));
        queue.append(StationOrderKind(3)).unwrap();
        assert!(queue.remove_first(StationOrderKind(3)));
        assert_eq!(queue.pop(), None);
        queue.append(StationOrderKind(1)).unwrap();
        assert_eq!(queue.pop(), Some(StationOrderKind(1)));
    }

    #[test]
    fn empty_storage_reports_the_stock_allocation_halt() {
        let mut queue = StationOrderQueue::<0>::new();
        assert_eq!(queue.append(StationOrderKind(0)), Err(StationQueueError::AllocationHalt));
        assert_eq!(queue.pop(), None);
        assert!(!queue.remove_first(StationOrderKind(0)));
    }
}

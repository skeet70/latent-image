//! Interrupt-controller source ownership across the kernel's group-mask window.
//!
//! X-M1 1.50 kernel0 0x42b00 and 0x42eb0 write all ones to a group's
//! clear-enable register and later write the restored word to its set-enable
//! register, on the calling core's bank (E-XM1-IRQ-GROUP-MASK-WINDOW-150). A
//! line that a clear-enable write disables stays owned by that bank until the
//! next set-enable write to its group on that bank. A source raised at an
//! owned, disabled line latches pending; that set-enable write delivers it if
//! it re-enables the line and discards it otherwise. Physical latching while
//! masked is unmeasured.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// A controller source, 0..128.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line(u8);

impl Line {
    pub fn new(line: u8) -> Option<Self> {
        (line < 128).then_some(Self(line))
    }

    fn bit(self) -> u128 {
        1 << self.0
    }
}

/// A 32-source enable group, 0..4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Group(u8);

impl Group {
    pub fn new(group: u8) -> Option<Self> {
        (group < 4).then_some(Self(group))
    }

    fn bits(self, word: u32) -> u128 {
        (word as u128) << (32 * self.0)
    }
}

/// One bank's enable and pending words with its open mask windows.
/// `masked`: lines a clear-enable write disabled whose group has had no
/// set-enable write since. `latched`: lines a raise made pending while masked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lines {
    pub enabled: u128,
    pub pending: u128,
    pub masked: u128,
    pub latched: u128,
}

impl Lines {
    pub fn clear_enable(self, group: Group, word: u32) -> Self {
        let bits = group.bits(word);
        Self { enabled: self.enabled & !bits, masked: self.masked | (self.enabled & bits), ..self }
    }

    pub fn set_enable(self, group: Group, word: u32) -> Self {
        let bits = group.bits(word);
        let span = group.bits(u32::MAX);
        let dropped = self.latched & span & !bits;
        Self {
            enabled: self.enabled | bits,
            pending: self.pending & !dropped,
            masked: self.masked & !span,
            latched: self.latched & !span,
        }
    }

    pub fn raise(self, line: Line) -> Self {
        let bit = line.bit();
        let fresh = self.masked & !self.pending & bit;
        Self { pending: self.pending | bit, latched: self.latched | fresh, ..self }
    }

    pub fn owns(self, line: Line) -> bool {
        (self.enabled | self.masked) & line.bit() != 0
    }

    fn words(self) -> (u128, u128, u128, u128) {
        (self.enabled, self.pending, self.masked, self.latched)
    }
}

/// How many owning banks a source admits.
#[pyclass(eq, eq_int, frozen, from_py_object, module = "x70_native._native")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IrqRoute {
    /// Every owning bank receives the event.
    Every,
    /// None or one owning bank; two refuse.
    AtMostOne,
    /// Exactly one owning bank; none or two refuse.
    ExactlyOne,
}

/// The banks owning `line` under `route`, in bank order, or None for a refusal.
pub fn route(banks: [Lines; 2], line: Line, route: IrqRoute) -> Option<Vec<usize>> {
    let owners: Vec<usize> = (0..2).filter(|&cpu| banks[cpu].owns(line)).collect();
    match (route, owners.len()) {
        (IrqRoute::Every, _) | (IrqRoute::AtMostOne, 0 | 1) | (IrqRoute::ExactlyOne, 1) => Some(owners),
        _ => None,
    }
}

fn line(value: u8) -> PyResult<Line> {
    Line::new(value).ok_or_else(|| PyValueError::new_err(format!("IRQ line {value} is outside 0..128")))
}

fn group(value: u8) -> PyResult<Group> {
    Group::new(value).ok_or_else(|| PyValueError::new_err(format!("IRQ enable group {value} is outside 0..4")))
}

type Words = (u128, u128, u128, u128);

fn lines((enabled, pending, masked, latched): Words) -> Lines {
    Lines { enabled, pending, masked, latched }
}

/// `(enabled, pending, masked, latched)` after a clear-enable write of `word` to `group`.
#[pyfunction]
pub(crate) fn irq_enable_clear(bank: Words, group_index: u8, word: u32) -> PyResult<Words> {
    Ok(lines(bank).clear_enable(group(group_index)?, word).words())
}

/// `(enabled, pending, masked, latched)` after a set-enable write of `word` to `group`.
#[pyfunction]
pub(crate) fn irq_enable_set(bank: Words, group_index: u8, word: u32) -> PyResult<Words> {
    Ok(lines(bank).set_enable(group(group_index)?, word).words())
}

/// `(enabled, pending, masked, latched)` after a source raises `line`.
#[pyfunction]
pub(crate) fn irq_raise(bank: Words, line_index: u8) -> PyResult<Words> {
    Ok(lines(bank).raise(line(line_index)?).words())
}

/// The banks owning `line` under `route`, or None for a refusal.
#[pyfunction]
pub(crate) fn irq_route(banks: [Words; 2], line_index: u8, policy: IrqRoute) -> PyResult<Option<Vec<usize>>> {
    Ok(route(banks.map(lines), line(line_index)?, policy))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two banks driven through the stock kernel's register sequences.
    #[derive(Default)]
    struct Controller {
        banks: [Lines; 2],
    }

    fn at(value: u8) -> Line {
        Line::new(value).unwrap()
    }

    fn group_of(line: Line) -> Group {
        Group::new(line.0 / 32).unwrap()
    }

    fn word_bit(line: Line) -> u32 {
        1 << (line.0 % 32)
    }

    impl Controller {
        fn enabled_word(&self, cpu: usize, group: Group) -> u32 {
            (self.banks[cpu].enabled >> (32 * group.0)) as u32
        }

        /// Enables `line` on `cpu` through 0x42b00's clear-all and saved|bit restore.
        fn register(&mut self, cpu: usize, line: Line) {
            self.window(cpu, line, |saved| saved | word_bit(line), |_| {});
        }

        /// 0x42b00/0x42eb0 on `cpu` for `line`: save the group's enable word, clear all
        /// 32 sources, run `inside`, then set `restore(saved)`.
        fn window(&mut self, cpu: usize, line: Line, restore: impl Fn(u32) -> u32, inside: impl FnOnce(&mut Self)) {
            let group = group_of(line);
            let saved = self.enabled_word(cpu, group);
            self.banks[cpu] = self.banks[cpu].clear_enable(group, u32::MAX);
            inside(self);
            self.banks[cpu] = self.banks[cpu].set_enable(group, restore(saved));
        }

        /// A device event: raise `line` at every bank `policy` routes it to.
        fn raise(&mut self, line: Line, policy: IrqRoute) -> Option<Vec<usize>> {
            let targets = route(self.banks, line, policy)?;
            for &cpu in &targets {
                self.banks[cpu] = self.banks[cpu].raise(line);
            }
            Some(targets)
        }

        fn deliverable(&self, cpu: usize, line: Line) -> bool {
            self.banks[cpu].enabled & self.banks[cpu].pending & line.bit() != 0
        }
    }

    /// Registers `sources` on `cpu`, opens a window registering `neighbour` in the
    /// first source's group, raises every source inside it, and returns whether
    /// each source was deliverable inside the window and after the restore.
    fn raise_in_window(cpu: usize, sources: &[u8], neighbour: u8, policy: IrqRoute) -> Vec<(Option<Vec<usize>>, bool, bool)> {
        let mut controller = Controller::default();
        for &source in sources {
            controller.register(cpu, at(source));
        }
        let neighbour = at(neighbour);
        let mut inside = Vec::new();
        controller.window(cpu, neighbour, |saved| saved | word_bit(neighbour), |c| {
            for &source in sources {
                let targets = c.raise(at(source), policy);
                inside.push((targets, c.deliverable(cpu, at(source))));
            }
        });
        inside
            .into_iter()
            .zip(sources)
            .map(|((targets, during), &source)| (targets, during, controller.deliverable(cpu, at(source))))
            .collect()
    }

    #[test]
    fn image_stream_channels_raised_in_the_window_deliver_after_the_restore() {
        for (targets, during, after) in raise_in_window(0, &[0x2c, 0x2d], 0x2f, IrqRoute::ExactlyOne) {
            assert_eq!(targets, Some(vec![0]));
            assert!(!during && after);
        }
    }

    #[test]
    fn producer_stream_statistics_raised_in_the_window_deliver_after_the_restore() {
        let [(targets, during, after)] = raise_in_window(0, &[0x2e], 0x2f, IrqRoute::Every).try_into().unwrap();
        assert_eq!(targets, Some(vec![0]));
        assert!(!during && after);
    }

    #[test]
    fn display_timing_phases_raised_in_the_window_deliver_after_the_restore() {
        for cpu in 0..2 {
            for (targets, during, after) in raise_in_window(cpu, &[0x34, 0x35, 0x37, 0x38], 0x36, IrqRoute::AtMostOne) {
                assert_eq!(targets, Some(vec![cpu]));
                assert!(!during && after);
            }
        }
    }

    #[test]
    fn application_timer_expiry_in_the_window_delivers_after_the_restore() {
        for channel_line in [0x21, 0x22, 0x23] {
            let [(targets, during, after)] = raise_in_window(0, &[channel_line], 0x24, IrqRoute::Every).try_into().unwrap();
            assert_eq!(targets, Some(vec![0]));
            assert!(!during && after);
        }
    }

    #[test]
    fn control_bus_completion_in_the_window_routes_and_delivers_after_the_restore() {
        let [(targets, during, after)] = raise_in_window(1, &[0x65], 0x64, IrqRoute::ExactlyOne).try_into().unwrap();
        assert_eq!(targets, Some(vec![1]));
        assert!(!during && after);
    }

    #[test]
    fn an_unregistering_restore_discards_only_what_the_window_latched() {
        let (kept, dropped) = (at(0x40), at(0x41));
        let mut controller = Controller::default();
        controller.register(0, kept);
        controller.register(0, dropped);
        controller.banks[0] = controller.banks[0].raise(kept);
        controller.window(0, dropped, |saved| saved & !word_bit(dropped), |c| {
            assert_eq!(c.raise(dropped, IrqRoute::ExactlyOne), Some(vec![0]));
        });
        assert!(controller.deliverable(0, kept));
        assert!(!controller.deliverable(0, dropped));
        assert_eq!(controller.banks[0].pending & dropped.bit(), 0);
        assert_eq!(route(controller.banks, dropped, IrqRoute::ExactlyOne), None);
    }

    #[test]
    fn an_unregistered_line_keeps_a_request_pending_from_before_the_window() {
        let line = at(0x10);
        let mut controller = Controller::default();
        controller.register(0, line);
        controller.raise(line, IrqRoute::ExactlyOne);
        controller.window(0, line, |saved| saved & !word_bit(line), |c| {
            c.raise(line, IrqRoute::ExactlyOne);
        });
        assert_ne!(controller.banks[0].pending & line.bit(), 0);
        assert!(!controller.deliverable(0, line));
    }

    #[test]
    fn a_window_on_one_bank_leaves_the_other_bank_live() {
        let (local, remote) = (at(0x2c), at(0x2d));
        let mut controller = Controller::default();
        controller.register(0, local);
        controller.register(1, remote);
        controller.window(0, local, |saved| saved, |c| {
            c.raise(remote, IrqRoute::ExactlyOne);
            assert!(c.deliverable(1, remote));
        });
    }

    #[test]
    fn sources_without_an_owner_or_with_two_owners_still_refuse() {
        let line = at(0x34);
        let mut controller = Controller::default();
        assert_eq!(controller.raise(line, IrqRoute::ExactlyOne), None);
        assert_eq!(controller.raise(line, IrqRoute::AtMostOne), Some(vec![]));
        assert_eq!(controller.raise(line, IrqRoute::Every), Some(vec![]));
        controller.register(0, line);
        controller.register(1, line);
        controller.window(0, line, |saved| saved | word_bit(line), |c| {
            assert_eq!(c.raise(line, IrqRoute::ExactlyOne), None);
            assert_eq!(c.raise(line, IrqRoute::AtMostOne), None);
            assert_eq!(c.raise(line, IrqRoute::Every), Some(vec![0, 1]));
        });
    }

    #[test]
    fn out_of_range_lines_and_groups_refuse() {
        assert_eq!(Line::new(128), None);
        assert_eq!(Group::new(4), None);
    }
}

use std::vec::Vec;

use crate::bss::{begin, finish, Bss, FillPorts};

#[derive(Debug, Eq, PartialEq)]
enum Effect {
    ClearFlag,
    Request(u32, u32),
    CpuFill(u32, u32),
    Publish,
}

struct Ports {
    answer: i32,
    polls_until_set: usize,
    effects: Vec<Effect>,
}

impl Ports {
    fn new(answer: i32) -> Self { Self { answer, polls_until_set: 0, effects: Vec::new() } }
}

impl FillPorts for Ports {
    fn clear_flag(&mut self) { self.effects.push(Effect::ClearFlag) }
    fn request(&mut self, base: u32, words: u32) -> i32 {
        self.effects.push(Effect::Request(base, words));
        self.answer
    }
    fn flag_set(&mut self) -> bool {
        let set = self.polls_until_set == 0;
        self.polls_until_set = self.polls_until_set.saturating_sub(1);
        set
    }
    fn cpu_fill(&mut self, base: u32, bytes: u32) { self.effects.push(Effect::CpuFill(base, bytes)) }
    fn publish(&mut self) { self.effects.push(Effect::Publish) }
}

const IMAGE5: Bss = Bss { base: 0x0146_0000, bytes: 0x2b_be40 };

#[test]
fn an_accepted_request_is_awaited_and_the_cpu_never_fills() {
    let mut ports = Ports::new(0);
    ports.polls_until_set = 3;
    let fill = begin(IMAGE5, &mut ports);
    assert_eq!(ports.effects, [Effect::ClearFlag, Effect::Request(0x0146_0000, 0xa_ef90)]);
    finish(IMAGE5, fill, &mut ports);
    assert_eq!(ports.polls_until_set, 0);
    assert_eq!(ports.effects, [Effect::ClearFlag, Effect::Request(0x0146_0000, 0xa_ef90)]);
}

#[test]
fn a_rejected_request_falls_back_to_a_cpu_fill_and_cache_publication_after_the_load() {
    let mut ports = Ports::new(-1);
    let fill = begin(IMAGE5, &mut ports);
    assert_eq!(ports.effects, [Effect::ClearFlag, Effect::Request(0x0146_0000, 0xa_ef90)]);
    finish(IMAGE5, fill, &mut ports);
    assert_eq!(ports.effects, [
        Effect::ClearFlag,
        Effect::Request(0x0146_0000, 0xa_ef90),
        Effect::CpuFill(0x0146_0000, 0x2b_be40),
        Effect::Publish,
    ]);
}

#[test]
fn an_unaligned_extent_is_never_requested() {
    for bss in [Bss { base: 0x0146_0002, bytes: 0x40 }, Bss { base: 0x0146_0000, bytes: 0x41 }] {
        let mut ports = Ports::new(0);
        let fill = begin(bss, &mut ports);
        assert!(ports.effects.is_empty());
        finish(bss, fill, &mut ports);
        assert_eq!(ports.effects, [Effect::CpuFill(bss.base, bss.bytes), Effect::Publish]);
    }
}

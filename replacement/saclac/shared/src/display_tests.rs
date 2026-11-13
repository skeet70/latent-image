use crate::display::*;

struct Driver {
    route: u8,
    gate: bool,
    outputs: u8,
}

impl Display for Driver {
    fn route(&self) -> DisplayRoute { DisplayRoute(self.route) }

    fn gate_open(&self) -> bool { self.gate }

    fn outputs(&self) -> Outputs { Outputs(self.outputs) }
}

fn driver(route: u8, gate: bool, outputs: u8) -> Driver { Driver { route, gate, outputs } }

#[test]
fn route_zero_has_no_outputs() {
    assert_eq!(active_outputs(&driver(0, true, 0b1111)), Outputs(0));
}

#[test]
fn the_gated_route_needs_an_open_gate() {
    assert_eq!(active_outputs(&driver(2, false, 0b0010)), Outputs(0));
    assert_eq!(active_outputs(&driver(2, true, 0b0010)), Outputs(0b0010));
}

#[test]
fn every_other_route_passes_its_outputs() {
    for route in [1, 3, 0xff] {
        assert_eq!(active_outputs(&driver(route, false, 0b0101)), Outputs(0b0101), "{route}");
    }
}

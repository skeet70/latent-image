use crate::display::*;

struct Driver {
    route: u8,
    status: [u8; STATUS_BYTES],
}

impl Display for Driver {
    fn route(&self) -> DisplayRoute { DisplayRoute(self.route) }

    fn status(&self) -> DisplayStatus { DisplayStatus(self.status) }
}

fn driver(route: u8, set: &[usize]) -> Driver {
    let mut status = [0; STATUS_BYTES];
    for byte in set {
        status[*byte] = 0x5a;
    }
    Driver { route, status }
}

#[test]
fn each_nonzero_output_byte_sets_its_bit() {
    assert_eq!(active_outputs(&driver(1, &[16, 18])), Outputs(0b0101));
    assert_eq!(active_outputs(&driver(3, &[16, 17, 18, 19])), Outputs(0b1111));
    assert_eq!(active_outputs(&driver(1, &[15, 20, 7])), Outputs(0));
}

#[test]
fn route_zero_has_no_outputs() {
    assert_eq!(active_outputs(&driver(0, &[16, 17, 18, 19])), Outputs(0));
}

#[test]
fn the_gated_route_needs_status_byte_seven() {
    assert_eq!(active_outputs(&driver(2, &[17])), Outputs(0));
    assert_eq!(active_outputs(&driver(2, &[7, 17])), Outputs(0b0010));
}

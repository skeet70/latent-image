use crate::geometry::*;

const fn rectangle(left: u32, top: u32, height: u32, width: u32) -> Rectangle { Rectangle { left, top, height, width } }

#[test]
fn percent_follows_the_longer_side() {
    let frame = Frame { vertical: 120, horizontal: 200 };
    assert_eq!(rectangle(0, 0, 30, 50).percent_of(frame), Some(25));
    assert_eq!(rectangle(0, 0, 60, 50).percent_of(frame), Some(50));
    assert_eq!(rectangle(0, 0, 40, 40).percent_of(frame), Some(20));
    assert_eq!(rectangle(0, 0, 40, 40).percent_of(Frame { vertical: 120, horizontal: 0 }), None);
}

#[test]
fn clip_cuts_each_edge_against_the_original_extent() {
    let mut cut = rectangle(10, 20, 100, 80);
    cut.clip(Point { left: 30, top: 0 }, Point { left: 70, top: 90 });
    assert_eq!(cut, rectangle(30, 20, 70, 40));
}

#[test]
fn rescale_moves_between_frames_and_recentres() {
    let placement = Placement { rectangle: rectangle(100, 50, 40, 60), centre: Point { left: 1, top: 1 } };
    let from = Frame { vertical: 200, horizontal: 400 };
    let to = Frame { vertical: 100, horizontal: 800 };
    assert_eq!(rescale(Some(placement), from, to), Some(Placement { rectangle: rectangle(200, 25, 20, 120), centre: Point { left: 260, top: 35 } }));
    assert_eq!(rescale(None, from, to), Some(Placement::default()));
    assert_eq!(rescale(Some(placement), Frame { vertical: 0, horizontal: 400 }, to), None);
}

struct Rule(ClipRule, u32);

impl ClipSettings for Rule {
    fn clip_rule(&mut self) -> ClipRule { self.0 }
    fn minimum_visible_percent(&mut self) -> u32 { self.1 }
}

fn region(rectangle: Rectangle, centre: Point) -> Region { Region { placement: Placement { rectangle, centre }, scale: 1000 } }

#[test]
fn clipped_region_scales_with_its_width_and_recentres() {
    let mut clipped = region(rectangle(10, 10, 20, 40), Point { left: 5, top: 5 });
    let frame = Frame { vertical: 100, horizontal: 100 };
    assert!(clip_region(&mut clipped, Point { left: 0, top: 0 }, Point { left: 30, top: 100 }, frame, &mut Rule(ClipRule::Overlap, 0)));
    assert_eq!(clipped, Region { placement: Placement { rectangle: rectangle(10, 10, 20, 20), centre: Point { left: 20, top: 20 } }, scale: 500 });
}

#[test]
fn regions_outside_the_bounds_or_with_the_centre_outside_are_refused_unchanged() {
    let frame = Frame { vertical: 100, horizontal: 100 };
    let minimum = Point { left: 0, top: 0 };
    let maximum = Point { left: 30, top: 100 };
    let outside = region(rectangle(50, 10, 20, 40), Point { left: 70, top: 20 });
    let mut kept = outside;
    assert!(!clip_region(&mut kept, minimum, maximum, frame, &mut Rule(ClipRule::Overlap, 0)));
    assert_eq!(kept, outside);

    let centre_on_edge = region(rectangle(10, 10, 20, 40), Point { left: 30, top: 20 });
    let mut kept = centre_on_edge;
    assert!(!clip_region(&mut kept, minimum, maximum, frame, &mut Rule(ClipRule::CentreInside, 0)));
    assert_eq!(kept, centre_on_edge);
    assert!(clip_region(&mut kept, minimum, maximum, frame, &mut Rule(ClipRule::Overlap, 0)));
}

#[test]
fn visible_share_compares_the_clipped_area_with_the_frame() {
    let frame = Frame { vertical: 10, horizontal: 10 };
    let bounds = (Point { left: 0, top: 0 }, Point { left: 8, top: 100 });
    let mut enough = region(rectangle(0, 0, 5, 20), Point { left: 4, top: 2 });
    assert!(clip_region(&mut enough, bounds.0, bounds.1, frame, &mut Rule(ClipRule::VisibleShare, 40)));
    assert_eq!(enough.placement.rectangle, rectangle(0, 0, 5, 8));
    let mut too_little = region(rectangle(0, 0, 5, 20), Point { left: 4, top: 2 });
    assert!(!clip_region(&mut too_little, bounds.0, bounds.1, frame, &mut Rule(ClipRule::VisibleShare, 41)));
}

#[test]
fn zero_width_region_is_refused() {
    let mut empty = region(rectangle(10, 10, 20, 0), Point { left: 10, top: 20 });
    assert!(!clip_region(&mut empty, Point { left: 0, top: 0 }, Point { left: 30, top: 100 }, Frame { vertical: 1, horizontal: 1 }, &mut Rule(ClipRule::Overlap, 0)));
}

#[test]
fn grid_cells_count_whole_steps_and_end_at_the_last_cell() {
    let grid = Grid { origin: Point { left: 10, top: 20 }, cells: 4, cell_width: 8, cell_height: 6, gap: Frame { vertical: 2, horizontal: 1 } };
    assert_eq!(grid.cell_at(Point { left: 10, top: 20 }), Cell { column: Some(0), row: Some(0) });
    assert_eq!(grid.cell_at(Point { left: 19, top: 35 }), Cell { column: Some(1), row: Some(1) });
    assert_eq!(grid.cell_at(Point { left: 18, top: 27 }), Cell { column: Some(0), row: Some(0) });
    assert_eq!(grid.cell_at(Point { left: 200, top: 300 }), Cell { column: Some(3), row: Some(3) });
    assert_eq!(grid.cell_at(Point { left: 9, top: 20 }), Cell { column: None, row: Some(0) });
    assert_eq!(Grid { cells: 0, ..grid }.cell_at(Point { left: 10, top: 20 }), Cell { column: None, row: None });
}

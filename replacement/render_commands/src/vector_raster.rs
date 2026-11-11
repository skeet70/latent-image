use super::shape_assets::{CoordinateMode, FillRule, LineCap, LineJoin, NaturalPlacement, Path, SegmentKind, StrokeDash};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VectorError { WorkspaceTooSmall, InvalidStroke, NonFiniteGeometry, SubdivisionLimit, DashBudgetExceeded }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurveAccuracy(f64);
impl CurveAccuracy {
    pub fn new(max_chord_error: f32) -> Option<Self> {
        if max_chord_error.is_finite() && max_chord_error > 0.0 { Some(Self(max_chord_error as f64)) } else { None }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SampleGrid { One, Two, Four, Eight }
impl SampleGrid {
    fn size(self) -> i32 { match self { Self::One => 1, Self::Two => 2, Self::Four => 4, Self::Eight => 8 } }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipBounds { left: i32, top: i32, right: i32, bottom: i32 }
impl ClipBounds {
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Option<Self> {
        if left <= right && top <= bottom { Some(Self { left, top, right, bottom }) } else { None }
    }
}

pub trait CoverageSink { fn cover(&mut self, x: i32, y: i32, coverage: u8); }

#[derive(Clone, Copy, Debug, PartialEq)]
struct Point { x: f64, y: f64 }
impl Point {
    const ZERO: Self = Self { x: 0.0, y: 0.0 };
    fn add(self, other: Self) -> Self { Self { x: self.x + other.x, y: self.y + other.y } }
    fn sub(self, other: Self) -> Self { Self { x: self.x - other.x, y: self.y - other.y } }
    fn scale(self, factor: f64) -> Self { Self { x: self.x * factor, y: self.y * factor } }
    fn dot(self, other: Self) -> f64 { self.x * other.x + self.y * other.y }
    fn cross(self, other: Self) -> f64 { self.x * other.y - self.y * other.x }
    fn length(self) -> f64 { libm::sqrt(self.dot(self)) }
    fn normal(self) -> Self { Self { x: -self.y, y: self.x } }
    fn finite(self) -> bool { self.x.is_finite() && self.y.is_finite() }
}

fn bounded_clip(clip: ClipBounds, points: impl Iterator<Item = Point>) -> ClipBounds {
    let mut left = f64::INFINITY;
    let mut top = f64::INFINITY;
    let mut right = f64::NEG_INFINITY;
    let mut bottom = f64::NEG_INFINITY;
    for point in points {
        left = left.min(point.x);
        top = top.min(point.y);
        right = right.max(point.x);
        bottom = bottom.max(point.y);
    }
    ClipBounds {
        left: clip.left.max(libm::floor(left) as i32),
        top: clip.top.max(libm::floor(top) as i32),
        right: clip.right.min(libm::ceil(right) as i32),
        bottom: clip.bottom.min(libm::ceil(bottom) as i32),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex { point: Point, begins: bool, closes: bool }
impl Vertex { pub const EMPTY: Self = Self { point: Point::ZERO, begins: false, closes: false }; }

#[derive(Clone, Copy, Debug)]
pub struct FlattenedPath<'a> { vertices: &'a [Vertex] }

struct Vertices<'a> { storage: &'a mut [Vertex], used: usize }
impl Vertices<'_> {
    fn push(&mut self, point: Point, begins: bool, closes: bool) -> Result<(), VectorError> {
        if !point.finite() { return Err(VectorError::NonFiniteGeometry); }
        let slot = self.storage.get_mut(self.used).ok_or(VectorError::WorkspaceTooSmall)?;
        *slot = Vertex { point, begins, closes };
        self.used += 1;
        Ok(())
    }
    fn reserve_steps(&self, bound: f64, accuracy: CurveAccuracy) -> Result<usize, VectorError> {
        let count = libm::ceil(libm::sqrt(bound / (8.0 * accuracy.0))).max(1.0);
        if !count.is_finite() || count > usize::MAX as f64 { return Err(VectorError::SubdivisionLimit); }
        let count = count as usize;
        if count > self.storage.len() - self.used { return Err(VectorError::WorkspaceTooSmall); }
        Ok(count)
    }
    fn quadratic(&mut self, a: Point, b: Point, c: Point, accuracy: CurveAccuracy) -> Result<(), VectorError> {
        let steps = self.reserve_steps(a.sub(b.scale(2.0)).add(c).length() * 2.0, accuracy)?;
        for i in 1..=steps {
            let t = i as f64 / steps as f64;
            let u = 1.0 - t;
            self.push(a.scale(u * u).add(b.scale(2.0 * u * t)).add(c.scale(t * t)), false, false)?;
        }
        Ok(())
    }
    fn cubic(&mut self, a: Point, b: Point, c: Point, d: Point, accuracy: CurveAccuracy) -> Result<(), VectorError> {
        // A cubic's second derivative is the linear interpolation of these endpoint vectors.
        let bound = a.sub(b.scale(2.0)).add(c).length().max(b.sub(c.scale(2.0)).add(d).length()) * 6.0;
        let steps = self.reserve_steps(bound, accuracy)?;
        for i in 1..=steps {
            let t = i as f64 / steps as f64;
            let u = 1.0 - t;
            self.push(a.scale(u * u * u).add(b.scale(3.0 * u * u * t)).add(c.scale(3.0 * u * t * t)).add(d.scale(t * t * t)), false, false)?;
        }
        Ok(())
    }
    fn arc(&mut self, start: Point, end: Point, radii: Point, rotation: f64, large: bool, ccw: bool, accuracy: CurveAccuracy) -> Result<(), VectorError> {
        let mut rx = radii.x.abs();
        let mut ry = radii.y.abs();
        if start == end { return Ok(()); }
        if rx == 0.0 || ry == 0.0 { return self.push(end, false, false); }
        let angle = rotation * core::f64::consts::PI / 180.0;
        let cosine = libm::cos(angle);
        let sine = libm::sin(angle);
        let delta = start.sub(end).scale(0.5);
        let local = Point { x: cosine * delta.x + sine * delta.y, y: -sine * delta.x + cosine * delta.y };
        let scale = libm::sqrt((local.x / rx) * (local.x / rx) + (local.y / ry) * (local.y / ry));
        if scale > 1.0 { rx *= scale; ry *= scale; }
        let normalized = Point { x: local.x / rx, y: local.y / ry };
        let denominator = normalized.dot(normalized);
        let sign = if large == ccw { -1.0 } else { 1.0 };
        let factor = sign * libm::sqrt(((1.0 - denominator) / denominator).max(0.0));
        let local_center = Point { x: factor * rx * normalized.y, y: -factor * ry * normalized.x };
        let center = Point { x: cosine * local_center.x - sine * local_center.y, y: sine * local_center.x + cosine * local_center.y }.add(start.add(end).scale(0.5));
        let first = Point { x: (local.x - local_center.x) / rx, y: (local.y - local_center.y) / ry };
        let last = Point { x: (-local.x - local_center.x) / rx, y: (-local.y - local_center.y) / ry };
        let initial = libm::atan2(first.y, first.x);
        let mut sweep = libm::atan2(first.cross(last), first.dot(last));
        if ccw && sweep < 0.0 { sweep += core::f64::consts::TAU; }
        if !ccw && sweep > 0.0 { sweep -= core::f64::consts::TAU; }
        let steps = self.reserve_steps(rx.max(ry) * sweep * sweep, accuracy)?;
        for i in 1..=steps {
            if i == steps { self.push(end, false, false)?; continue; }
            let t = initial + sweep * (i as f64 / steps as f64);
            let x = rx * libm::cos(t);
            let y = ry * libm::sin(t);
            self.push(center.add(Point { x: cosine * x - sine * y, y: sine * x + cosine * y }), false, false)?;
        }
        Ok(())
    }
}

/// The error bound is in path coordinates. Sampling uses pixel-centred uniform grids.
pub fn flatten_path<'a>(path: Path<'_>, workspace: &'a mut [Vertex], accuracy: CurveAccuracy) -> Result<FlattenedPath<'a>, VectorError> {
    flatten(path, workspace, accuracy, None)
}

pub fn flatten_path_placed<'a>(path: Path<'_>, workspace: &'a mut [Vertex], accuracy: CurveAccuracy, placement: NaturalPlacement) -> Result<FlattenedPath<'a>, VectorError> {
    flatten(path, workspace, accuracy, Some(placement))
}

fn flatten<'a>(path: Path<'_>, workspace: &'a mut [Vertex], accuracy: CurveAccuracy, placement: Option<NaturalPlacement>) -> Result<FlattenedPath<'a>, VectorError> {
    let mut output = Vertices { storage: workspace, used: 0 };
    let mut current = Point::ZERO;
    let mut start = Point::ZERO;
    let mut quadratic_control = None;
    let mut cubic_control = None;
    let mut closed = false;
    for segment in path.segments() {
        let number = |index| segment.coordinates.get(index).unwrap().value() as f64;
        let point = |index| {
            let value = Point { x: number(index), y: number(index + 1) };
            if segment.mode == CoordinateMode::Relative { value.add(current) } else { value }
        };
        if segment.kind != SegmentKind::Move && segment.kind != SegmentKind::Close && (output.used == 0 || closed) {
            output.push(current, true, false)?;
            start = current;
            closed = false;
        }
        let mut next_quadratic = None;
        let mut next_cubic = None;
        match segment.kind {
            SegmentKind::Move => { current = point(0); start = current; output.push(current, true, false)?; closed = false; }
            SegmentKind::Close => {
                if output.used != 0 && !closed { output.push(start, false, true)?; current = start; closed = true; }
            }
            SegmentKind::Line => { current = point(0); output.push(current, false, false)?; }
            SegmentKind::Horizontal => { current.x = number(0) + if segment.mode == CoordinateMode::Relative { current.x } else { 0.0 }; output.push(current, false, false)?; }
            SegmentKind::Vertical => { current.y = number(0) + if segment.mode == CoordinateMode::Relative { current.y } else { 0.0 }; output.push(current, false, false)?; }
            SegmentKind::Quadratic | SegmentKind::SmoothQuadratic => {
                let (control, end) = if segment.kind == SegmentKind::Quadratic { (point(0), point(2)) } else { (quadratic_control.map_or(current, |p: Point| current.scale(2.0).sub(p)), point(0)) };
                output.quadratic(current, control, end, accuracy)?;
                current = end;
                next_quadratic = Some(control);
            }
            SegmentKind::Cubic | SegmentKind::SmoothCubic => {
                let (first, second, end) = if segment.kind == SegmentKind::Cubic { (point(0), point(2), point(4)) } else { (cubic_control.map_or(current, |p: Point| current.scale(2.0).sub(p)), point(0), point(2)) };
                output.cubic(current, first, second, end, accuracy)?;
                current = end;
                next_cubic = Some(second);
            }
            SegmentKind::SmallCounterClockwiseArc | SegmentKind::SmallClockwiseArc | SegmentKind::LargeCounterClockwiseArc | SegmentKind::LargeClockwiseArc => {
                let end = point(3);
                output.arc(current, end, Point { x: number(0), y: number(1) }, number(2), matches!(segment.kind, SegmentKind::LargeCounterClockwiseArc | SegmentKind::LargeClockwiseArc), matches!(segment.kind, SegmentKind::SmallCounterClockwiseArc | SegmentKind::LargeCounterClockwiseArc), accuracy)?;
                current = end;
            }
        }
        quadratic_control = next_quadratic;
        cubic_control = next_cubic;
    }
    if let Some(placement) = placement {
        for vertex in &mut output.storage[..output.used] {
            vertex.point = Point { x: placement.map_x(vertex.point.x), y: placement.map_y(vertex.point.y) };
        }
    }
    Ok(FlattenedPath { vertices: &output.storage[..output.used] })
}

fn sample(clip: ClipBounds, grid: SampleGrid, sink: &mut impl CoverageSink, mut contains: impl FnMut(Point) -> bool) {
    let size = grid.size();
    for y in clip.top..clip.bottom {
        for x in clip.left..clip.right {
            let mut hits = 0;
            for sy in 0..size { for sx in 0..size {
                let point = Point { x: x as f64 + (sx as f64 + 0.5) / size as f64, y: y as f64 + (sy as f64 + 0.5) / size as f64 };
                hits += i32::from(contains(point));
            } }
            if hits != 0 { sink.cover(x, y, ((hits * 255 + size * size / 2) / (size * size)) as u8); }
        }
    }
}

fn crossing(a: Point, b: Point, point: Point) -> i32 {
    if a.y <= point.y && point.y < b.y {
        if b.sub(a).cross(point.sub(a)) > 0.0 { 1 } else { 0 }
    } else if b.y <= point.y && point.y < a.y {
        if b.sub(a).cross(point.sub(a)) < 0.0 { -1 } else { 0 }
    } else { 0 }
}

pub fn fill(path: FlattenedPath<'_>, rule: FillRule, clip: ClipBounds, grid: SampleGrid, sink: &mut impl CoverageSink) {
    let clip = bounded_clip(clip, path.vertices.iter().map(|vertex| vertex.point));
    sample(clip, grid, sink, |point| {
        let mut winding = 0i64;
        let mut first = None;
        let mut previous = None;
        for vertex in path.vertices {
            if vertex.begins {
                if let (Some(last), Some(start)) = (previous, first) { winding += crossing(last, start, point) as i64; }
                first = Some(vertex.point);
            } else if let Some(last) = previous { winding += crossing(last, vertex.point, point) as i64; }
            previous = Some(vertex.point);
        }
        if let (Some(last), Some(start)) = (previous, first) { winding += crossing(last, start, point) as i64; }
        match rule { FillRule::EvenOdd => winding % 2 != 0, FillRule::NonZero => winding != 0 }
    });
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrokeMetrics { radius: f64, miter_limit: f64 }
impl StrokeMetrics {
    pub fn new(width: f32, miter_limit: f32) -> Result<Self, VectorError> {
        if !width.is_finite() || width < 0.0 || !miter_limit.is_finite() || miter_limit < 1.0 { return Err(VectorError::InvalidStroke); }
        Ok(Self { radius: width as f64 * 0.5, miter_limit: miter_limit as f64 })
    }
}
#[derive(Clone, Copy, Debug)]
pub struct StrokeStyle<'a> { pub metrics: StrokeMetrics, pub cap: LineCap, pub join: LineJoin, pub dash: Option<StrokeDash<'a>> }

#[derive(Clone, Copy, Debug)]
pub struct StrokePrimitive(Primitive);
impl StrokePrimitive { pub const EMPTY: Self = Self(Primitive::Empty); }
#[derive(Clone, Copy, Debug)]
enum Primitive {
    Empty, Triangle([Point; 3]), Quad([Point; 4]), Disk { center: Point, radius: f64 },
    HalfDisk { center: Point, radius: f64, direction: Point },
}
impl Primitive {
    fn extent(self) -> [Point; 4] {
        match self {
            Self::Empty => [Point::ZERO; 4],
            Self::Triangle([a, b, c]) => [a, b, c, a],
            Self::Quad(points) => points,
            Self::Disk { center, radius } | Self::HalfDisk { center, radius, .. } => [
                center.add(Point { x: -radius, y: -radius }),
                center.add(Point { x: radius, y: -radius }),
                center.add(Point { x: radius, y: radius }),
                center.add(Point { x: -radius, y: radius }),
            ],
        }
    }
    fn contains(self, point: Point) -> bool {
        match self {
            Self::Empty => false,
            Self::Disk { center, radius } => { let d = point.sub(center); d.dot(d) <= radius * radius }
            Self::HalfDisk { center, radius, direction } => {
                let d = point.sub(center);
                d.dot(direction) >= 0.0 && d.dot(d) <= radius * radius
            }
            Self::Triangle(vertices) => polygon_contains(&vertices, point),
            Self::Quad(vertices) => polygon_contains(&vertices, point),
        }
    }
}
fn polygon_contains(vertices: &[Point], point: Point) -> bool {
    let mut positive = false;
    let mut negative = false;
    let mut area = 0.0;
    for i in 0..vertices.len() {
        let a = vertices[i];
        let b = vertices[(i + 1) % vertices.len()];
        area += a.cross(b);
        let side = b.sub(a).cross(point.sub(a));
        positive |= side > 0.0;
        negative |= side < 0.0;
    }
    area != 0.0 && !(positive && negative)
}
struct Primitives<'a> { storage: &'a mut [StrokePrimitive], used: usize }
impl Primitives<'_> {
    fn push(&mut self, primitive: Primitive) -> Result<(), VectorError> {
        let slot = self.storage.get_mut(self.used).ok_or(VectorError::WorkspaceTooSmall)?;
        *slot = StrokePrimitive(primitive);
        self.used += 1;
        Ok(())
    }
    fn cap(&mut self, at: Point, direction: Point, radius: f64, cap: LineCap) -> Result<(), VectorError> {
        match cap {
            LineCap::Butt => Ok(()),
            LineCap::Round => self.push(Primitive::HalfDisk { center: at, radius, direction }),
            LineCap::Square => {
                let normal = direction.normal().scale(radius);
                let end = at.add(direction.scale(radius));
                self.push(Primitive::Quad([at.add(normal), end.add(normal), end.sub(normal), at.sub(normal)]))
            }
        }
    }
    fn join(&mut self, at: Point, incoming: Point, outgoing: Point, radius: f64, style: StrokeStyle<'_>) -> Result<(), VectorError> {
        let turn = incoming.cross(outgoing);
        if turn == 0.0 {
            if incoming.dot(outgoing) < 0.0 && style.join == LineJoin::Round { self.push(Primitive::Disk { center: at, radius })?; }
            return Ok(());
        }
        if style.join == LineJoin::Round { return self.push(Primitive::Disk { center: at, radius }); }
        let side = if turn > 0.0 { -radius } else { radius };
        let first = incoming.normal().scale(side);
        let last = outgoing.normal().scale(side);
        let denominator = 1.0 + incoming.dot(outgoing);
        if style.join == LineJoin::Miter && denominator > 0.0 {
            let offset = first.add(last).scale(1.0 / denominator);
            if offset.length() <= radius * style.metrics.miter_limit {
                return self.push(Primitive::Quad([at, at.add(first), at.add(offset), at.add(last)]));
            }
        }
        self.push(Primitive::Triangle([at, at.add(first), at.add(last)]))
    }
}

#[derive(Clone, Copy)]
struct Run { start: Point, end: Point, direction: Point }
struct StrokeBuilder<'a> {
    output: Primitives<'a>, first: Option<Run>, previous: Option<Run>, degenerate: Option<Point>,
    first_dash_boundary: bool, last_dash_boundary: bool, style: StrokeStyle<'a>, radius: f64,
}
impl StrokeBuilder<'_> {
    fn dot(&mut self, at: Point, incoming: Point, outgoing: Point) -> Result<(), VectorError> {
        if self.style.cap == LineCap::Butt { return Ok(()); }
        self.output.cap(at, incoming.scale(-1.0), self.radius, self.style.cap)?;
        self.output.cap(at, outgoing, self.radius, self.style.cap)?;
        self.output.join(at, incoming, outgoing, self.radius, self.style)
    }
    fn run(&mut self, start: Point, end: Point, begins_dash: bool) -> Result<(), VectorError> {
        let delta = end.sub(start);
        let length = delta.length();
        if length == 0.0 {
            if self.first.is_none() { self.degenerate = Some(start); }
            return Ok(());
        }
        self.degenerate = None;
        let run = Run { start, end, direction: delta.scale(1.0 / length) };
        let normal = run.direction.normal().scale(self.radius);
        self.output.push(Primitive::Quad([start.add(normal), end.add(normal), end.sub(normal), start.sub(normal)]))?;
        if let Some(previous) = self.previous {
            if previous.end == start && !begins_dash {
                self.output.join(start, previous.direction, run.direction, self.radius, self.style)?;
            } else {
                self.output.cap(previous.end, previous.direction, self.radius, self.style.cap)?;
                self.output.cap(start, run.direction.scale(-1.0), self.radius, self.style.cap)?;
            }
        } else {
            self.first = Some(run);
            self.first_dash_boundary = begins_dash;
        }
        self.last_dash_boundary = false;
        self.previous = Some(run);
        Ok(())
    }
    fn finish(&mut self, closed: bool) -> Result<(), VectorError> {
        if let (Some(first), Some(last)) = (self.first, self.previous) {
            if closed && last.end == first.start && !self.first_dash_boundary && !self.last_dash_boundary {
                self.output.join(first.start, last.direction, first.direction, self.radius, self.style)?;
            } else {
                self.output.cap(first.start, first.direction.scale(-1.0), self.radius, self.style.cap)?;
                self.output.cap(last.end, last.direction, self.radius, self.style.cap)?;
            }
        }
        if self.first.is_none() {
            if let Some(point) = self.degenerate {
                let tangent = Point { x: 1.0, y: 0.0 };
                self.dot(point, tangent, tangent)?;
            }
        }
        self.degenerate = None;
        self.first = None;
        self.previous = None;
        Ok(())
    }
}

/// Counts on/off edge fragments and zero-length pattern intervals, independently of emitted primitives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DashBudget(usize);
impl DashBudget {
    pub const fn new(max_intervals: usize) -> Self { Self(max_intervals) }
    fn consume(&mut self) -> Result<(), VectorError> {
        self.0 = self.0.checked_sub(1).ok_or(VectorError::DashBudgetExceeded)?;
        Ok(())
    }
}

struct DashPattern { lengths: [f64; 6], count: usize, index: usize, remaining: f64, invisible: bool, begins_dash: bool }
impl DashPattern {
    fn new(dash: Option<StrokeDash<'_>>) -> Result<Self, VectorError> {
        let mut result = Self { lengths: [0.0; 6], count: 0, index: 0, remaining: 0.0, invisible: false, begins_dash: false };
        let Some(dash) = dash else { return Ok(result); };
        if dash.lengths.len() > 6 { return Err(VectorError::InvalidStroke); }
        // OpenVG 1.1, section 8.7.5: an odd dash array ignores its final element.
        result.count = dash.lengths.len() & !1;
        if result.count == 0 { return Ok(result); }
        let mut total = 0.0;
        let mut on = 0.0;
        for index in 0..result.count {
            let length = (dash.lengths.get(index).unwrap().value() as f64).max(0.0);
            result.lengths[index] = length;
            total += length;
            if index % 2 == 0 { on += length; }
        }
        if total == 0.0 { result.count = 0; return Ok(result); }
        result.invisible = on == 0.0;
        let mut phase = (dash.phase.value() as f64) % total;
        if phase < 0.0 { phase += total; }
        for _ in 0..result.count {
            let length = result.lengths[result.index];
            if phase == 0.0 || phase < length {
                result.remaining = length - phase;
                result.begins_dash = phase == 0.0;
                return Ok(result);
            }
            phase -= length;
            result.index = (result.index + 1) % result.count;
        }
        Err(VectorError::InvalidStroke)
    }
    fn advance(&mut self) {
        self.index = (self.index + 1) % self.count;
        self.remaining = self.lengths[self.index];
        self.begins_dash = true;
    }
    fn flush_zeros(&mut self, at: Point, incoming: Point, outgoing: Point, builder: &mut StrokeBuilder<'_>, budget: &mut DashBudget) -> Result<(), VectorError> {
        if self.count == 0 || (self.invisible && builder.style.cap == LineCap::Butt) { return Ok(()); }
        while self.remaining == 0.0 {
            budget.consume()?;
            if self.index % 2 == 0 { builder.dot(at, incoming, outgoing)?; }
            self.advance();
        }
        Ok(())
    }
    fn terminal_zeros(&mut self, at: Point, incoming: Point, outgoing: Point, builder: &mut StrokeBuilder<'_>, budget: &mut DashBudget) -> Result<(), VectorError> {
        let index = self.index;
        let remaining = self.remaining;
        let begins_dash = self.begins_dash;
        let result = self.flush_zeros(at, incoming, outgoing, builder, budget);
        self.index = index;
        self.remaining = remaining;
        self.begins_dash = begins_dash;
        result
    }
    fn edge(&mut self, a: Point, b: Point, incoming: Option<Point>, builder: &mut StrokeBuilder<'_>, budget: &mut DashBudget) -> Result<(), VectorError> {
        if self.count == 0 { return builder.run(a, b, false); }
        if self.invisible && builder.style.cap == LineCap::Butt { return Ok(()); }
        let delta = b.sub(a);
        let length = delta.length();
        if length == 0.0 { return Ok(()); }
        let direction = delta.scale(1.0 / length);
        let mut offset = 0.0;
        self.flush_zeros(a, incoming.unwrap_or(direction), direction, builder, budget)?;
        while offset < length {
            budget.consume()?;
            let step = self.remaining.min(length - offset);
            let next = offset + step;
            if next == offset { return Err(VectorError::SubdivisionLimit); }
            let start = if offset == 0.0 { a } else { a.add(direction.scale(offset)) };
            let end = if next == length { b } else { a.add(direction.scale(next)) };
            if start == end { return Err(VectorError::SubdivisionLimit); }
            if self.index % 2 == 0 {
                builder.run(start, end, self.begins_dash)?;
                self.begins_dash = false;
            }
            offset = next;
            self.remaining -= step;
            if self.remaining <= 0.0 {
                if self.index % 2 == 0 { builder.last_dash_boundary = true; }
                self.advance();
            }
            if offset < length { self.flush_zeros(end, direction, direction, builder, budget)?; }
        }
        Ok(())
    }
}

fn direction(a: Point, b: Point) -> Option<Point> {
    let delta = b.sub(a);
    let length = delta.length();
    if length == 0.0 { None } else { Some(delta.scale(1.0 / length)) }
}

fn closing_direction(vertices: &[Vertex]) -> Option<Point> {
    let mut previous = vertices.first()?.point;
    let mut last = None;
    let mut closed = false;
    for vertex in vertices.iter().skip(1) {
        if vertex.begins { break; }
        if let Some(tangent) = direction(previous, vertex.point) { last = Some(tangent); }
        previous = vertex.point;
        closed = vertex.closes;
    }
    if closed { last } else { None }
}

pub fn stroke(path: FlattenedPath<'_>, style: StrokeStyle<'_>, workspace: &mut [StrokePrimitive], mut budget: DashBudget, clip: ClipBounds, grid: SampleGrid, sink: &mut impl CoverageSink) -> Result<(), VectorError> {
    let mut dash = DashPattern::new(style.dash)?;
    if style.metrics.radius == 0.0 || (dash.invisible && style.cap == LineCap::Butt) { return Ok(()); }
    let mut builder = StrokeBuilder {
        output: Primitives { storage: workspace, used: 0 }, first: None, previous: None, degenerate: None,
        first_dash_boundary: false, last_dash_boundary: false, style, radius: style.metrics.radius,
    };
    let mut previous = None;
    let mut incoming = None;
    let mut first_direction = None;
    let mut closed = false;
    for (index, vertex) in path.vertices.iter().enumerate() {
        if vertex.begins {
            if let (Some(at), Some(tangent)) = (previous, incoming) {
                dash.terminal_zeros(at, tangent, if closed { first_direction.unwrap_or(tangent) } else { tangent }, &mut builder, &mut budget)?;
            }
            builder.finish(closed)?;
            closed = false;
            incoming = closing_direction(&path.vertices[index..]);
            first_direction = None;
        } else if let Some(last) = previous {
            dash.edge(last, vertex.point, incoming, &mut builder, &mut budget)?;
            if let Some(tangent) = direction(last, vertex.point) {
                incoming = Some(tangent);
                if first_direction.is_none() { first_direction = Some(tangent); }
            }
            closed = vertex.closes;
        }
        previous = Some(vertex.point);
    }
    if let (Some(at), Some(tangent)) = (previous, incoming) {
        dash.terminal_zeros(at, tangent, if closed { first_direction.unwrap_or(tangent) } else { tangent }, &mut builder, &mut budget)?;
    }
    builder.finish(closed)?;
    let primitives = &builder.output.storage[..builder.output.used];
    let clip = bounded_clip(clip, primitives.iter().flat_map(|primitive| primitive.0.extent()));
    sample(clip, grid, sink, |point| primitives.iter().any(|primitive| primitive.0.contains(point)));
    Ok(())
}

#[cfg(test)]
#[path = "vector_raster_tests.rs"]
mod tests;

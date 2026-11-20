//! The per-sample coverage rule: every grid sample of every pixel in the bounded clip is tested on its own. The row
//! sweep in `vector_raster` must cover exactly the same pixels with exactly the same coverage.

use super::{bounded_clip, stroke_primitives, ClipBounds, CoverageSink, DashBudget, FillRule, FlattenedPath, Point, Primitive, SampleGrid, Scanlines, ShapePlacement, StrokePrimitive, StrokeStyle, VectorError};

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

pub fn fill(path: FlattenedPath<'_>, rule: FillRule, clip: ClipBounds, grid: SampleGrid, _scanlines: &mut Scanlines<'_>, sink: &mut impl CoverageSink) -> Result<(), VectorError> {
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
    Ok(())
}

fn contains(primitive: Primitive, point: Point) -> bool {
    match primitive {
        Primitive::Empty => false,
        Primitive::Disk { center, radius } => { let d = point.sub(center); d.dot(d) <= radius * radius }
        Primitive::HalfDisk { center, radius, direction } => {
            let d = point.sub(center);
            d.dot(direction) >= 0.0 && d.dot(d) <= radius * radius
        }
        Primitive::Triangle(vertices) => polygon_contains(&vertices, point),
        Primitive::Quad(vertices) => polygon_contains(&vertices, point),
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

pub fn stroke(path: FlattenedPath<'_>, style: StrokeStyle<'_>, workspace: &mut [StrokePrimitive], budget: DashBudget, clip: ClipBounds, grid: SampleGrid, _scanlines: &mut Scanlines<'_>, sink: &mut impl CoverageSink) -> Result<(), VectorError> {
    let used = stroke_primitives(path, style, workspace, budget)?;
    if used == 0 { return Ok(()); }
    let primitives = &workspace[..used];
    let clip = bounded_clip(clip, primitives.iter().flat_map(|primitive| primitive.0.extent()));
    sample(clip, grid, sink, |point| primitives.iter().any(|primitive| contains(primitive.0, point)));
    Ok(())
}

pub fn stroke_placed(path: FlattenedPath<'_>, style: StrokeStyle<'_>, workspace: &mut [StrokePrimitive], budget: DashBudget, placement: ShapePlacement, clip: ClipBounds, grid: SampleGrid, _scanlines: &mut Scanlines<'_>, sink: &mut impl CoverageSink) -> Result<(), VectorError> {
    let used = stroke_primitives(path, style, workspace, budget)?;
    if used == 0 { return Ok(()); }
    let primitives = &workspace[..used];
    let extents = primitives.iter().flat_map(|primitive| primitive.0.extent()).map(|point| Point { x: placement.map_x(point.x), y: placement.map_y(point.y) });
    let clip = bounded_clip(clip, extents);
    sample(clip, grid, sink, |point| {
        let local = Point { x: placement.unmap_x(point.x), y: placement.unmap_y(point.y) };
        primitives.iter().any(|primitive| contains(primitive.0, local))
    });
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Coordinate(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Row(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RowCount(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Dimension(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Expansion(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Shear(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Stride(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BitmapOffset(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Span { pub first: Coordinate, pub last: Coordinate }
impl Span {
    pub const fn new(first: i32,last: i32) -> Self { Self { first: Coordinate(first),last: Coordinate(last) } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Style { Paint, Erase, Outline, Widen, Down, Up, DownRight, UpRight, Unchanged }
impl From<i32> for Style {
    fn from(value: i32) -> Self {
        match value { 0 => Self::Paint, 1 => Self::Erase, 2 => Self::Outline, 3 => Self::Widen,
            4 => Self::Down, 5 => Self::Up, 6 => Self::DownRight, 7 => Self::UpRight, _ => Self::Unchanged }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Output { Bitmap, Runs }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Slant { None, Rows(Shear), Columns(Shear) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Orientation { Normal, Rotate }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Point { pub x: Coordinate, pub y: Coordinate }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bounds { pub minimum: Point, pub maximum: Point }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Config {
    pub clip: Point, pub origin: Point, pub width: Dimension, pub height: Dimension,
    pub style: Style, pub expansion: Expansion, pub slant: Slant, pub orientation: Orientation,
    pub output: Output, pub stride: Stride, pub count: RowCount, pub first_row: Row, pub run_first_row: Row,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderStatus { Complete, RunCapacity }
pub trait BitmapStorage {
    type Error;
    fn bitmap_byte(&self,offset: BitmapOffset) -> Result<u8,Self::Error>;
    fn set_bitmap_byte(&mut self,offset: BitmapOffset,value: u8) -> Result<(),Self::Error>;
    fn bounds(&self) -> Bounds;
    fn set_bounds(&mut self,bounds: Bounds);
}
pub trait Storage: BitmapStorage {
    fn input(&self,row: Row) -> Result<Span,Self::Error>;
    fn set_input(&mut self,row: Row,span: Span) -> Result<(),Self::Error>;
    fn scratch(&self,row: Row) -> Result<Span,Self::Error>;
    fn set_scratch(&mut self,row: Row,span: Span) -> Result<(),Self::Error>;
    fn merge(&mut self,row: Row,span: Span) -> Result<bool,Self::Error>;
    fn subtract(&mut self,row: Row,span: Span) -> Result<bool,Self::Error>;
    fn fill_span(&mut self,row: Row,span: Span) -> Result<(),Self::Error>;
    fn divide(&mut self,numerator: i32,denominator: i32) -> Result<i32,Self::Error>;
}
fn bounds(storage: &mut impl BitmapStorage,first_x: i32,first_y: i32,last_x: i32,last_y: i32) {
    let mut bounds=storage.bounds();
    bounds.minimum.x.0=bounds.minimum.x.0.min(first_x); bounds.maximum.x.0=bounds.maximum.x.0.max(last_x);
    bounds.minimum.y.0=bounds.minimum.y.0.min(first_y); bounds.maximum.y.0=bounds.maximum.y.0.max(last_y);
    storage.set_bounds(bounds);
}
fn apply_byte<T: BitmapStorage>(storage: &mut T,address: BitmapOffset,mask: u8,erase: bool) -> Result<(),T::Error> {
    let old=storage.bitmap_byte(address)?;
    storage.set_bitmap_byte(address,if erase { old & !mask } else { old | mask })
}
fn pixel<T: BitmapStorage>(config: Config,storage: &mut T,row: Row,column: Coordinate,erase: bool) -> Result<(),T::Error> {
    let (x,y)=match config.orientation {
        Orientation::Normal => (column.0.wrapping_add(config.origin.x.0),row.0.wrapping_add(config.origin.y.0)),
        Orientation::Rotate => (config.origin.x.0.wrapping_add(config.height.0).wrapping_sub(1).wrapping_sub(row.0),column.0.wrapping_add(config.origin.y.0)),
    };
    apply_byte(storage,BitmapOffset(config.stride.0.wrapping_mul(y).wrapping_add(x>>3)),0x80u8>>(x as u32&7),erase)?;
    if !erase { bounds(storage,x,y,x,y); }
    Ok(())
}
fn bitmap_span<T: BitmapStorage>(config: Config,storage: &mut T,row: Row,span: Span,erase: bool) -> Result<(),T::Error> {
    match config.orientation {
        Orientation::Normal => {
            let first=span.first.0.wrapping_add(config.origin.x.0);
            let last=span.last.0.wrapping_add(config.origin.x.0);
            let y=row.0.wrapping_add(config.origin.y.0);
            let address=BitmapOffset(config.stride.0.wrapping_mul(y).wrapping_add(first>>3));
            let suffix=0xffu8>>(first as u32&7);
            let prefix=0xffu8<<(7-(last as u32&7));
            let distance=(last>>3).wrapping_sub(first>>3);
            if distance==0 { apply_byte(storage,address,suffix & prefix,erase)?; }
            else {
                apply_byte(storage,address,suffix,erase)?;
                let interior=distance.wrapping_sub(1).max(0);
                let mut index=0i32;
                while index<interior {
                    storage.set_bitmap_byte(BitmapOffset(address.0.wrapping_add(1).wrapping_add(index)),if erase { 0 } else { 255 })?;
                    index=index.wrapping_add(1);
                }
                apply_byte(storage,BitmapOffset(address.0.wrapping_add(1).wrapping_add(interior)),prefix,erase)?;
            }
            if !erase { bounds(storage,first,y,last,y); }
        }
        Orientation::Rotate => {
            let x=config.origin.x.0.wrapping_add(config.height.0).wrapping_sub(1).wrapping_sub(row.0);
            let first=span.first.0.wrapping_add(config.origin.y.0);
            let last=span.last.0.wrapping_add(config.origin.y.0);
            let column=if erase { x/8 } else { x>>3 };
            let mut address=BitmapOffset(config.stride.0.wrapping_mul(first).wrapping_add(column));
            let mask=0x80u8>>(x as u32&7);
            let mut y=first;
            while y<=last {
                apply_byte(storage,address,mask,erase)?;
                y=y.wrapping_add(1); address.0=address.0.wrapping_add(config.stride.0);
            }
            if !erase { bounds(storage,x,first,x,last); }
        }
    }
    Ok(())
}
pub fn paint_span<T: BitmapStorage>(config: Config,storage: &mut T,row: Row,span: Span) -> Result<(),T::Error> {
    bitmap_span(config,storage,row,span,false)
}
fn emit<T: Storage>(config: Config,storage: &mut T,row: Row,mut span: Span,erase: bool) -> Result<bool,T::Error> {
    if let Slant::Rows(amount)=config.slant {
        let shift=storage.divide(amount.0.wrapping_mul(config.height.0.wrapping_sub(row.0)),config.height.0)?;
        span.first.0=span.first.0.wrapping_add(shift); span.last.0=span.last.0.wrapping_add(shift);
    }
    if let Slant::Columns(amount)=config.slant {
        let mut x=span.first.0;
        let mut product=amount.0.wrapping_mul(x);
        while x<=span.last.0 {
            let y=Row(row.0.wrapping_add(storage.divide(product,config.width.0)?));
            match config.output {
                Output::Bitmap => pixel(config,storage,y,Coordinate(x),erase)?,
                Output::Runs => {
                    let result=if erase { storage.subtract(y,Span::new(x,x))? } else { storage.merge(y,Span::new(x,x))? };
                    if !result { return Ok(false); }
                }
            }
            product=product.wrapping_add(amount.0); x=x.wrapping_add(1);
        }
        return Ok(true);
    }
    match config.output {
        Output::Bitmap => {
            if erase { bitmap_span(config,storage,row,span,true)?; } else { storage.fill_span(row,span)?; }
            Ok(true)
        }
        Output::Runs => if erase { storage.subtract(row,span) } else { storage.merge(row,span) },
    }
}
fn window<T: Storage>(storage: &T,first: Row,last: Row,clip: Coordinate,widen: Expansion,previous: Span) -> Result<Span,T::Error> {
    if first.0>last.0 { return Ok(previous); }
    let input=storage.input(first)?;
    let mut result=Span::new(input.first.0.wrapping_sub(clip.0),input.last.0.wrapping_add(widen.0).wrapping_sub(clip.0));
    let mut index=first.0.wrapping_add(1);
    while index<=last.0 {
        let input=storage.input(Row(index))?;
        result.first.0=result.first.0.min(input.first.0.wrapping_sub(clip.0));
        result.last.0=result.last.0.max(input.last.0.wrapping_add(widen.0).wrapping_sub(clip.0));
        index=index.wrapping_add(1);
    }
    Ok(result)
}
fn outline_emit<T: Storage>(config: Config,storage: &mut T,row: Row,source: Row,unconditional: bool) -> Result<(),T::Error> {
    let mut span=storage.scratch(source)?;
    match config.slant {
        Slant::Rows(amount) => {
            if !unconditional && span.first.0>=span.last.0 { return Ok(()); }
            let shift=storage.divide(amount.0.wrapping_mul(config.height.0.wrapping_sub(source.0)),config.height.0)?;
            span.first.0=span.first.0.wrapping_add(shift); span.last.0=span.last.0.wrapping_add(shift);
            storage.merge(row,span)?;
        }
        _ => match config.output {
            Output::Runs => { storage.merge(row,span)?; }
            Output::Bitmap => bitmap_span(config,storage,row,span,false)?,
        }
    }
    Ok(())
}
fn outline<T: Storage>(config: Config,storage: &mut T) -> Result<(),T::Error> {
    let radius=config.expansion.0;
    let clip=config.clip.x.0;
    let count=config.count.0;
    let start=config.first_row.0.wrapping_sub(config.clip.y.0);
    let lower=config.run_first_row.0;
    let prefix=if start.wrapping_sub(radius)<lower { start.wrapping_sub(lower) } else { radius };
    let mut row=start;
    let empty=Span::new(0x7fff,0);
    let mut i=0;
    while i<prefix { storage.set_scratch(Row(row),empty)?; row=row.wrapping_add(1); i=i.wrapping_add(1); }
    let first=row;
    i=0;
    while i<count {
        let span=storage.input(Row(i))?;
        let span=Span::new((span.first.0.wrapping_sub(radius).wrapping_sub(clip) as i16) as i32,(span.last.0.wrapping_add(radius).wrapping_sub(clip) as i16) as i32);
        storage.set_input(Row(i),span)?;
        storage.set_scratch(Row(row),span)?; row=row.wrapping_add(1); i=i.wrapping_add(1);
    }
    i=0;
    while i<radius { storage.set_scratch(Row(row.wrapping_add(i)),empty)?; i=i.wrapping_add(1); }
    if radius>=0 && first>=prefix.wrapping_add(lower) {
        let span=storage.input(Row(0))?;
        i=0;
        loop {
            let target=first.wrapping_sub(i);
            if target<prefix.wrapping_add(lower) { break; }
            storage.set_scratch(Row(target),span)?;
            i=i.wrapping_add(1); if i>radius { break; }
        }
    }
    i=0;
    while i<count {
        let span=storage.input(Row(i))?;
        let before=storage.input(Row(i.wrapping_sub(1)))?;
        let after=storage.input(Row(i.wrapping_add(1)))?;
        let center=first.wrapping_add(i);
        if (span.first.0<before.first.0 || span.last.0>before.last.0) && radius>0 && center>0 {
            let mut distance=1;
            loop {
                let target=Row(center.wrapping_sub(distance));
                let old=storage.scratch(target)?;
                storage.set_scratch(target,Span::new(old.first.0.min(span.first.0),old.last.0.max(span.last.0)))?;
                distance=distance.wrapping_add(1);
                if center.wrapping_sub(distance)<0 || distance>radius { break; }
            }
        }
        if (span.first.0<after.first.0 || span.last.0>after.last.0) && radius>0 {
            let mut distance=1;
            loop {
                let target=Row(center.wrapping_add(distance));
                let old=storage.scratch(target)?;
                storage.set_scratch(target,Span::new(old.first.0.min(span.first.0),old.last.0.max(span.last.0)))?;
                distance=distance.wrapping_add(1); if distance>radius { break; }
            }
        }
        i=i.wrapping_add(1);
    }
    if radius>0 {
        let last=storage.input(Row(count.wrapping_sub(1)))?;
        i=0;
        while i<radius {
            let target=Row(first.wrapping_add(count).wrapping_add(i));
            let old=storage.scratch(target)?;
            storage.set_scratch(target,Span::new(old.first.0.min(last.first.0),old.last.0.max(last.last.0)))?;
            i=i.wrapping_add(1);
        }
    }
    i=0;
    while i<prefix { outline_emit(config,storage,Row(start.wrapping_sub(prefix).wrapping_add(i)),Row(start.wrapping_add(i)),false)?; i=i.wrapping_add(1); }
    i=0;
    while i<count { outline_emit(config,storage,Row(start.wrapping_add(i)),Row(first.wrapping_add(i)),true)?; i=i.wrapping_add(1); }
    i=0;
    let tail=first.wrapping_add(count);
    while i<radius && tail.wrapping_add(i)<config.height.0.wrapping_add(radius).wrapping_add(prefix) {
        let source=Row(tail.wrapping_add(i));
        let span=storage.scratch(source)?;
        if matches!(config.slant,Slant::Rows(_)) { outline_emit(config,storage,Row(source.0.wrapping_sub(prefix)),source,false)?; }
        else if span.first.0>=span.last.0 { break; }
        else if matches!(config.output,Output::Runs) { storage.merge(Row(source.0.wrapping_sub(prefix)),Span::new(0,0))?; }
        else { outline_emit(config,storage,Row(source.0.wrapping_sub(prefix)),source,true)?; }
        i=i.wrapping_add(1);
    }
    Ok(())
}
pub fn render<T: Storage>(config: Config,storage: &mut T) -> Result<RenderStatus,T::Error> {
    let style=config.style;
    if matches!(style,Style::Unchanged) { return Ok(RenderStatus::Complete); }
    if matches!(style,Style::Outline) { outline(config,storage)?; return Ok(RenderStatus::Complete); }
    let count=config.count.0;
    let base=config.first_row.0.wrapping_sub(config.clip.y.0);
    let radius=config.expansion.0;
    let (first,last)=match style {
        Style::Down | Style::DownRight => (0,count.wrapping_sub(1).wrapping_add(radius)),
        Style::Up | Style::UpRight => (radius.wrapping_neg(),count.wrapping_sub(1)),
        _ => (0,count.wrapping_sub(1)),
    };
    let mut index=first;
    let mut previous=Span::new(0,0);
    while index<=last {
        let span=match style {
            Style::Down | Style::DownRight => window(storage,Row(index.wrapping_sub(radius).max(0)),Row(index.min(count.wrapping_sub(1))),config.clip.x,Expansion(if matches!(style,Style::DownRight) { radius } else { 0 }),previous)?,
            Style::Up | Style::UpRight => window(storage,Row(index.max(0)),Row(index.wrapping_add(radius).min(count.wrapping_sub(1))),config.clip.x,Expansion(if matches!(style,Style::UpRight) { radius } else { 0 }),previous)?,
            _ => { let span=storage.input(Row(index))?; Span::new(span.first.0.wrapping_sub(config.clip.x.0),span.last.0.wrapping_sub(config.clip.x.0).wrapping_add(if matches!(style,Style::Widen) { radius } else { 0 })) },
        };
        previous=span;
        if !emit(config,storage,Row(base.wrapping_add(index)),span,matches!(style,Style::Erase))? { return Ok(RenderStatus::RunCapacity); }
        index=index.wrapping_add(1);
    }
    Ok(RenderStatus::Complete)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Eq, PartialEq)]
    enum BufferError { OutsideBitmap }
    struct Bitmap { pixels: [u8;32], bounds: Bounds }
    impl Bitmap {
        fn new() -> Self {
            Self { pixels: [0;32],bounds: Bounds {
                minimum: Point { x: Coordinate(0x7fff),y: Coordinate(0x7fff) },
                maximum: Point { x: Coordinate(0),y: Coordinate(0) },
            } }
        }
    }
    impl BitmapStorage for Bitmap {
        type Error=BufferError;
        fn bitmap_byte(&self,offset: BitmapOffset) -> Result<u8,BufferError> {
            let index=usize::try_from(offset.0).map_err(|_| BufferError::OutsideBitmap)?;
            self.pixels.get(index).copied().ok_or(BufferError::OutsideBitmap)
        }
        fn set_bitmap_byte(&mut self,offset: BitmapOffset,value: u8) -> Result<(),BufferError> {
            let index=usize::try_from(offset.0).map_err(|_| BufferError::OutsideBitmap)?;
            *self.pixels.get_mut(index).ok_or(BufferError::OutsideBitmap)?=value;
            Ok(())
        }
        fn bounds(&self) -> Bounds { self.bounds }
        fn set_bounds(&mut self,bounds: Bounds) { self.bounds=bounds; }
    }
    fn config(orientation: Orientation) -> Config {
        Config {
            clip: Point { x: Coordinate(0),y: Coordinate(0) },
            origin: Point { x: Coordinate(0),y: Coordinate(0) },
            width: Dimension(16),height: Dimension(8),style: Style::Paint,expansion: Expansion(0),
            slant: Slant::None,orientation,output: Output::Bitmap,stride: Stride(2),
            count: RowCount(0),first_row: Row(0),run_first_row: Row(0),
        }
    }
    #[test]
    fn paints_inclusive_cross_byte_span() {
        let mut bitmap=Bitmap::new();
        assert_eq!(paint_span(config(Orientation::Normal),&mut bitmap,Row(1),Span::new(6,10)),Ok(()));
        let mut expected=[0;32];
        expected[2]=0x03; expected[3]=0xe0;
        assert_eq!(bitmap.pixels,expected);
        assert_eq!(bitmap.bounds,Bounds {
            minimum: Point { x: Coordinate(6),y: Coordinate(1) },
            maximum: Point { x: Coordinate(10),y: Coordinate(1) },
        });
    }
    #[test]
    fn rotates_span_into_strided_column() {
        let mut bitmap=Bitmap::new();
        assert_eq!(paint_span(config(Orientation::Rotate),&mut bitmap,Row(2),Span::new(6,8)),Ok(()));
        let mut expected=[0;32];
        expected[12]=0x04; expected[14]=0x04; expected[16]=0x04;
        assert_eq!(bitmap.pixels,expected);
        assert_eq!(bitmap.bounds,Bounds {
            minimum: Point { x: Coordinate(5),y: Coordinate(6) },
            maximum: Point { x: Coordinate(5),y: Coordinate(8) },
        });
    }
    #[test]
    fn propagates_checked_storage_fault() {
        let mut bitmap=Bitmap::new();
        assert_eq!(paint_span(config(Orientation::Normal),&mut bitmap,Row(16),Span::new(0,7)),Err(BufferError::OutsideBitmap));
        assert_eq!(bitmap.pixels,[0;32]);
    }
}

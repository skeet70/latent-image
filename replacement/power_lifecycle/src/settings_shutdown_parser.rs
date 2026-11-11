#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct MetadataDescriptor(pub [u8; 172]);
impl MetadataDescriptor {
    pub fn word(&self, offset: usize) -> u32 { u32::from_le_bytes(self.0[offset..offset + 4].try_into().unwrap()) }
    pub fn put(&mut self, offset: usize, value: u32) { self.0[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct CoordinatePair(pub [u32; 2]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseFailure;

pub fn decimal(bytes: &[u8]) -> Result<u32, ParseFailure> {
    let mut value = 0u32;
    for byte in bytes.iter().copied().take_while(|byte| *byte != 0) {
        let digit = byte.wrapping_sub(b'0');
        if digit > 9 { return Err(ParseFailure); }
        value = value.checked_mul(10).and_then(|value| value.checked_add(u32::from(digit))).ok_or(ParseFailure)?;
    }
    Ok(value)
}
struct Fields<'a> { source: &'a [u8], position: usize }
impl<'a> Fields<'a> {
    fn next(&mut self, delimiter: u8) -> &[u8] {
        let start = self.position;
        while self.position < self.source.len() && self.position - start < 15 {
            let byte = self.source[self.position];
            if byte == 0 || byte == b'\r' || byte == delimiter { break; }
            self.position += 1;
        }
        let end = self.position;
        if self.source.get(self.position).is_some_and(|byte| *byte != 0) { self.position += 1; }
        &self.source[start..end]
    }
    fn skip(&mut self, delimiter: u8) {
        while self.source.get(self.position).is_some_and(|byte| *byte != 0 && *byte != b'\r' && *byte != delimiter) { self.position += 1; }
        if self.source.get(self.position).is_some_and(|byte| *byte != 0) { self.position += 1; }
    }
}
fn micro_coordinate(token: &[u8], degrees: usize) -> Result<u32, ParseFailure> {
    if token.len() <= degrees + 8 { return Err(ParseFailure); }
    let dot = token.iter().position(|byte| *byte == b'.').ok_or(ParseFailure)?;
    if token.len() - dot - 1 != 6 { return Err(ParseFailure); }
    let mut packed = [0; 16];
    packed[..token.len()].copy_from_slice(token);
    packed.copy_within(dot + 1..dot + 7, dot);
    let degree = decimal(&packed[..degrees])?;
    let minute = decimal(&packed[degrees..degrees + 2])?;
    let fraction = decimal(&packed[degrees + 2..degrees + 8])?;
    let scaled_fraction = fraction.wrapping_mul(60);
    let scaled_fraction = ((u64::from(scaled_fraction) * 0xd1b71759) >> 45) as u32;
    Ok(degree.wrapping_mul(360000).wrapping_add(minute.wrapping_mul(6000)).wrapping_add(scaled_fraction))
}
pub fn coordinates(source: &[u8], output: &mut CoordinatePair) -> Result<(), ParseFailure> {
    let mut fields = Fields { source, position: 0 };
    output.0[0] = micro_coordinate(fields.next(b','), 2)?;
    if fields.next(b' ').first() == Some(&b'S') { output.0[0] = output.0[0].wrapping_neg(); }
    output.0[1] = micro_coordinate(fields.next(b','), 3)?;
    if fields.next(b' ').first() == Some(&b'W') { output.0[1] = output.0[1].wrapping_neg(); }
    Ok(())
}
fn rational(token: &[u8]) -> Option<(u32, u32)> {
    let dot = token.iter().position(|byte| *byte == b'.')?;
    let integer = decimal(&token[..dot]).ok()?;
    let fraction = decimal(&token[dot + 1..]).ok()?;
    let denominator = (0..token.len() - dot - 1).fold(1u32, |value, _| value.wrapping_mul(10));
    Some((integer.wrapping_mul(denominator).wrapping_add(fraction), denominator))
}
fn angle(token: &[u8], degrees: usize) -> Option<(u32, u32, u32)> {
    if token.iter().position(|byte| *byte == b'.')? != degrees + 2 { return None; }
    let degree = decimal(&token[..degrees]).ok()?;
    let minute = decimal(&token[degrees..degrees + 2]).ok()?;
    let fraction = decimal(&token[degrees + 3..]).ok()?.wrapping_mul(60);
    let divisor = (0..token.len() - degrees - 3).fold(1u32, |value, _| value.wrapping_mul(10));
    let seconds = if divisor > 1000 { fraction / (divisor / 1000) } else { fraction.wrapping_mul(1000 / divisor) };
    Some((degree, minute, seconds))
}

pub fn reconstruct(descriptor: &mut MetadataDescriptor, text: &[u8; 72], mode: u32, scratch: &mut CoordinatePair) {
    descriptor.0[12..16].copy_from_slice(&[2, 3, 0, 0]);
    descriptor.0[124..126].copy_from_slice(b"K\0");
    descriptor.0[148..156].fill(0);
    descriptor.0[148..154].copy_from_slice(b"WGS-84");
    descriptor.0[136..138].copy_from_slice(b"T\0");
    let _ = coordinates(text, scratch);
    descriptor.put(4, scratch.0[0]); descriptor.put(8, scratch.0[1]);
    let mut fields = Fields { source: text, position: 0 };
    if let Some((degree, minute, seconds)) = angle(fields.next(b','), 2) {
        for (offset, value) in [(20, degree), (24, 1), (28, minute), (32, 1), (36, seconds), (40, 1000)] { descriptor.put(offset, value); }
    }
    let hemisphere = fields.next(b' ');
    if hemisphere.len() == 1 { descriptor.0[16] = hemisphere[0]; descriptor.0[17] = 0; }
    if let Some((degree, minute, seconds)) = angle(fields.next(b','), 3) {
        for (offset, value) in [(48, degree), (52, 1), (56, minute), (60, 1), (64, seconds), (68, 1000)] { descriptor.put(offset, value); }
    }
    let hemisphere = fields.next(b' ');
    if hemisphere.len() == 1 { descriptor.0[44] = hemisphere[0]; descriptor.0[45] = 0; }
    let altitude = fields.next(b',');
    let mut altitude_bytes = [0; 16]; altitude_bytes[..altitude.len()].copy_from_slice(altitude);
    if altitude.first() == Some(&b'-') {
        descriptor.0[72] = 1;
        altitude_bytes.copy_within(1..altitude.len(), 0);
    } else { descriptor.0[72] = 0; }
    let altitude_end = altitude_bytes.iter().position(|byte| *byte == 0).unwrap_or(16);
    if let Some((numerator, denominator)) = rational(&altitude_bytes[..altitude_end]) { descriptor.put(76, numerator); descriptor.put(80, denominator); }
    fields.skip(b' ');
    if let Some((numerator, denominator)) = rational(fields.next(b',')) { descriptor.put(128, numerator); descriptor.put(132, denominator); }
    fields.skip(b' ');
    let date = fields.next(b' ');
    if date.len() == 10 { descriptor.0[156..166].copy_from_slice(date); }
    let time = fields.next(b' ');
    if time.iter().position(|byte| *byte == b'.') == Some(8) {
        if let (Ok(hour), Ok(minute), Ok(second)) = (decimal(&time[..2]), decimal(&time[3..5]), decimal(&time[6..8])) {
            for (offset, value) in [(84, hour), (88, 1), (92, minute), (96, 1), (100, second), (104, 1)] { descriptor.put(offset, value); }
        }
    }
    if mode == 2 { descriptor.0[12..172].fill(0); }
}

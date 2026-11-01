use flate2::{Compression, GzBuilder, read::MultiGzDecoder, write::GzEncoder};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use sha2::{Digest as _, Sha256};
use std::{fs::File, io::{self, BufRead, BufReader, Read, Write}, num::NonZeroU64, path::Path};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Digest(pub String);
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ByteLimit(NonZeroU64);
impl ByteLimit {
    pub const fn new(bytes: u64) -> Option<Self> {
        match NonZeroU64::new(bytes) { Some(bytes) => Some(Self(bytes)), None => None }
    }
    pub const fn get(self) -> u64 { self.0.get() }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Limits { compressed: ByteLimit, decoded: ByteLimit, line: ByteLimit }
impl Limits {
    pub const fn new(compressed: ByteLimit, decoded: ByteLimit, line: ByteLimit) -> Option<Self> {
        if line.get() <= decoded.get() && line.get() < usize::MAX as u64 {
            Some(Self { compressed, decoded, line })
        } else { None }
    }
    pub const fn compressed(self) -> ByteLimit { self.compressed }
    pub const fn line(self) -> ByteLimit { self.line }
}
impl<'de> Deserialize<'de> for Limits {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire { compressed: ByteLimit, decoded: ByteLimit, line: ByteLimit }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.compressed, wire.decoded, wire.line).ok_or_else(|| D::Error::custom("invalid watch byte limits"))
    }
}
pub const LIMITS: Limits = Limits {
    compressed: ByteLimit(NonZeroU64::new(256 * 1024 * 1024).unwrap()),
    decoded: ByteLimit(NonZeroU64::new(1024 * 1024 * 1024).unwrap()),
    line: ByteLimit(NonZeroU64::new(64 * 1024 * 1024).unwrap()),
};
pub const METADATA_LIMIT: ByteLimit = ByteLimit(NonZeroU64::new(256 * 1024 * 1024).unwrap());
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(transparent)]
pub struct ByteCount(u64);
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "kebab-case")]
pub enum Error {
    CompressedLimit { limit: ByteLimit },
    DecodedLimit { limit: ByteLimit },
    LineLimit { limit: ByteLimit },
    IncompleteLine,
    EmbeddedNewline,
    Io(String),
    Codec(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(formatter, "{self:?}") }
}
impl std::error::Error for Error {}
fn write_error(error: io::Error) -> Error {
    error.get_ref().and_then(|error| error.downcast_ref::<Error>()).cloned().unwrap_or_else(|| Error::Io(error.to_string()))
}
fn read_error(error: io::Error) -> Error {
    error.get_ref().and_then(|error| error.downcast_ref::<Error>()).cloned().unwrap_or_else(|| Error::Codec(error.to_string()))
}
struct BoundedWriter<W> { inner: W, used: ByteCount, limit: ByteLimit }
impl<W: Write> Write for BoundedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.used.0.checked_add(bytes.len() as u64).is_none_or(|next| next > self.limit.get()) {
            return Err(io::Error::other(Error::CompressedLimit { limit: self.limit }));
        }
        let written = self.inner.write(bytes)?;
        self.used.0 += written as u64;
        Ok(written)
    }
    fn flush(&mut self) -> io::Result<()> { self.inner.flush() }
}
struct BoundedReader<R> { inner: R, used: ByteCount, limit: ByteLimit }
impl<R: Read> Read for BoundedReader<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() { return Ok(0); }
        let remaining = self.limit.get() - self.used.0;
        if remaining == 0 {
            let mut probe = [0];
            return match self.inner.read(&mut probe)? {
                0 => Ok(0),
                _ => Err(io::Error::other(Error::CompressedLimit { limit: self.limit })),
            };
        }
        let size = bytes.len().min(usize::try_from(remaining).unwrap_or(usize::MAX));
        let read = self.inner.read(&mut bytes[..size])?;
        self.used.0 += read as u64;
        Ok(read)
    }
}
pub struct Writer<W: Write> { gzip: GzEncoder<BoundedWriter<W>>, decoded: ByteCount, limits: Limits }
impl<W: Write> Writer<W> {
    pub fn new(inner: W, limits: Limits) -> Self {
        let inner = BoundedWriter { inner, used: ByteCount(0), limit: limits.compressed };
        Self { gzip: GzBuilder::new().mtime(0).operating_system(255).write(inner, Compression::default()), decoded: ByteCount(0), limits }
    }
    pub fn append(&mut self, line: &[u8]) -> Result<(), Error> {
        if line.contains(&b'\n') { return Err(Error::EmbeddedNewline); }
        let bytes = (line.len() as u64).checked_add(1).ok_or(Error::LineLimit { limit: self.limits.line })?;
        if bytes > self.limits.line.get() { return Err(Error::LineLimit { limit: self.limits.line }); }
        let decoded = self.decoded.0.checked_add(bytes).ok_or(Error::DecodedLimit { limit: self.limits.decoded })?;
        if decoded > self.limits.decoded.get() { return Err(Error::DecodedLimit { limit: self.limits.decoded }); }
        self.gzip.write_all(line).and_then(|_| self.gzip.write_all(b"\n")).and_then(|_| self.gzip.flush()).map_err(write_error)?;
        self.decoded = ByteCount(decoded);
        Ok(())
    }
    pub fn finish(self) -> Result<W, Error> {
        let mut written = self.gzip.finish().map_err(write_error)?;
        written.flush().map_err(write_error)?;
        Ok(written.inner)
    }
}
pub struct Reader<R: Read> {
    gzip: BufReader<MultiGzDecoder<BoundedReader<R>>>,
    line: Vec<u8>,
    decoded: ByteCount,
    limits: Limits,
}
impl<R: Read> Reader<R> {
    pub fn new(inner: R, limits: Limits) -> Self {
        Self { gzip: BufReader::new(MultiGzDecoder::new(BoundedReader { inner, used: ByteCount(0), limit: limits.compressed })), line: Vec::new(), decoded: ByteCount(0), limits }
    }
    pub fn next_line(&mut self) -> Result<Option<&[u8]>, Error> {
        self.line.clear();
        let bytes = (&mut self.gzip).take(self.limits.line.get() + 1).read_until(b'\n', &mut self.line).map_err(read_error)?;
        if bytes == 0 { return Ok(None); }
        if bytes as u64 > self.limits.line.get() { return Err(Error::LineLimit { limit: self.limits.line }); }
        let decoded = self.decoded.0.checked_add(bytes as u64).ok_or(Error::DecodedLimit { limit: self.limits.decoded })?;
        if decoded > self.limits.decoded.get() { return Err(Error::DecodedLimit { limit: self.limits.decoded }); }
        if self.line.last() != Some(&b'\n') { return Err(Error::IncompleteLine); }
        self.decoded = ByteCount(decoded);
        Ok(Some(&self.line))
    }
}
pub fn digest_file(path: &Path, limit: ByteLimit) -> Result<Digest, Error> {
    let file = File::open(path).map_err(write_error)?;
    if file.metadata().map_err(write_error)?.len() > limit.get() { return Err(Error::CompressedLimit { limit }); }
    let mut reader = BoundedReader { inner: file, used: ByteCount(0), limit };
    let mut hash = Sha256::new();
    let mut buffer = [0; 16 * 1024];
    loop {
        let bytes = reader.read(&mut buffer).map_err(write_error)?;
        if bytes == 0 { break; }
        hash.update(&buffer[..bytes]);
    }
    Ok(Digest(format!("{:x}", hash.finalize())))
}

#[cfg(test)]
mod tests {
    use super::{ByteLimit, Error, Limits, Reader, Writer, LIMITS};
    use std::io::{self, Cursor, Read, Write};

    fn limits(compressed: u64, decoded: u64, line: u64) -> Limits {
        Limits::new(ByteLimit::new(compressed).unwrap(), ByteLimit::new(decoded).unwrap(), ByteLimit::new(line).unwrap()).unwrap()
    }
    fn packed(lines: &[&[u8]]) -> Vec<u8> {
        let mut writer = Writer::new(Vec::new(), LIMITS);
        for line in lines { writer.append(line).unwrap(); }
        writer.finish().unwrap()
    }
    fn collect<R: Read>(mut reader: Reader<R>) -> Result<Vec<Vec<u8>>, Error> {
        let mut lines = Vec::new();
        while let Some(line) = reader.next_line()? { lines.push(line.to_vec()); }
        Ok(lines)
    }
    #[test]
    fn gzip_lines_round_trip_without_reordering_or_omitting_bytes() {
        let data = packed(&[b"{\"a\":1}", b"{\"b\":2}"]);
        let lines = collect(Reader::new(Cursor::new(data), LIMITS)).unwrap();
        assert_eq!(lines, [b"{\"a\":1}\n".to_vec(), b"{\"b\":2}\n".to_vec()]);
    }
    #[test]
    fn truncated_gzip_trailer_refuses() {
        let mut data = packed(&[b"{\"a\":1}"]);
        data.truncate(data.len() - 8);
        assert!(collect(Reader::new(Cursor::new(data), LIMITS)).is_err());
    }
    #[test]
    fn corrupt_gzip_crc_refuses() {
        let mut data = packed(&[b"{\"a\":1}"]);
        let crc = data.len() - 8;
        data[crc] ^= 1;
        assert!(collect(Reader::new(Cursor::new(data), LIMITS)).is_err());
    }
    #[test]
    fn trailing_non_gzip_bytes_refuse() {
        let mut data = packed(&[b"{\"a\":1}"]);
        data.extend_from_slice(b"unframed");
        assert!(collect(Reader::new(Cursor::new(data), LIMITS)).is_err());
    }
    #[test]
    fn every_concatenated_member_is_consumed() {
        let mut data = packed(&[b"{\"a\":1}"]);
        data.extend_from_slice(&packed(&[b"{\"b\":2}"]));
        let lines = collect(Reader::new(Cursor::new(data), LIMITS)).unwrap();
        assert_eq!(lines.len(), 2);
    }
    #[test]
    fn cumulative_decoded_limit_refuses_small_individual_lines() {
        let data = packed(&[b"123456789", b"123456789"]);
        assert!(matches!(collect(Reader::new(Cursor::new(data), limits(256, 16, 10))), Err(Error::DecodedLimit { .. })));
    }
    #[test]
    fn decoded_line_limit_refuses_before_an_unbounded_line_accumulates() {
        let data = packed(&[b"123456789"]);
        assert!(matches!(collect(Reader::new(Cursor::new(data), limits(256, 16, 8))), Err(Error::LineLimit { .. })));
    }
    #[test]
    fn compressed_limit_refuses_before_ignored_tail_can_qualify() {
        let data = packed(&[b"{\"a\":1}"]);
        let cap = data.len() as u64 - 1;
        assert!(collect(Reader::new(Cursor::new(data), limits(cap, 64, 32))).is_err());
    }
    #[test]
    fn a_refused_append_retains_the_completed_prefix() {
        let mut writer = Writer::new(Vec::new(), limits(256, 16, 10));
        writer.append(b"123456789").unwrap();
        assert!(matches!(writer.append(b"123456789"), Err(Error::DecodedLimit { .. })));
        let data = writer.finish().unwrap();
        assert_eq!(collect(Reader::new(Cursor::new(data), LIMITS)).unwrap(), [b"123456789\n".to_vec()]);
    }
    #[derive(Clone, Copy)]
    enum FlushState { Healthy, Failed }
    struct FlushFailure(std::rc::Rc<std::cell::Cell<FlushState>>);
    impl Write for FlushFailure {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> { Ok(bytes.len()) }
        fn flush(&mut self) -> io::Result<()> {
            match self.0.get() {
                FlushState::Healthy => Ok(()),
                FlushState::Failed => Err(io::Error::other("declared flush failure")),
            }
        }
    }
    #[test]
    fn finishing_propagates_an_output_flush_failure() {
        let state = std::rc::Rc::new(std::cell::Cell::new(FlushState::Healthy));
        let mut writer = Writer::new(FlushFailure(state.clone()), LIMITS);
        writer.append(b"{\"a\":1}").unwrap();
        state.set(FlushState::Failed);
        assert!(writer.finish().is_err());
    }
    #[test]
    fn compressed_writer_budget_applies_to_the_gzip_trailer() {
        let mut writer = Writer::new(Vec::new(), limits(10, 64, 32));
        let appended = writer.append(b"{\"a\":1}");
        assert!(appended.is_err() || writer.finish().is_err());
    }
}

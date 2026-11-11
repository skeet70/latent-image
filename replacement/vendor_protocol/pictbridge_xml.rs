use core::fmt::{self, Write};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XmlError { Capacity, Malformed }

pub struct XmlBuffer { bytes: [u8; 2048], length: usize }
impl Default for XmlBuffer { fn default() -> Self { Self::new() } }
impl XmlBuffer {
    pub const fn new() -> Self { Self { bytes: [0; 2048], length: 0 } }
    pub fn clear(&mut self) { self.length = 0; }
    pub fn bytes(&self) -> &[u8] { &self.bytes[..self.length] }
    pub fn append(&mut self, text: &str) -> Result<(), XmlError> {
        let end = self.length.checked_add(text.len()).filter(|end| *end <= self.bytes.len()).ok_or(XmlError::Capacity)?;
        self.bytes[self.length..end].copy_from_slice(text.as_bytes()); self.length = end; Ok(())
    }
    pub fn word(&mut self, value: u32) -> Result<(), XmlError> { write!(self, "{value:08X}").map_err(|_| XmlError::Capacity) }
    pub fn begin(&mut self, output: bool) -> Result<(), XmlError> {
        self.clear(); self.append("<?xml version=\"1.0\"?>\n<dps xmlns=\"http://www.cipa.jp/dps/schema/\">\n")?;
        self.append(if output { "\t<output>\n" } else { "\t<input>\n" })
    }
    pub fn end(&mut self, output: bool) -> Result<(), XmlError> { self.append(if output { "\t</output>\n</dps>\n" } else { "\t</input>\n</dps>\n" }) }
    pub fn field(&mut self, tag: &str, text: &str, tabs: usize) -> Result<(), XmlError> {
        for _ in 0..tabs { self.append("\t")?; }
        self.append("<")?; self.append(tag)?; self.append(">")?; self.append(text)?;
        self.append("</")?; self.append(tag)?; self.append(">\n")
    }
    pub fn word_field(&mut self, tag: &str, value: u32, tabs: usize) -> Result<(), XmlError> {
        for _ in 0..tabs { self.append("\t")?; }
        self.append("<")?; self.append(tag)?; self.append(">")?; self.word(value)?;
        self.append("</")?; self.append(tag)?; self.append(">\n")
    }
    pub fn result(&mut self, result: u32, notification: Option<&str>) -> Result<(), XmlError> {
        self.begin(true)?; self.word_field("result", result, 2)?;
        if let Some(tag) = notification { self.append("\t\t<")?; self.append(tag)?; self.append("/>\n")?; }
        self.end(true)
    }
}
impl Write for XmlBuffer { fn write_str(&mut self, text: &str) -> fmt::Result { self.append(text).map_err(|_| fmt::Error) } }

pub fn document(packet: &[u8]) -> Result<&str, XmlError> {
    let text = core::str::from_utf8(packet).map_err(|_| XmlError::Malformed)?;
    let declaration = text.find("<?").ok_or(XmlError::Malformed)?;
    if !text[declaration + 2..].get(..3).is_some_and(|prefix| prefix.eq_ignore_ascii_case("xml")) { return Err(XmlError::Malformed); }
    element(text, "dps").ok_or(XmlError::Malformed)
}

pub fn element<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let bytes = text.as_bytes(); let mut cursor = 0;
    while cursor < bytes.len() {
        let relative = text[cursor..].find('<')?; let start = cursor + relative;
        if text[start..].starts_with("<!--") { cursor = start + text[start..].find("-->")? + 3; continue; }
        if text[start..].starts_with("<![CDATA[") { cursor = start + text[start..].find("]]>")? + 3; continue; }
        let tag = start + 1; let end = tag.checked_add(name.len())?;
        if text.get(tag..end) == Some(name) && matches!(bytes.get(end), Some(b'>') | Some(b'/') | Some(b' ') | Some(b'\t') | Some(b'\n')) {
            let content = end + text[end..].find('>')? + 1;
            if bytes.get(content.wrapping_sub(2)) == Some(&b'/') { return Some(""); }
            let mut close = content;
            loop {
                close += text[close..].find("</")?;
                if text.get(close + 2..close + 2 + name.len()) == Some(name) && bytes.get(close + 2 + name.len()) == Some(&b'>') { return text.get(content..close); }
                close += 2;
            }
        }
        cursor = start + 1;
    }
    None
}

pub fn hex(text: &str) -> Option<u32> {
    let mut value = 0u32; let mut digits = 0;
    for byte in text.bytes() {
        let nibble = match byte { b'0'..=b'9' => Some(byte - b'0'), b'A'..=b'F' => Some(byte - b'A' + 10), b'a'..=b'f' => Some(byte - b'a' + 10), _ => None };
        if let Some(nibble) = nibble { value = (value << 4) | u32::from(nibble); digits += 1; if digits == 8 { return Some(value); } }
        else { value = 0; digits = 0; }
    }
    None
}

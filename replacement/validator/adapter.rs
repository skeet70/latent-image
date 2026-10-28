use std::io::{self, BufRead, Write};

use area3_validator::{validate, Outcome, PROTECTED_BYTES, RECORD_BYTES, ROW_PARITY_MASK, ROW_PARITY_OFFSET};

fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn decode_record(text: &[u8], record: &mut [u8; RECORD_BYTES]) -> bool {
    if text.len() != RECORD_BYTES * 2 {
        return false;
    }
    let mut index = 0;
    while index < RECORD_BYTES {
        let high = match nibble(text[index * 2]) {
            Some(value) => value,
            None => return false,
        };
        let low = match nibble(text[index * 2 + 1]) {
            Some(value) => value,
            None => return false,
        };
        record[index] = (high << 4) | low;
        index += 1;
    }
    true
}

fn decode_length(text: &[u8]) -> Option<u32> {
    if text.len() != 8 {
        return None;
    }
    let mut value = 0u32;
    for byte in text {
        value = (value << 4) | (nibble(*byte)? as u32);
    }
    Some(value)
}

fn encode_record(record: &[u8; RECORD_BYTES]) -> String {
    let mut text = String::with_capacity(RECORD_BYTES * 2);
    for byte in record {
        text.push(char::from_digit((byte >> 4) as u32, 16).unwrap());
        text.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap());
    }
    text
}

fn reply(record_text: &str, length_text: &str) -> String {
    let length = match decode_length(length_text.as_bytes()) {
        Some(value) => value,
        None => return "refused bad_length_field".to_string(),
    };
    let mut record = [0u8; RECORD_BYTES];
    if !decode_record(record_text.as_bytes(), &mut record) {
        return "refused bad_record_hex".to_string();
    }
    if length != PROTECTED_BYTES as u32 {
        return "refused unsupported_length".to_string();
    }
    let stored_row = u32::from_le_bytes([
        record[ROW_PARITY_OFFSET],
        record[ROW_PARITY_OFFSET + 1],
        record[ROW_PARITY_OFFSET + 2],
        record[ROW_PARITY_OFFSET + 3],
    ]);
    if stored_row & !ROW_PARITY_MASK != 0 {
        return "refused row_parity_high_bits".to_string();
    }
    match validate(&mut record) {
        Outcome::Accepted => format!("accepted {}", encode_record(&record)),
        Outcome::Corrected(site) => format!(
            "corrected {} {} {}",
            site.row(),
            site.column(),
            encode_record(&record)
        ),
        Outcome::Rejected(syndrome) => format!(
            "rejected {:04x} {:08x} {}",
            syndrome.row.bits(),
            syndrome.column.bits(),
            encode_record(&record)
        ),
    }
}

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(text) => text,
            Err(_) => return,
        };
        let mut fields = line.split_whitespace();
        let response = match fields.next() {
            Some("validate") => {
                let record_text = fields.next().unwrap_or("");
                let length_text = fields.next().unwrap_or("");
                if fields.next().is_some() {
                    "refused bad_request".to_string()
                } else {
                    reply(record_text, length_text)
                }
            }
            _ => "refused bad_verb".to_string(),
        };
        if writeln!(output, "{}", response).is_err() {
            return;
        }
    }
    let _ = output.flush();
}

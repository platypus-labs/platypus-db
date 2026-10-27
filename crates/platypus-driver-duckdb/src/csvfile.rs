//! What a CSV or TSV file is checked for before DuckDB reads it ([`crate::files`]): its
//! delimiter, the lines that hold another number of values than the others (which DuckDB's
//! sniffer would skip silently, header included), and whether its bytes are UTF-8.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Read;

use encoding_rs::Encoding;

/// Bytes read to tell the delimiter.
pub const SAMPLE_BYTES: usize = 64 * 1024;

/// Records whose numbers of values decide how many a line has.
const SAMPLE_RECORDS: usize = 1000;

/// Lines named in the error, at most.
const NAMED_LINES: usize = 5;

/// The delimiters told apart, the usual first.
const DELIMITERS: [u8; 4] = *b",;\t|";

/// The words of the error of a file with lines that do not fit: the import dialog offers to
/// leave them out.
pub const BAD_LINES: &str = "with another number of values than";

fn reader(delimiter: u8) -> csv::ReaderBuilder {
    let mut builder = csv::ReaderBuilder::new();
    builder
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter)
        .buffer_capacity(1 << 20);
    builder
}

fn io(e: &csv::Error) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

/// The most frequent of `counts`, and how many times it occurs; on a tie, the first seen.
fn mode(counts: impl IntoIterator<Item = usize>) -> (usize, usize) {
    let mut seen: HashMap<usize, (usize, usize)> = HashMap::new();
    for (i, n) in counts.into_iter().enumerate() {
        seen.entry(n).or_insert((0, i)).0 += 1;
    }
    seen.into_iter()
        .max_by(|(_, (a, first_a)), (_, (b, first_b))| a.cmp(b).then(first_b.cmp(first_a)))
        .map_or((0, 0), |(n, (times, _))| (n, times))
}

/// The delimiter of `sample`, the start of a file (all of it when `complete`): the one that
/// splits the most of its first lines into the same number of values, more than one. A comma
/// when none does.
#[must_use]
pub fn delimiter(sample: &[u8], complete: bool) -> u8 {
    let mut best = (b',', (0, 0));
    for d in DELIMITERS {
        let mut rdr = reader(d).from_reader(sample);
        let mut record = csv::ByteRecord::new();
        let mut counts = Vec::new();
        while counts.len() < 50 && rdr.read_byte_record(&mut record).unwrap_or(false) {
            counts.push(record.len());
        }
        // The last line of a cut sample may be cut too.
        if !complete && counts.len() > 1 && counts.len() < 50 {
            counts.pop();
        }
        let (values, times) = mode(counts);
        let score = if values > 1 { (times, values) } else { (0, 0) };
        if score > best.1 {
            best = (d, score);
        }
    }
    best.0
}

/// A line that holds another number of values than the file's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadLine {
    /// The record's number, from 0, blank lines not counted.
    pub record: u64,
    /// The line it starts on, from 1: see [`Scan::count_lines`].
    pub line: u64,
    /// The byte it starts at.
    pub byte: u64,
    pub values: usize,
}

/// What [`scan`] found in a file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scan {
    /// The number of values of a line: the most frequent among the first lines.
    pub values: usize,
    pub bad: Vec<BadLine>,
    /// Whether every byte is UTF-8.
    pub utf8: bool,
}

impl Scan {
    /// Numbers the lines the error names as an editor does, from `input`, the file scanned:
    /// the reader counts no blank line.
    ///
    /// # Errors
    /// When the input cannot be read.
    pub fn count_lines(&mut self, input: impl Read) -> std::io::Result<()> {
        let mut named = self.bad.iter_mut().take(NAMED_LINES).peekable();
        let (mut at, mut line) = (0u64, 1u64);
        let mut buffer = vec![0; 1 << 16];
        let mut input = input;
        while named.peek().is_some() {
            let n = input.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            for &b in &buffer[..n] {
                // A record's position comes before the blank lines that precede it.
                if b != b'\n' && b != b'\r' {
                    while let Some(bad) = named.next_if(|bad| bad.byte <= at) {
                        bad.line = line;
                    }
                }
                at += 1;
                if b == b'\n' {
                    line += 1;
                }
            }
        }
        Ok(())
    }

    /// The error of a file with [`Self::bad`] lines.
    #[must_use]
    pub fn bad_lines_error(&self) -> String {
        let n = self.bad.len();
        let mut text = format!(
            "{n} line{} {BAD_LINES} the {} of the others:",
            if n == 1 { "" } else { "s" },
            self.values
        );
        for (i, bad) in self.bad.iter().take(NAMED_LINES).enumerate() {
            let _ = write!(
                text,
                "{} line {} has {}",
                if i == 0 { "" } else { "," },
                bad.line,
                bad.values
            );
        }
        if n > NAMED_LINES {
            let _ = write!(text, ", and {} more", n - NAMED_LINES);
        }
        text.push_str(". Leave them out to import the other lines, or correct the file.");
        text
    }
}

/// The lines of the CSV `input` split by `delimiter`: how many values a line holds, the lines
/// that hold another number, and whether the bytes are UTF-8.
///
/// # Errors
/// When the input cannot be read.
pub fn scan(input: impl Read, delimiter: u8) -> std::io::Result<Scan> {
    let mut rdr = reader(delimiter).from_reader(input);
    let mut record = csv::ByteRecord::new();
    let mut first: Vec<BadLine> = Vec::new();
    let mut scan = Scan {
        utf8: true,
        ..Scan::default()
    };
    let mut values = None;
    let mut index = 0;
    while rdr.read_byte_record(&mut record).map_err(|e| io(&e))? {
        if scan.utf8 && std::str::from_utf8(record.as_slice()).is_err() {
            scan.utf8 = false;
        }
        let line = BadLine {
            record: index,
            line: record.position().map_or(0, csv::Position::line),
            byte: record.position().map_or(0, csv::Position::byte),
            values: record.len(),
        };
        index += 1;
        match values {
            Some(n) if line.values != n => scan.bad.push(line),
            Some(_) => {}
            None => {
                first.push(line);
                if first.len() == SAMPLE_RECORDS {
                    values = Some(sampled(&mut scan, &mut first));
                }
            }
        }
    }
    if values.is_none() {
        sampled(&mut scan, &mut first);
    }
    Ok(scan)
}

/// Sets the number of values of `scan` from the `first` lines, and adds those that differ.
fn sampled(scan: &mut Scan, first: &mut Vec<BadLine>) -> usize {
    scan.values = mode(first.iter().map(|l| l.values)).0;
    scan.bad
        .extend(first.drain(..).filter(|l| l.values != scan.values));
    scan.values
}

/// The bytes of `text` without the records `skip` (record numbers, in order): the lines of
/// the others, as they are.
///
/// # Errors
/// When the text cannot be read as CSV.
pub fn without(text: &[u8], delimiter: u8, skip: &[u64]) -> std::io::Result<Vec<u8>> {
    let mut rdr = reader(delimiter).from_reader(text);
    let mut record = csv::ByteRecord::new();
    let mut out = Vec::with_capacity(text.len());
    let mut skip = skip.iter().copied().peekable();
    // The record before, by where it starts and whether it stays: it ends where this starts.
    let mut previous: Option<(usize, bool)> = None;
    let mut index = 0;
    while rdr.read_byte_record(&mut record).map_err(|e| io(&e))? {
        let start = record
            .position()
            .and_then(|p| usize::try_from(p.byte()).ok())
            .unwrap_or(0);
        if let Some((from, true)) = previous {
            out.extend_from_slice(&text[from..start]);
        }
        let keep = skip.peek() != Some(&index);
        if !keep {
            skip.next();
        }
        previous = Some((start, keep));
        index += 1;
    }
    if let Some((from, true)) = previous {
        out.extend_from_slice(&text[from..]);
    }
    Ok(out)
}

/// `bytes` decoded from `encoding` to UTF-8, without a byte order mark.
///
/// # Errors
/// When the bytes are not text of that encoding.
pub fn decode(bytes: &[u8], encoding: &'static Encoding) -> Result<Vec<u8>, String> {
    let bytes = match Encoding::for_bom(bytes) {
        Some((found, length)) if found == encoding => &bytes[length..],
        _ => bytes,
    };
    encoding
        .decode_without_bom_handling_and_without_replacement(bytes)
        .map(|text| text.into_owned().into_bytes())
        .ok_or_else(|| format!("the file is not {} text", encoding.name()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_the_delimiter() {
        assert_eq!(delimiter(b"a,b\n1,2\n", true), b',');
        assert_eq!(delimiter(b"a;b;c\n1,5;2;3\n", true), b';');
        assert_eq!(delimiter(b"a\tb\n\"x,y\"\t2\n", true), b'\t');
        assert_eq!(delimiter(b"single\nvalue\n", true), b',');
        // Irregular lines still have a usual number of values.
        assert_eq!(delimiter(b"a,b\n1,2\n3\n4,5,6\n", true), b',');
    }

    #[test]
    fn finds_lines_with_another_number_of_values() {
        let text = b"a,b\n1,2\n3\n\n4,5,6\n\"x\ny\",7\n9\n";
        let mut scan = scan(&text[..], b',').unwrap();
        assert_eq!(scan.values, 2);
        assert!(scan.utf8);
        scan.count_lines(&text[..]).unwrap();
        let found: Vec<_> = scan
            .bad
            .iter()
            .map(|b| (b.record, b.line, b.values))
            .collect();
        // The blank line counts, and so does the line inside quotes.
        assert_eq!(found, [(2, 3, 1), (3, 5, 3), (5, 8, 1)]);
        scan.bad.pop();
        assert_eq!(
            scan.bad_lines_error(),
            "2 lines with another number of values than the 2 of the others: line 3 has 1, \
             line 5 has 3. Leave them out to import the other lines, or correct the file."
        );
        // A title line before the header is the odd one.
        let scan = scan_of("Report\nid,name\n1,a\n2,b\n");
        assert_eq!((scan.values, scan.bad.len()), (2, 1));
        assert!(scan_of("id,city\n1,citt\u{e0}\n").utf8);
        assert!(
            !self::scan(&b"id,city\n1,citt\xe0\n"[..], b',')
                .unwrap()
                .utf8
        );
    }

    fn scan_of(text: &str) -> Scan {
        scan(text.as_bytes(), b',').unwrap()
    }

    #[test]
    fn leaves_records_out_as_they_are() {
        let text = b"a,b\n1,2\n3\n\"x\ny\",\"\"\n4,5,6";
        assert_eq!(
            without(text, b',', &[2, 4]).unwrap(),
            b"a,b\n1,2\n\"x\ny\",\"\"\n"
        );
        assert_eq!(without(text, b',', &[]).unwrap(), text);
    }

    #[test]
    fn decodes_other_encodings() {
        let latin = b"id,city\n1,citt\xe0 \x80\n";
        assert_eq!(
            decode(latin, encoding_rs::WINDOWS_1252).unwrap(),
            "id,city\n1,citt\u{e0} \u{20ac}\n".as_bytes()
        );
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("a,\u{e8}\n".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(
            decode(&utf16, encoding_rs::UTF_16LE).unwrap(),
            "a,\u{e8}\n".as_bytes()
        );
        assert!(decode(latin, encoding_rs::UTF_8).is_err());
    }
}

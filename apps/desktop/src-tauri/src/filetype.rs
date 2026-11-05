//! Tells a file from the bytes of a cell, by magic numbers, and finds the file a text cell
//! holds as a `data:` URL, base64 or hex. A value is never called a file from its column's type:
//! only from what its bytes say. The frontend (`src/grid/fileType.ts`) only draws images.

use serde::Serialize;

/// A kind of file: how the UI names it, the extension saving gives it and its MIME type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FileType {
    pub name: &'static str,
    pub ext: &'static str,
    pub mime: &'static str,
}

const fn kind(name: &'static str, ext: &'static str, mime: &'static str) -> FileType {
    FileType { name, ext, mime }
}

const ZIP: FileType = kind("ZIP", "zip", "application/zip");
const DOCX: FileType = kind(
    "DOCX",
    "docx",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
);
const XLSX: FileType = kind(
    "XLSX",
    "xlsx",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
);
const PPTX: FileType = kind(
    "PPTX",
    "pptx",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
);
const ODT: FileType = kind("ODT", "odt", "application/vnd.oasis.opendocument.text");
const ODS: FileType = kind(
    "ODS",
    "ods",
    "application/vnd.oasis.opendocument.spreadsheet",
);

/// Bytes read to tell a file; a ZIP's first entries are looked at within this much.
pub const PREFIX: usize = 64 * 1024;
/// A file coded as text is at least this many bytes: a short hex or base64 string is a word, a
/// hash or an identifier, not a file.
const MIN_CODED: usize = 16;

fn at(bytes: &[u8], offset: usize, magic: &[u8]) -> bool {
    bytes.get(offset..offset + magic.len()) == Some(magic)
}

fn le16(bytes: &[u8], at: usize) -> Option<usize> {
    Some(usize::from(u16::from_le_bytes([
        *bytes.get(at)?,
        *bytes.get(at + 1)?,
    ])))
}

fn le32(bytes: &[u8], at: usize) -> Option<usize> {
    let b = bytes.get(at..at + 4)?;
    usize::try_from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok()
}

/// What a ZIP is, from the names of its entries (Office) or its `mimetype` entry (`OpenDocument`).
fn zip_kind(bytes: &[u8]) -> FileType {
    let mut pos = 0;
    while at(bytes, pos, b"PK\x03\x04") {
        let (Some(flags), Some(size), Some(name_len), Some(extra_len)) = (
            le16(bytes, pos + 6),
            le32(bytes, pos + 18),
            le16(bytes, pos + 26),
            le16(bytes, pos + 28),
        ) else {
            break;
        };
        let start = pos + 30;
        let Some(name) = bytes.get(start..start + name_len) else {
            break;
        };
        let data = start + name_len + extra_len;
        if name == b"mimetype" {
            let text = bytes.get(data..data + size.min(80)).unwrap_or_default();
            if text.starts_with(b"application/vnd.oasis.opendocument.text") {
                return ODT;
            }
            if text.starts_with(b"application/vnd.oasis.opendocument.spreadsheet") {
                return ODS;
            }
        }
        if name.starts_with(b"word/") {
            return DOCX;
        }
        if name.starts_with(b"xl/") {
            return XLSX;
        }
        if name.starts_with(b"ppt/") {
            return PPTX;
        }
        // The next entry follows the data; with a data descriptor the size is not known here,
        // so the next signature is searched for.
        pos = if size > 0 && flags & 8 == 0 {
            data + size
        } else {
            match bytes
                .get(data..)
                .and_then(|rest| rest.windows(4).position(|w| w == b"PK\x03\x04"))
            {
                Some(next) => data + next,
                None => break,
            }
        };
    }
    ZIP
}

/// An SVG image: XML text with an `<svg` element near its start.
fn is_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(2048)];
    let Ok(text) =
        std::str::from_utf8(head).or_else(|e| std::str::from_utf8(&head[..e.valid_up_to()]))
    else {
        return false;
    };
    let text = text.trim_start_matches('\u{feff}').trim_start();
    (text.starts_with("<svg")
        || text.starts_with("<?xml")
        || text.starts_with("<!--")
        || text.starts_with("<!DOCTYPE svg"))
        && text.contains("<svg")
}

/// The kind of file `bytes` start like (`PREFIX` bytes are enough), if any.
#[must_use]
pub fn detect(bytes: &[u8]) -> Option<FileType> {
    let b = bytes;
    Some(if at(b, 0, b"%PDF-") {
        kind("PDF", "pdf", "application/pdf")
    } else if at(b, 0, b"\x89PNG\r\n\x1a\n") {
        kind("PNG", "png", "image/png")
    } else if at(b, 0, b"\xff\xd8\xff") {
        kind("JPEG", "jpg", "image/jpeg")
    } else if at(b, 0, b"GIF87a") || at(b, 0, b"GIF89a") {
        kind("GIF", "gif", "image/gif")
    } else if at(b, 0, b"RIFF") && at(b, 8, b"WEBP") {
        kind("WebP", "webp", "image/webp")
    } else if at(b, 0, b"RIFF") && at(b, 8, b"WAVE") {
        kind("WAV", "wav", "audio/wav")
    } else if at(b, 0, b"BM")
        && b.len() >= 18
        && at(b, 6, &[0, 0, 0, 0])
        && matches!(le32(b, 14), Some(12 | 40 | 52 | 56 | 64 | 108 | 124))
    {
        kind("BMP", "bmp", "image/bmp")
    } else if at(b, 0, b"II*\0") || at(b, 0, b"MM\0*") {
        kind("TIFF", "tif", "image/tiff")
    } else if at(b, 0, b"PK\x03\x04") {
        zip_kind(b)
    } else if at(b, 0, b"\x1f\x8b\x08") {
        kind("GZIP", "gz", "application/gzip")
    } else if at(b, 0, b"7z\xbc\xaf\x27\x1c") {
        kind("7Z", "7z", "application/x-7z-compressed")
    } else if at(b, 0, b"Rar!\x1a\x07") {
        kind("RAR", "rar", "application/vnd.rar")
    } else if at(b, 257, b"ustar") {
        kind("TAR", "tar", "application/x-tar")
    } else if at(b, 0, b"ID3") && b.get(3).is_some_and(|&v| v < 5) && b.len() >= 10 {
        kind("MP3", "mp3", "audio/mpeg")
    } else if at(b, 0, b"OggS") {
        kind("OGG", "ogg", "audio/ogg")
    } else if at(b, 4, b"ftyp") {
        if at(b, 8, b"qt  ") {
            kind("MOV", "mov", "video/quicktime")
        } else {
            kind("MP4", "mp4", "video/mp4")
        }
    } else if at(b, 0, b"\x1a\x45\xdf\xa3") {
        let head = &b[..b.len().min(64)];
        if head.windows(4).any(|w| w == b"webm") {
            kind("WebM", "webm", "video/webm")
        } else {
            kind("MKV", "mkv", "video/x-matroska")
        }
    } else if at(b, 0, b"SQLite format 3\0") {
        kind("SQLite", "sqlite", "application/vnd.sqlite3")
    } else if is_svg(b) {
        kind("SVG", "svg", "image/svg+xml")
    } else {
        return None;
    })
}

/// How a text cell codes its file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Coding {
    /// `data:<mime>;base64,...`, the mime type declared (empty when not).
    DataBase64,
    /// `data:<mime>,...` with percent escapes.
    DataText,
    /// Hex digits after `skip` bytes of `\x` or `0x`.
    Hex {
        skip: usize,
    },
    Base64,
}

/// A file a text cell holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TextFile {
    #[serde(flatten)]
    pub kind: FileType,
    /// The bytes of the decoded file.
    pub size: u64,
    #[serde(skip)]
    coding: Coding,
    #[serde(skip)]
    body: usize,
}

fn hex_value(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn base64_value(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    }
}

/// Hex digits of `text` (whitespace ignored) as bytes; `None` for any other character or an
/// odd count. At most `limit` bytes are decoded.
fn decode_hex(text: &[u8], limit: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 2);
    let mut high = None;
    for &c in text {
        if c.is_ascii_whitespace() {
            continue;
        }
        let v = hex_value(c)?;
        match high.take() {
            None => high = Some(v),
            Some(h) => {
                out.push((h << 4) | v);
                if out.len() >= limit {
                    return Some(out);
                }
            }
        }
    }
    high.is_none().then_some(out)
}

/// Base64 of `text`, standard or URL-safe, whitespace ignored, padding optional; `None` for
/// any other character or an impossible length. Decodes at most `limit` bytes when `whole` is
/// false (the text then may be a cut of a longer one).
fn decode_base64(text: &[u8], limit: usize, whole: bool) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits, mut digits, mut pad) = (0u32, 0, 0usize, 0usize);
    for &c in text {
        if c.is_ascii_whitespace() {
            continue;
        }
        if c == b'=' {
            pad += 1;
            continue;
        }
        if pad > 0 {
            return None;
        }
        acc = (acc << 6) | u32::from(base64_value(c)?);
        bits += 6;
        digits += 1;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xff).ok()?);
            if !whole && out.len() >= limit {
                return Some(out);
            }
        }
    }
    let valid = match (digits + pad) % 4 {
        0 => pad <= 2,
        // Unpadded: two or three digits are left over.
        _ => pad == 0 && digits % 4 != 1,
    };
    valid.then_some(out)
}

fn percent_decode(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text[i] == b'%'
            && let (Some(&h), Some(&l)) = (text.get(i + 1), text.get(i + 2))
            && let (Some(h), Some(l)) = (hex_value(h), hex_value(l))
        {
            out.push((h << 4) | l);
            i += 3;
        } else {
            out.push(text[i]);
            i += 1;
        }
    }
    out
}

/// The type a `data:` URL declares, for content the magic numbers do not know.
fn declared_type(mime: &str) -> FileType {
    let (name, ext, mime): (&'static str, &'static str, &'static str) = match mime {
        "text/plain" => ("TXT", "txt", "text/plain"),
        "text/csv" => ("CSV", "csv", "text/csv"),
        "text/html" => ("HTML", "html", "text/html"),
        "application/json" => ("JSON", "json", "application/json"),
        "application/xml" | "text/xml" => ("XML", "xml", "application/xml"),
        _ => ("File", "bin", "application/octet-stream"),
    };
    kind(name, ext, mime)
}

/// The split of a `data:` URL: its media type, whether it is base64 and where the data starts.
fn data_url(text: &str) -> Option<(&str, bool, usize)> {
    let head = text.get(..5)?;
    if !head.eq_ignore_ascii_case("data:") {
        return None;
    }
    let comma = text.as_bytes()[..text.len().min(300)]
        .iter()
        .position(|&c| c == b',')?;
    let meta = &text[5..comma];
    let base64 = meta
        .rsplit(';')
        .next()
        .is_some_and(|p| p.eq_ignore_ascii_case("base64"));
    let mime = meta.split(';').next().unwrap_or_default();
    Some((mime, base64, comma + 1))
}

/// The decoded size of `len` base64 digits (padding and whitespace counted in `len` too late
/// to matter: the caller passes the digits only).
fn base64_size(digits: usize) -> u64 {
    (digits / 4 * 3 + [0, 0, 1, 2][digits % 4]) as u64
}

/// The text's candidates for coding, in the order they are tried.
fn codings(text: &str) -> Vec<(Coding, usize)> {
    let t = text.as_bytes();
    if data_url(text).is_some() {
        return Vec::new();
    }
    let mut found = Vec::new();
    if t.starts_with(b"\\x") || t.starts_with(b"0x") || t.starts_with(b"0X") {
        found.push((Coding::Hex { skip: 2 }, 2));
    } else {
        found.push((Coding::Hex { skip: 0 }, 0));
        found.push((Coding::Base64, 0));
    }
    found
}

fn hex_digits(t: &[u8]) -> usize {
    t.iter().filter(|c| !c.is_ascii_whitespace()).count()
}

/// The file `text` holds: a `data:` URL, hex (as PostgreSQL prints bytea, `0x...` or plain) or
/// base64 (standard or URL-safe, with line breaks) whose bytes are a known file. Only the
/// start is decoded to tell the type; ordinary text is not taken for coded bytes.
#[must_use]
pub fn text_file(text: &str) -> Option<TextFile> {
    let trimmed = text.trim();
    if let Some((mime, base64, start)) = data_url(trimmed) {
        let body = &trimmed.as_bytes()[start..];
        let (bytes, size) = if base64 {
            let head = decode_base64(&body[..body.len().min(8192)], 4096, false)?;
            let digits = body.iter().filter(|&&c| base64_value(c).is_some()).count();
            (head, base64_size(digits))
        } else {
            let head = percent_decode(&body[..body.len().min(4096)]);
            (head, percent_decode(body).len() as u64)
        };
        let kind = detect(&bytes).unwrap_or_else(|| declared_type(mime));
        return Some(TextFile {
            kind,
            size,
            coding: if base64 {
                Coding::DataBase64
            } else {
                Coding::DataText
            },
            body: start,
        });
    }
    for (coding, skip) in codings(trimmed) {
        let body = &trimmed.as_bytes()[skip..];
        let cut = &body[..body.len().min(8192)];
        // The start tells the type; only then is the rest looked at.
        let head = match coding {
            Coding::Hex { .. } => decode_hex(cut, 4096),
            _ => decode_base64(cut, 4096, false),
        };
        let Some(kind) = head.as_deref().and_then(detect) else {
            continue;
        };
        let size = match coding {
            Coding::Hex { .. } => (hex_digits(body) / 2) as u64,
            _ => base64_size(
                body.iter()
                    .filter(|c| !c.is_ascii_whitespace() && **c != b'=')
                    .count(),
            ),
        };
        // The whole text must be the coding, not just its start.
        let sound = match coding {
            Coding::Hex { .. } => decode_hex(body, usize::MAX).is_some(),
            _ => decode_base64(body, usize::MAX, true).is_some(),
        };
        if sound && size >= MIN_CODED as u64 {
            return Some(TextFile {
                kind,
                size,
                coding,
                body: skip,
            });
        }
    }
    None
}

/// The bytes of the file `text` holds, `None` when it holds none (see [`text_file`]).
#[must_use]
pub fn decode_text(text: &str) -> Option<Vec<u8>> {
    let trimmed = text.trim();
    let found = text_file(trimmed)?;
    let body = &trimmed.as_bytes()[found.body..];
    match found.coding {
        Coding::DataBase64 | Coding::Base64 => decode_base64(body, usize::MAX, true),
        Coding::Hex { .. } => decode_hex(body, usize::MAX),
        Coding::DataText => Some(percent_decode(body)),
    }
}

/// How a value is shown in the grid: `PDF · 2.3 MB`.
#[must_use]
pub fn label(kind: FileType, size: u64) -> String {
    format!("{} · {}", kind.name, human_size(size))
}

#[allow(clippy::cast_precision_loss)]
fn human_size(size: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    if size < 1024 {
        return format!("{size} B");
    }
    let mut value = size as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::{decode_text, detect, label, text_file};

    fn name(bytes: &[u8]) -> Option<&'static str> {
        detect(bytes).map(|k| k.name)
    }

    fn zip_entry(name: &str, data: &[u8]) -> Vec<u8> {
        let mut e = b"PK\x03\x04\x14\0\0\0\0\0\0\0\0\0\0\0\0\0".to_vec();
        e.extend(u32::try_from(data.len()).expect("len").to_le_bytes());
        e.extend(u32::try_from(data.len()).expect("len").to_le_bytes());
        e.extend(u16::try_from(name.len()).expect("len").to_le_bytes());
        e.extend([0, 0]);
        e.extend(name.as_bytes());
        e.extend(data);
        e
    }

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        entries.iter().flat_map(|(n, d)| zip_entry(n, d)).collect()
    }

    fn pdf() -> Vec<u8> {
        let mut b = b"%PDF-1.7\n".to_vec();
        b.extend((0..200u8).map(|i| i.wrapping_mul(7)));
        b
    }

    fn b64(bytes: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for c in bytes.chunks(3) {
            let n = c
                .iter()
                .enumerate()
                .fold(0u32, |a, (i, &b)| a | (u32::from(b) << (16 - 8 * i)));
            for i in 0..=c.len() {
                s.push(char::from(T[(n >> (18 - 6 * i)) as usize & 63]));
            }
            for _ in c.len()..3 {
                s.push('=');
            }
        }
        s
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().fold(String::new(), |mut s, b| {
            let _ = std::fmt::Write::write_fmt(&mut s, format_args!("{b:02x}"));
            s
        })
    }

    #[test]
    fn tells_the_known_types() {
        assert_eq!(name(&pdf()), Some("PDF"));
        assert_eq!(name(b"\x89PNG\r\n\x1a\n\0\0"), Some("PNG"));
        assert_eq!(name(b"\xff\xd8\xff\xe0\0\x10JFIF"), Some("JPEG"));
        assert_eq!(name(b"GIF89a\x01\0"), Some("GIF"));
        assert_eq!(name(b"RIFF\0\0\0\0WEBPVP8 "), Some("WebP"));
        assert_eq!(name(b"RIFF\0\0\0\0WAVEfmt "), Some("WAV"));
        assert_eq!(name(b"II*\0\x08\0\0\0"), Some("TIFF"));
        assert_eq!(name(b"\x1f\x8b\x08\0\0\0\0\0"), Some("GZIP"));
        assert_eq!(name(b"7z\xbc\xaf\x27\x1c\0\x04"), Some("7Z"));
        assert_eq!(name(b"Rar!\x1a\x07\x01\0"), Some("RAR"));
        assert_eq!(name(b"ID3\x03\0\0\0\0\0\x21"), Some("MP3"));
        assert_eq!(name(b"OggS\0\x02\0\0"), Some("OGG"));
        assert_eq!(name(b"\0\0\0\x18ftypisom\0\0\x02\0"), Some("MP4"));
        assert_eq!(name(b"\0\0\0\x14ftypqt  \0\0\0\0"), Some("MOV"));
        assert_eq!(name(b"\x1a\x45\xdf\xa3\x93B\x82\x88matroska"), Some("MKV"));
        assert_eq!(name(b"\x1a\x45\xdf\xa3\x93B\x82\x84webm"), Some("WebM"));
        assert_eq!(name(b"SQLite format 3\0\x10\0"), Some("SQLite"));
        assert_eq!(
            name(b"<?xml version=\"1.0\"?><svg xmlns=\"x\"/>"),
            Some("SVG")
        );
        assert_eq!(name(b"  <svg width=\"1\"></svg>"), Some("SVG"));
        let mut bmp = b"BM\x36\0\0\0\0\0\0\0\x36\0\0\0\x28\0\0\0".to_vec();
        bmp.extend([0; 8]);
        assert_eq!(name(&bmp), Some("BMP"));
        let mut tar = vec![0u8; 257];
        tar.extend(b"ustar\0");
        assert_eq!(name(&tar), Some("TAR"));
    }

    #[test]
    fn tells_office_and_open_document_zips() {
        let docx = zip_of(&[
            ("[Content_Types].xml", b"<x/>"),
            ("word/document.xml", b"<d/>"),
        ]);
        assert_eq!(name(&docx), Some("DOCX"));
        let xlsx = zip_of(&[
            ("[Content_Types].xml", b"<x/>"),
            ("xl/workbook.xml", b"<d/>"),
        ]);
        assert_eq!(name(&xlsx), Some("XLSX"));
        let pptx = zip_of(&[("ppt/presentation.xml", b"<d/>")]);
        assert_eq!(name(&pptx), Some("PPTX"));
        let odt = zip_of(&[
            ("mimetype", b"application/vnd.oasis.opendocument.text"),
            ("content.xml", b"<d/>"),
        ]);
        assert_eq!(name(&odt), Some("ODT"));
        let ods = zip_of(&[(
            "mimetype",
            b"application/vnd.oasis.opendocument.spreadsheet",
        )]);
        assert_eq!(name(&ods), Some("ODS"));
        assert_eq!(name(&zip_of(&[("a.txt", b"hello")])), Some("ZIP"));
    }

    #[test]
    fn plain_bytes_are_no_file() {
        assert_eq!(detect(b""), None);
        assert_eq!(detect(b"%PD"), None);
        assert_eq!(detect(b"BM"), None);
        assert_eq!(detect(b"hello world, this is text"), None);
        assert_eq!(detect(b"<html><body>no image</body></html>"), None);
        assert_eq!(
            detect(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]),
            None
        );
        // Pseudo-random bytes of many lengths.
        let mut x = 0x1234_5678_u32;
        for len in 1..200 {
            let bytes: Vec<u8> = (0..len)
                .map(|_| {
                    x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (x >> 24) as u8
                })
                .collect();
            assert_eq!(detect(&bytes), None, "{bytes:?}");
        }
    }

    #[test]
    fn finds_files_in_text() {
        let file = pdf();
        let url = format!("data:application/pdf;base64,{}", b64(&file));
        let found = text_file(&url).expect("data URL");
        assert_eq!((found.kind.name, found.size), ("PDF", file.len() as u64));
        assert_eq!(decode_text(&url), Some(file.clone()));
        // Base64 alone, with line breaks, URL-safe and unpadded.
        let coded = b64(&file);
        assert_eq!(decode_text(&coded), Some(file.clone()));
        let mut wrapped = String::new();
        for line in coded.as_bytes().chunks(76) {
            wrapped.push_str(&String::from_utf8_lossy(line));
            wrapped.push_str("\r\n");
        }
        assert_eq!(decode_text(&wrapped), Some(file.clone()));
        let safe = coded
            .replace('+', "-")
            .replace('/', "_")
            .trim_end_matches('=')
            .to_owned();
        assert_eq!(decode_text(&safe), Some(file.clone()));
        // Hex: plain, `0x`, and PostgreSQL's `\x` text form of a bytea.
        for text in [
            hex(&file),
            format!("0x{}", hex(&file)),
            format!("\\x{}", hex(&file)),
            hex(&file).to_uppercase(),
        ] {
            assert_eq!(decode_text(&text), Some(file.clone()), "{}", &text[..12]);
        }
        // A data URL of an unknown type is still a file, typed by its declaration.
        let text = text_file("data:text/plain;base64,aGVsbG8gd29ybGQ=").expect("text URL");
        assert_eq!((text.kind.ext, text.size), ("txt", 11));
        assert_eq!(
            decode_text("data:text/plain,hello%20world"),
            Some(b"hello world".to_vec())
        );
    }

    #[test]
    fn ordinary_text_is_not_a_file() {
        let words = "The quick brown fox jumps over the lazy dog".repeat(5);
        for text in [
            "",
            "hello",
            "Hello World",
            "HelloWorldThisIsPlainTextOnly",
            "d41d8cd98f00b204e9800998ecf8427e",
            "da39a3ee5e6b4b0d3255bfef95601890afd80709",
            "550e8400-e29b-41d4-a716-446655440000",
            "550e8400e29b41d4a716446655440000",
            "255044462d",
            "0xdeadbeef",
            "\\xdeadbeef",
            "12345678901234567890123456789012",
            "aGVsbG8gd29ybGQ=",
            "data",
            "data:",
            &words,
        ] {
            assert_eq!(text_file(text), None, "{text}");
            assert_eq!(decode_text(text), None, "{text}");
        }
        // A PDF's coding with a stray character is no file either.
        let bad = format!("{}!", b64(&pdf()));
        assert_eq!(decode_text(&bad), None);
    }

    #[test]
    fn labels_a_file_with_its_size() {
        let kind = detect(&pdf()).expect("pdf");
        assert_eq!(label(kind, 100), "PDF · 100 B");
        assert_eq!(label(kind, 2_411_724), "PDF · 2.3 MB");
    }
}

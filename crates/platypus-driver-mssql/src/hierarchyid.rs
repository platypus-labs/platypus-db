//! `hierarchyid` values read from their serialized bytes into the path `ToString()` writes
//! (`/1/2.3/`), which SQL Server also reads back.
//!
//! The bytes are a string of bits: each label of a level is a prefix that picks its range,
//! the bits of its offset in the range mixed with fixed bits, and a last bit that ends the
//! level (`1`, a `/`) or goes on with another label (`0`, a `.`; such labels are stored one
//! above their value). Zero bits pad the last byte.

/// The ranges of labels: the smallest label and its bits, `x` for the offset, `0` and `1`
/// fixed, `T` the end of the label. The prefixes before the first `x` tell them apart.
const PATTERNS: [(i64, &str); 13] = [
    (0, "01xxT"),
    (4, "100xxT"),
    (8, "101xxxT"),
    (16, "110xx0x1xxxT"),
    (80, "1110xxx0xxx0x1xxxT"),
    (1_104, "11110xxxxx0xxx0x1xxxT"),
    (5_200, "111110xxxxxxxxxxxxxxxxxxx0xxxxxx0xxx0x1xxxT"),
    (
        4_294_972_496,
        "111111xxxxxxxxxxxxxx0xxxxxxxxxxxxxxxxxxxxx0xxxxxx0xxx0x1xxxT",
    ),
    (-8, "00111xxxT"),
    (-72, "0010xx0x1xxxT"),
    (-4_168, "000110xxxxx0xxx0x1xxxT"),
    (
        -4_294_971_464,
        "000101xxxxxxxxxxxxxxxxxxx0xxxxxx0xxx0x1xxxT",
    ),
    (
        -281_479_271_682_120,
        "000100xxxxxxxxxxxxxx0xxxxxxxxxxxxxxxxxxxxx0xxxxxx0xxx0x1xxxT",
    ),
];

/// Bit `i` of `bytes`, the first bit the highest of the first byte.
fn bit(bytes: &[u8], i: usize) -> Option<bool> {
    bytes.get(i / 8).map(|b| b & (0x80 >> (i % 8)) != 0)
}

/// One label at bit `pos`: its value, whether it ends its level, and the bit after it.
fn label(bytes: &[u8], pos: usize) -> Option<(i64, bool, usize)> {
    let (min, pattern) = PATTERNS.iter().find(|(_, p)| {
        let prefix = &p[..p.find('x').unwrap_or(p.len())];
        prefix
            .bytes()
            .enumerate()
            .all(|(i, c)| bit(bytes, pos + i) == Some(c == b'1'))
    })?;
    let mut offset: i64 = 0;
    let mut last = false;
    let prefix = pattern.find('x').unwrap_or(pattern.len());
    for (i, c) in pattern.bytes().enumerate().skip(prefix) {
        let b = bit(bytes, pos + i)?;
        match c {
            b'x' => offset = offset * 2 + i64::from(b),
            b'T' => last = b,
            fixed => {
                if b != (fixed == b'1') {
                    return None;
                }
            }
        }
    }
    let value = min + offset - i64::from(!last);
    Some((value, last, pos + pattern.len()))
}

/// The path of a `hierarchyid` from its bytes; `None` when they are not one.
pub(crate) fn path(bytes: &[u8]) -> Option<String> {
    let bits = bytes.len() * 8;
    let mut out = String::from("/");
    let mut pos = 0;
    let mut open = false;
    // What is left after the last label is the zero padding of its byte.
    while (pos..bits).any(|i| bit(bytes, i) == Some(true)) {
        let (value, last, next) = label(bytes, pos)?;
        if open {
            out.push('.');
        }
        out.push_str(&value.to_string());
        if last {
            out.push('/');
        }
        open = !last;
        pos = next;
    }
    (!open && bits - pos < 8).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::path;

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn reads_paths_as_sql_server_writes_them() {
        // `CAST(CAST(p AS hierarchyid) AS varbinary(892))` on SQL Server 2022.
        for (bytes, shown) in [
            ("", "/"),
            ("48", "/0/"),
            ("58", "/1/"),
            ("78", "/3/"),
            ("84", "/4/"),
            ("9C", "/7/"),
            ("A2", "/8/"),
            ("BE", "/15/"),
            ("C110", "/16/"),
            ("DBF0", "/79/"),
            ("E00440", "/80/"),
            ("EEEFC0", "/1103/"),
            ("F00088", "/1104/"),
            ("F7DDF8", "/5199/"),
            ("F80000000220", "/5200/"),
            ("3F80", "/-1/"),
            ("3880", "/-8/"),
            ("2DF8", "/-9/"),
            ("2088", "/-72/"),
            ("1BEEFC", "/-73/"),
            ("180044", "/-4168/"),
            ("17FFFFBF77E0", "/-4169/"),
            ("6240", "/1.0/"),
            ("62C0", "/1.1/"),
            ("52C0", "/0.1/"),
            ("73C0", "/2.3/"),
            ("5B9E", "/1/2.3/"),
            ("39FC00001BB34553", "/-7/12345.0.9/"),
            ("61FC", "/1.-1/"),
            ("7AC0", "/3/1/"),
        ] {
            assert_eq!(path(&hex(bytes)).as_deref(), Some(shown), "{bytes}");
        }
    }

    #[test]
    fn refuses_bytes_that_are_no_path() {
        // A label cut short, a level left open, a fixed bit that is wrong.
        assert_eq!(path(&hex("C1")), None);
        assert_eq!(path(&hex("62")), None);
        assert_eq!(path(&hex("C010")), None);
        assert_eq!(path(&hex("FFFFFFFF")), None);
    }
}

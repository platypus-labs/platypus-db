//! Geometries read from the values the drivers return, for drawing them: well-known binary
//! (WKB, `PostGIS` EWKB as `\x…` hex text, MySQL's SRID-prefixed WKB), well-known text (WKT,
//! with an optional `SRID=…;`) and PostgreSQL's own geometric types as text. Only `x` and
//! `y` are kept; `z` and `m` are skipped.

use serde::Serialize;

/// A shape in plane coordinates.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Shape {
    Point {
        x: f64,
        y: f64,
    },
    /// A line string or an open path. PostgreSQL prints a closed path as a polygon: it is
    /// read as one.
    Line {
        points: Vec<[f64; 2]>,
    },
    /// The outer ring first, then the holes.
    Polygon {
        rings: Vec<Vec<[f64; 2]>>,
    },
    Circle {
        x: f64,
        y: f64,
        r: f64,
    },
    /// Multi-points, -lines and -polygons, and geometry collections.
    Collection {
        items: Vec<Shape>,
    },
}

/// The shape in a binary value: WKB, EWKB, or MySQL's 4-byte SRID followed by WKB.
#[must_use]
pub fn from_bytes(bytes: &[u8]) -> Option<Shape> {
    Wkb::new(bytes)
        .read_all()
        .or_else(|| Wkb::new(bytes.get(4..)?).read_all())
}

/// The shape in a text value: hex (E)WKB, WKT, or a PostgreSQL geometric type.
#[must_use]
pub fn from_text(text: &str) -> Option<Shape> {
    let text = text.trim();
    let hex = text.strip_prefix("\\x").unwrap_or(text);
    if hex.len() >= 10 && hex.len().is_multiple_of(2) && hex.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return from_bytes(&decode_hex(hex)?);
    }
    if text.starts_with(|c: char| c.is_ascii_alphabetic()) {
        let wkt = match text.split_once(';') {
            Some((srid, rest)) if srid.to_ascii_uppercase().starts_with("SRID=") => rest,
            _ => text,
        };
        return Wkt::new(wkt).read_all();
    }
    pg_geometric(text)
}

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Bounds the nesting of collections, so a crafted value cannot exhaust the stack.
const MAX_DEPTH: usize = 32;

struct Wkb<'a> {
    buf: &'a [u8],
    at: usize,
}

impl<'a> Wkb<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, at: 0 }
    }

    fn read_all(mut self) -> Option<Shape> {
        let shape = self.geometry(0)?;
        (self.at == self.buf.len()).then_some(shape)
    }

    fn bytes<const N: usize>(&mut self) -> Option<[u8; N]> {
        let b = self.buf.get(self.at..self.at + N)?.try_into().ok()?;
        self.at += N;
        Some(b)
    }

    fn u32(&mut self, little: bool) -> Option<u32> {
        let b = self.bytes::<4>()?;
        Some(if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    }

    fn f64(&mut self, little: bool) -> Option<f64> {
        let b = self.bytes::<8>()?;
        Some(if little {
            f64::from_le_bytes(b)
        } else {
            f64::from_be_bytes(b)
        })
    }

    fn geometry(&mut self, depth: usize) -> Option<Shape> {
        if depth > MAX_DEPTH {
            return None;
        }
        let little = match self.bytes::<1>()?[0] {
            0 => false,
            1 => true,
            _ => return None,
        };
        let code = self.u32(little)?;
        // EWKB flags in the high bits; ISO WKB adds 1000 (Z), 2000 (M) or 3000 (ZM).
        let (ewkb_z, ewkb_m, srid) = (
            code & 0x8000_0000 != 0,
            code & 0x4000_0000 != 0,
            code & 0x2000_0000 != 0,
        );
        let code = code & 0x0FFF_FFFF;
        let (kind, iso) = (code % 1000, code / 1000);
        if iso > 3 {
            return None;
        }
        let extra = usize::from(ewkb_z || iso == 1 || iso == 3)
            + usize::from(ewkb_m || iso == 2 || iso == 3);
        if srid {
            self.u32(little)?;
        }
        let point = |r: &mut Self| -> Option<[f64; 2]> {
            let xy = [r.f64(little)?, r.f64(little)?];
            for _ in 0..extra {
                r.f64(little)?;
            }
            Some(xy)
        };
        let points = |r: &mut Self| -> Option<Vec<[f64; 2]>> {
            let n = r.u32(little)? as usize;
            // Each point takes at least 16 bytes: a bad count fails before allocating.
            if n > (r.buf.len() - r.at) / 16 {
                return None;
            }
            (0..n).map(|_| point(r)).collect()
        };
        match kind {
            1 => {
                let [x, y] = point(self)?;
                // An empty point is NaN, NaN: an empty shape, as `POINT EMPTY` in WKT.
                Some(if x.is_nan() {
                    Shape::Collection { items: Vec::new() }
                } else {
                    Shape::Point { x, y }
                })
            }
            2 => Some(Shape::Line {
                points: points(self)?,
            }),
            3 => {
                let n = self.u32(little)?;
                let rings = (0..n).map(|_| points(self)).collect::<Option<Vec<_>>>()?;
                Some(Shape::Polygon { rings })
            }
            4..=7 => {
                let n = self.u32(little)?;
                let items = (0..n)
                    .map(|_| self.geometry(depth + 1))
                    .collect::<Option<Vec<_>>>()?;
                Some(Shape::Collection { items })
            }
            _ => None,
        }
    }
}

struct Wkt<'a> {
    rest: &'a str,
}

impl<'a> Wkt<'a> {
    fn new(text: &'a str) -> Self {
        Self { rest: text }
    }

    fn read_all(mut self) -> Option<Shape> {
        let shape = self.geometry(0)?;
        self.rest.trim().is_empty().then_some(shape)
    }

    fn skip_space(&mut self) {
        self.rest = self.rest.trim_start();
    }

    fn eat(&mut self, c: char) -> bool {
        self.skip_space();
        match self.rest.strip_prefix(c) {
            Some(rest) => {
                self.rest = rest;
                true
            }
            None => false,
        }
    }

    fn word(&mut self) -> String {
        self.skip_space();
        let end = self
            .rest
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(self.rest.len());
        let (word, rest) = self.rest.split_at(end);
        self.rest = rest;
        word.to_ascii_uppercase()
    }

    /// Skips the next word if it is one of `words`.
    fn eat_word(&mut self, words: &[&str]) -> bool {
        let save = self.rest;
        let found = words.contains(&self.word().as_str());
        if !found {
            self.rest = save;
        }
        found
    }

    fn number(&mut self) -> Option<f64> {
        self.skip_space();
        let end = self
            .rest
            .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
            .unwrap_or(self.rest.len());
        let (n, rest) = self.rest.split_at(end);
        self.rest = rest;
        n.parse().ok()
    }

    /// `x y` with any further ordinates skipped.
    fn point(&mut self) -> Option<[f64; 2]> {
        let xy = [self.number()?, self.number()?];
        self.skip_space();
        while self
            .rest
            .starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '.')
        {
            self.number()?;
            self.skip_space();
        }
        Some(xy)
    }

    /// `(item, item, …)`; `None` when it does not parse.
    fn list<T>(&mut self, mut item: impl FnMut(&mut Self) -> Option<T>) -> Option<Vec<T>> {
        if !self.eat('(') {
            return None;
        }
        let mut items = vec![item(self)?];
        while self.eat(',') {
            items.push(item(self)?);
        }
        self.eat(')').then_some(items)
    }

    fn points(&mut self) -> Option<Vec<[f64; 2]>> {
        self.list(Self::point)
    }

    /// A point of a multi-point, with or without its own parentheses.
    fn multi_point(&mut self) -> Option<Shape> {
        self.skip_space();
        let [x, y] = if self.rest.starts_with('(') {
            self.points()?.into_iter().next()?
        } else {
            self.point()?
        };
        Some(Shape::Point { x, y })
    }

    fn geometry(&mut self, depth: usize) -> Option<Shape> {
        if depth > MAX_DEPTH {
            return None;
        }
        let kind = self.word();
        // Z, M or ZM before the coordinates.
        self.eat_word(&["Z", "M", "ZM"]);
        if self.eat_word(&["EMPTY"]) {
            return Some(Shape::Collection { items: Vec::new() });
        }
        match kind.as_str() {
            "POINT" => {
                let [x, y] = self.points()?.into_iter().next()?;
                Some(Shape::Point { x, y })
            }
            "LINESTRING" => Some(Shape::Line {
                points: self.points()?,
            }),
            "POLYGON" => Some(Shape::Polygon {
                rings: self.list(Self::points)?,
            }),
            "MULTIPOINT" => Some(Shape::Collection {
                items: self.list(Self::multi_point)?,
            }),
            "MULTILINESTRING" => Some(Shape::Collection {
                items: self
                    .list(Self::points)?
                    .into_iter()
                    .map(|points| Shape::Line { points })
                    .collect(),
            }),
            "MULTIPOLYGON" => Some(Shape::Collection {
                items: self
                    .list(|w| w.list(Self::points))?
                    .into_iter()
                    .map(|rings| Shape::Polygon { rings })
                    .collect(),
            }),
            "GEOMETRYCOLLECTION" => Some(Shape::Collection {
                items: self.list(|w| w.geometry(depth + 1))?,
            }),
            _ => None,
        }
    }
}

/// PostgreSQL's `point`, `box`, `lseg`, `path`, `polygon` and `circle` as text; `line` is
/// infinite and has no shape.
fn pg_geometric(text: &str) -> Option<Shape> {
    if let Some(inner) = text.strip_prefix('<').and_then(|t| t.strip_suffix('>')) {
        let (center, r) = inner.rsplit_once(',')?;
        let [[x, y]] = pg_points(center)?[..] else {
            return None;
        };
        return Some(Shape::Circle {
            x,
            y,
            r: r.trim().parse().ok()?,
        });
    }
    let points = pg_points(text)?;
    if text.starts_with('[') {
        // lseg and open path.
        return Some(Shape::Line { points });
    }
    match points[..] {
        [[x, y]] => Some(Shape::Point { x, y }),
        // A box is two corners without surrounding parentheses.
        [[x1, y1], [x2, y2]] if !text.starts_with("((") => Some(Shape::Polygon {
            rings: vec![vec![[x1, y1], [x2, y1], [x2, y2], [x1, y2], [x1, y1]]],
        }),
        _ => Some(Shape::Polygon {
            rings: vec![points],
        }),
    }
}

/// Every `(x,y)` in the text, which holds nothing else but brackets and commas.
fn pg_points(text: &str) -> Option<Vec<[f64; 2]>> {
    let mut points = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('(') {
        if rest[..open].contains(|c: char| !matches!(c, '(' | '[' | ',' | ' ')) {
            return None;
        }
        let close = open + rest[open..].find(')')?;
        let inner = &rest[open + 1..close];
        if inner.contains('(') {
            rest = &rest[open + 1..];
            continue;
        }
        let (x, y) = inner.split_once(',')?;
        points.push([x.trim().parse().ok()?, y.trim().parse().ok()?]);
        rest = &rest[close + 1..];
    }
    let tail_ok = rest.chars().all(|c| matches!(c, ')' | ']' | ' '));
    (!points.is_empty() && tail_ok).then_some(points)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f64, y: f64) -> Shape {
        Shape::Point { x, y }
    }

    /// WKB of a little-endian point.
    fn wkb_point(x: f64, y: f64) -> Vec<u8> {
        let mut b = vec![1];
        b.extend(1_u32.to_le_bytes());
        b.extend(x.to_le_bytes());
        b.extend(y.to_le_bytes());
        b
    }

    #[test]
    fn reads_wkb_ewkb_and_mysql() {
        assert_eq!(from_bytes(&wkb_point(1.5, -2.0)), Some(point(1.5, -2.0)));
        // EWKB point with SRID 4326, big-endian, as PostGIS sends it in hex.
        let ewkb = "\\x0020000001000010e63ff00000000000004000000000000000";
        assert_eq!(from_text(ewkb), Some(point(1.0, 2.0)));
        // MySQL: SRID 0 in four little-endian bytes, then WKB.
        let mut mysql = 0_u32.to_le_bytes().to_vec();
        mysql.extend(wkb_point(3.0, 4.0));
        assert_eq!(from_bytes(&mysql), Some(point(3.0, 4.0)));
        // ISO WKB point Z (1001): z is skipped.
        let mut z = vec![1];
        z.extend(1001_u32.to_le_bytes());
        for v in [1.0_f64, 2.0, 9.0] {
            z.extend(v.to_le_bytes());
        }
        assert_eq!(from_bytes(&z), Some(point(1.0, 2.0)));
    }

    #[test]
    fn reads_wkb_lines_polygons_and_collections() {
        let mut line = vec![1];
        line.extend(2_u32.to_le_bytes());
        line.extend(2_u32.to_le_bytes());
        for v in [0.0_f64, 0.0, 1.0, 1.0] {
            line.extend(v.to_le_bytes());
        }
        let expected = Shape::Line {
            points: vec![[0.0, 0.0], [1.0, 1.0]],
        };
        assert_eq!(from_bytes(&line), Some(expected.clone()));
        let mut multi = vec![1];
        multi.extend(7_u32.to_le_bytes());
        multi.extend(2_u32.to_le_bytes());
        multi.extend(wkb_point(5.0, 6.0));
        multi.extend(&line);
        assert_eq!(
            from_bytes(&multi),
            Some(Shape::Collection {
                items: vec![point(5.0, 6.0), expected]
            })
        );
    }

    #[test]
    fn reads_empty_points_inside_collections() {
        // GEOMETRYCOLLECTION (POINT EMPTY, POINT (1 2))
        let mut multi = vec![1];
        multi.extend(7_u32.to_le_bytes());
        multi.extend(2_u32.to_le_bytes());
        multi.extend(wkb_point(f64::NAN, f64::NAN));
        multi.extend(wkb_point(1.0, 2.0));
        assert_eq!(
            from_bytes(&multi),
            Some(Shape::Collection {
                items: vec![Shape::Collection { items: vec![] }, point(1.0, 2.0)]
            })
        );
        // A bad item fails the whole value instead of being skipped.
        let mut bad = multi[..9].to_vec();
        bad.extend([1, 99, 0, 0, 0]);
        bad.extend(wkb_point(1.0, 2.0));
        assert_eq!(from_bytes(&bad), None);
    }

    #[test]
    fn refuses_bad_binary() {
        assert_eq!(from_bytes(&[]), None);
        assert_eq!(from_bytes(&[1, 2, 0, 0, 0]), None);
        // A count far beyond the bytes there are.
        let mut huge = vec![1];
        huge.extend(2_u32.to_le_bytes());
        huge.extend(u32::MAX.to_le_bytes());
        assert_eq!(from_bytes(&huge), None);
        // Trailing bytes after a point.
        let mut extra = wkb_point(1.0, 1.0);
        extra.push(0);
        assert_eq!(from_bytes(&extra), None);
    }

    #[test]
    fn reads_wkt() {
        assert_eq!(from_text("POINT (1 2)"), Some(point(1.0, 2.0)));
        assert_eq!(
            from_text("SRID=4326;point z (1 2 3)"),
            Some(point(1.0, 2.0))
        );
        assert_eq!(
            from_text("POLYGON((0 0,4 0,4 4,0 0),(1 1,2 1,2 2,1 1))"),
            Some(Shape::Polygon {
                rings: vec![
                    vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 0.0]],
                    vec![[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 1.0]],
                ]
            })
        );
        assert_eq!(
            from_text("MULTIPOINT (1 2, (3 4))"),
            Some(Shape::Collection {
                items: vec![point(1.0, 2.0), point(3.0, 4.0)]
            })
        );
        assert_eq!(
            from_text("GEOMETRYCOLLECTION (POINT (1 2), LINESTRING (0 0, 1 1))"),
            Some(Shape::Collection {
                items: vec![
                    point(1.0, 2.0),
                    Shape::Line {
                        points: vec![[0.0, 0.0], [1.0, 1.0]]
                    }
                ]
            })
        );
        assert_eq!(
            from_text("POINT EMPTY"),
            Some(Shape::Collection { items: vec![] })
        );
        for bad in [
            "POINT (1)",
            "POINT (1 2",
            "CURVE (1 2)",
            "hello",
            "POINT (1 2) x",
        ] {
            assert_eq!(from_text(bad), None, "{bad}");
        }
    }

    #[test]
    fn reads_postgres_geometric_types() {
        assert_eq!(from_text("(1,2)"), Some(point(1.0, 2.0)));
        assert_eq!(
            from_text("(2,3),(0,-1)"),
            Some(Shape::Polygon {
                rings: vec![vec![
                    [2.0, 3.0],
                    [0.0, 3.0],
                    [0.0, -1.0],
                    [2.0, -1.0],
                    [2.0, 3.0]
                ]]
            })
        );
        assert_eq!(
            from_text("[(0,0),(2.5,1)]"),
            Some(Shape::Line {
                points: vec![[0.0, 0.0], [2.5, 1.0]]
            })
        );
        assert_eq!(
            from_text("((0,0),(1,0),(1,1))"),
            Some(Shape::Polygon {
                rings: vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]]
            })
        );
        assert_eq!(
            from_text("<(1,2),3>"),
            Some(Shape::Circle {
                x: 1.0,
                y: 2.0,
                r: 3.0
            })
        );
        assert_eq!(from_text("{1,-1,0}"), None);
        assert_eq!(from_text("(1,x)"), None);
    }
}

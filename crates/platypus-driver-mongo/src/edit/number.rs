//! The numbers of the grid's cells, which an edit writes in the numeric type the document
//! already stores when that type holds the number exactly: a column's type is only what its
//! documents share (ints and longs make a `long` column, any of them with doubles a `double`
//! one), so it does not say what one document holds.

use std::str::FromStr;

use mongodb::bson::{Bson, Decimal128, Document, doc};

/// The numeric types, by their `$type` aliases.
pub const NUMERIC: [&str; 4] = ["int", "long", "double", "decimal"];

/// A number, in each numeric type that holds it exactly.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Exact {
    int: Option<i32>,
    long: Option<i64>,
    double: Option<f64>,
    decimal: Option<Decimal128>,
}

/// The digits of a decimal number's text: its sign, its significant digits and the power of
/// ten they are multiplied by. `-12.50e1` is `(true, "125", 0)`; zero has no digits. `None` for
/// a text that is not a decimal number.
fn parts(text: &str) -> Option<(bool, String, i64)> {
    let negative = text.starts_with('-');
    let rest = text.strip_prefix(['-', '+']).unwrap_or(text);
    let (mantissa, exponent) = match rest.find(['e', 'E']) {
        Some(i) => (&rest[..i], rest[i + 1..].parse::<i64>().ok()?),
        None => (rest, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{whole}{fraction}");
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let significant = digits.trim_end_matches('0');
    let trailing = digits.len() - significant.len();
    let significant = significant.trim_start_matches('0');
    if significant.is_empty() {
        return Some((negative, String::new(), 0));
    }
    let exponent = exponent
        .checked_sub(i64::try_from(fraction.len()).ok()?)?
        .checked_add(i64::try_from(trailing).ok()?)?;
    Some((negative, significant.to_owned(), exponent))
}

/// The special doubles, as the grid shows them.
fn special(text: &str) -> Option<f64> {
    match text {
        "NaN" | "nan" => Some(f64::NAN),
        "Infinity" | "inf" | "+inf" => Some(f64::INFINITY),
        "-Infinity" | "-inf" => Some(f64::NEG_INFINITY),
        _ => None,
    }
}

impl Exact {
    /// The number `text` writes; `None` when it is not one.
    #[must_use]
    pub fn of(text: &str) -> Option<Self> {
        if let Some(d) = special(text) {
            return Some(Self {
                double: Some(d),
                ..Self::default()
            });
        }
        let (negative, digits, exponent) = parts(text)?;
        let long = if digits.is_empty() {
            Some(0)
        } else {
            usize::try_from(exponent)
                .ok()
                .filter(|&zeros| digits.len() + zeros <= 19)
                .and_then(|zeros| {
                    let sign = if negative { "-" } else { "" };
                    format!("{sign}{digits}{}", "0".repeat(zeros)).parse().ok()
                })
        };
        // The double the text names, written back shortest, is the same number.
        let double = text
            .parse::<f64>()
            .ok()
            .filter(|d| d.is_finite())
            .filter(|d| parts(&format!("{d:e}")) == Some((negative, digits.clone(), exponent)));
        let decimal = (digits.len() <= 34)
            .then(|| Decimal128::from_str(text).ok())
            .flatten();
        Some(Self {
            int: long.and_then(|n| i32::try_from(n).ok()),
            long,
            double,
            decimal,
        })
    }

    /// The number as a value of numeric type `ty`, when that type holds it.
    #[must_use]
    pub fn as_type(&self, ty: &str) -> Option<Bson> {
        match ty {
            "int" => self.int.map(Bson::Int32),
            "long" => self.long.map(Bson::Int64),
            "double" => self.double.map(Bson::Double),
            "decimal" => self.decimal.map(Bson::Decimal128),
            _ => None,
        }
    }

    /// The number in type `ty` when it holds it, else in the first numeric type that does.
    #[must_use]
    pub fn by_type(&self, ty: &str) -> Option<Bson> {
        std::iter::once(ty)
            .chain(NUMERIC)
            .find_map(|t| self.as_type(t))
    }

    /// The numeric types that do not hold it.
    #[must_use]
    pub fn missing(&self) -> Vec<&'static str> {
        NUMERIC
            .into_iter()
            .filter(|t| self.as_type(t).is_none())
            .collect()
    }

    /// The aggregation expression that writes it to field `field` in the numeric type the
    /// field holds, when that type holds it, else in type `ty` ([`Self::by_type`]).
    #[must_use]
    pub fn kept(&self, field: &str, ty: &str) -> Option<Document> {
        let default = self.by_type(ty)?;
        let branches: Vec<Bson> = NUMERIC
            .into_iter()
            .filter_map(|t| {
                let then = self.as_type(t)?;
                Some(Bson::Document(doc! {
                    "case": { "$eq": [{ "$type": format!("${field}") }, t] },
                    "then": then,
                }))
            })
            .collect();
        Some(doc! { "$switch": { "branches": branches, "default": default } })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn types(text: &str) -> Vec<&'static str> {
        let n = Exact::of(text).unwrap();
        NUMERIC
            .into_iter()
            .filter(|t| n.as_type(t).is_some())
            .collect()
    }

    #[test]
    fn a_number_is_in_the_types_that_hold_it_exactly() {
        assert_eq!(types("5"), NUMERIC);
        assert_eq!(types("5.0"), NUMERIC);
        assert_eq!(types("-0"), NUMERIC);
        assert_eq!(types("3.5"), ["double", "decimal"]);
        assert_eq!(types("1099511627776"), ["long", "double", "decimal"]);
        assert_eq!(types("9007199254740993"), ["long", "decimal"]);
        assert_eq!(types("0.1"), ["double", "decimal"]);
        assert_eq!(types("1e21"), ["double", "decimal"]);
        assert_eq!(types("0.123456789012345678901"), ["decimal"]);
        assert_eq!(types("NaN"), ["double"]);
        assert_eq!(types("-inf"), ["double"]);
        for text in ["", "abc", "1,5", "0x10", "1e", "--1", "."] {
            assert_eq!(Exact::of(text), None, "{text}");
        }
    }

    #[test]
    fn the_values_are_the_number() {
        let n = Exact::of("-12.50e1").unwrap();
        assert_eq!(n.as_type("int"), Some(Bson::Int32(-125)));
        assert_eq!(n.as_type("long"), Some(Bson::Int64(-125)));
        assert_eq!(n.as_type("double"), Some(Bson::Double(-125.0)));
        let Some(Bson::Double(zero)) = Exact::of("-0").unwrap().as_type("double") else {
            panic!()
        };
        assert!(zero == 0.0 && zero.is_sign_negative());
        assert_eq!(
            Exact::of("1.10")
                .unwrap()
                .as_type("decimal")
                .unwrap()
                .to_string(),
            "1.10"
        );
    }

    #[test]
    fn falls_back_to_the_first_type_that_holds_it() {
        let by = |text: &str, ty: &str| Exact::of(text).unwrap().by_type(ty);
        assert_eq!(by("3.5", "int"), Some(Bson::Double(3.5)));
        assert_eq!(by("3000000000", "int"), Some(Bson::Int64(3_000_000_000)));
        assert_eq!(by("5", "double"), Some(Bson::Double(5.0)));
        assert!(matches!(
            by("0.123456789012345678901", "double"),
            Some(Bson::Decimal128(_))
        ));
    }

    #[test]
    fn the_expression_has_a_branch_per_type_that_holds_it() {
        let kept = Exact::of("3.5").unwrap().kept("qty", "int").unwrap();
        let switch = kept.get_document("$switch").unwrap();
        let branches = switch.get_array("branches").unwrap();
        let cases: Vec<&str> = branches
            .iter()
            .map(|b| {
                let case = b.as_document().unwrap().get_document("case").unwrap();
                let eq = case.get_array("$eq").unwrap();
                assert_eq!(eq[0], Bson::Document(doc! { "$type": "$qty" }));
                eq[1].as_str().unwrap()
            })
            .collect();
        assert_eq!(cases, ["double", "decimal"]);
        assert_eq!(switch.get("default"), Some(&Bson::Double(3.5)));
    }
}

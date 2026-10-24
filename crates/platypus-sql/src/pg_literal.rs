//! PostgreSQL array (`{a,"b c",NULL}`) and composite (`(1,"x",)`) literals, split into their
//! elements for the data editor and joined back. Elements are text as the grid shows them;
//! `None` is NULL. Only one-dimensional arrays are split: nested ones are edited as text.

/// The elements of a one-dimensional array literal, or `None` when the text is not one
/// (nested arrays, explicit bounds such as `[0:1]={…}`, malformed text).
#[must_use]
pub fn split_array(text: &str) -> Option<Vec<Option<String>>> {
    let inner = text.trim().strip_prefix('{')?.strip_suffix('}')?;
    let mut items = Vec::new();
    if inner.trim().is_empty() {
        return Some(items);
    }
    let mut chars = inner.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let item = if chars.next_if_eq(&'"').is_some() {
            let mut value = String::new();
            loop {
                match chars.next()? {
                    '"' => break,
                    '\\' => value.push(chars.next()?),
                    c => value.push(c),
                }
            }
            while chars.next_if(|c| c.is_whitespace()).is_some() {}
            Some(value)
        } else {
            let mut value = String::new();
            while let Some(c) = chars.next_if(|c| *c != ',') {
                match c {
                    '{' | '}' | '"' => return None,
                    '\\' => value.push(chars.next()?),
                    c => value.push(c),
                }
            }
            let value = value.trim_end();
            if value.is_empty() {
                return None;
            }
            (!value.eq_ignore_ascii_case("null")).then(|| value.to_owned())
        };
        items.push(item);
        match chars.next() {
            None => return Some(items),
            Some(',') => {}
            Some(_) => return None,
        }
    }
}

/// An array literal of `items`, quoting the elements that need it.
#[must_use]
pub fn join_array(items: &[Option<String>]) -> String {
    join(items, ('{', '}'), "NULL")
}

/// The fields of a composite literal, or `None` when the text is not one.
#[must_use]
pub fn split_composite(text: &str) -> Option<Vec<Option<String>>> {
    let inner = text.trim().strip_prefix('(')?.strip_suffix(')')?;
    let mut fields = Vec::new();
    let mut chars = inner.chars().peekable();
    loop {
        // A field is any mix of quoted and plain parts; nothing at all is NULL.
        let mut value = String::new();
        let mut quoted = false;
        while let Some(c) = chars.next_if(|c| *c != ',') {
            match c {
                '"' => {
                    quoted = true;
                    loop {
                        match chars.next()? {
                            // `""` inside quotes is a quote.
                            '"' if chars.next_if_eq(&'"').is_some() => value.push('"'),
                            '"' => break,
                            '\\' => value.push(chars.next()?),
                            c => value.push(c),
                        }
                    }
                }
                '\\' => value.push(chars.next()?),
                '(' | ')' => return None,
                c => value.push(c),
            }
        }
        fields.push((quoted || !value.is_empty()).then_some(value));
        if chars.next().is_none() {
            return Some(fields);
        }
    }
}

/// A composite literal of `fields`, quoting the ones that need it.
#[must_use]
pub fn join_composite(fields: &[Option<String>]) -> String {
    join(fields, ('(', ')'), "")
}

/// `items` between `brackets`, with `null` for NULL. Values that read as `null` or hold
/// whitespace, a separator, a quote, a backslash or a bracket are quoted.
fn join(items: &[Option<String>], brackets: (char, char), null: &str) -> String {
    let (open, close) = brackets;
    let mut out = String::from(open);
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        match item {
            None => out.push_str(null),
            Some(v) if needs_quotes(v, brackets, null) => push_quoted(&mut out, v),
            Some(v) => out.push_str(v),
        }
    }
    out.push(close);
    out
}

fn needs_quotes(v: &str, (open, close): (char, char), null: &str) -> bool {
    v.is_empty()
        || v.eq_ignore_ascii_case(null)
        || v.chars()
            .any(|c| matches!(c, ',' | '"' | '\\') || c == open || c == close || c.is_whitespace())
}

fn push_quoted(out: &mut String, v: &str) {
    out.push('"');
    for c in v.chars() {
        if matches!(c, '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn some(items: &[&str]) -> Vec<Option<String>> {
        items.iter().map(|s| Some((*s).to_owned())).collect()
    }

    #[test]
    fn splits_arrays() {
        assert_eq!(split_array("{}"), Some(vec![]));
        assert_eq!(split_array("{1,2,3}"), Some(some(&["1", "2", "3"])));
        assert_eq!(
            split_array(r#"{a,"b c",NULL,"NULL","",null, x y }"#),
            Some(vec![
                Some("a".into()),
                Some("b c".into()),
                None,
                Some("NULL".into()),
                Some(String::new()),
                None,
                Some("x y".into()),
            ])
        );
        assert_eq!(
            split_array(r#"{"q\"x","back\\slash",a\,b}"#),
            Some(some(&["q\"x", "back\\slash", "a,b"]))
        );
    }

    #[test]
    fn refuses_what_is_not_a_flat_array() {
        for text in [
            "{{1,2},{3,4}}",
            "[0:1]={1,2}",
            "{1,}",
            "{,1}",
            "1,2",
            r#"{"open}"#,
            "{a b\"c}",
        ] {
            assert_eq!(split_array(text), None, "{text}");
        }
    }

    #[test]
    fn joins_arrays_that_split_back() {
        let items = vec![
            Some("plain".to_owned()),
            Some("with space".to_owned()),
            Some("q\"x\\y".to_owned()),
            Some("{braces},".to_owned()),
            Some("null".to_owned()),
            Some(String::new()),
            None,
        ];
        let text = join_array(&items);
        assert_eq!(
            text,
            r#"{plain,"with space","q\"x\\y","{braces},","null","",NULL}"#
        );
        assert_eq!(split_array(&text), Some(items));
        assert_eq!(join_array(&[]), "{}");
    }

    #[test]
    fn splits_composites() {
        assert_eq!(
            split_composite(r#"(1,"a b",,"",x)"#),
            Some(vec![
                Some("1".into()),
                Some("a b".into()),
                None,
                Some(String::new()),
                Some("x".into()),
            ])
        );
        // PostgreSQL doubles quotes and backslashes; the grid escapes them with a backslash.
        assert_eq!(
            split_composite(r#"("say ""hi""","a\\b","c\"d")"#),
            Some(some(&["say \"hi\"", "a\\b", "c\"d"]))
        );
        assert_eq!(split_composite("()"), Some(vec![None]));
        assert_eq!(split_composite(" (a,b) "), Some(some(&["a", "b"])));
        assert_eq!(split_composite("(1,(2))"), None);
        assert_eq!(split_composite("1,2"), None);
    }

    #[test]
    fn joins_composites_that_split_back() {
        let fields = vec![
            Some("1".to_owned()),
            None,
            Some(String::new()),
            Some("a (b), \"c\" \\".to_owned()),
        ];
        let text = join_composite(&fields);
        assert_eq!(text, r#"(1,,"","a (b), \"c\" \\")"#);
        assert_eq!(split_composite(&text), Some(fields));
    }
}

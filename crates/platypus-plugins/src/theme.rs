//! What a plugin's theme may hold. A value reaches the page's CSS, so it is held to a narrow
//! grammar rather than asked of a CSS parser: a colour and nothing else, with no way to name a
//! resource or to leave the declaration. Fonts are files of the package, never names or URLs.
//!
//! The page checks the same again before painting (`apps/desktop/src/themes/color.ts`) and sets
//! each value through `style.setProperty`.

use std::collections::BTreeMap;

use crate::extensions::ThemeDecl;

/// The colours a theme may set: `PALETTE_KEYS` of `apps/desktop/src/theme.ts`, which a test of
/// the page compares with this list.
pub const TOKENS: &[&str] = &[
    "bg",
    "bg-sidebar",
    "bg-input",
    "fg",
    "fg-muted",
    "border",
    "accent",
    "accent-fg",
    "hover",
    "error",
    "error-bg",
    "ok",
    "ok-bg",
    "warn-bg",
    "grid-bg",
    "grid-bg-alt",
    "grid-header",
    "grid-selection",
    "syn-keyword",
    "syn-string",
    "syn-number",
    "syn-comment",
    "syn-type",
    "syn-ident",
    "bg-rail",
    "bg-tabbar",
    "fg-strong",
    "border-strong",
    "active",
    "obj-db",
    "obj-table",
    "obj-view",
    "obj-routine",
    "obj-trigger",
    "type-dev",
    "type-dev-bg",
    "type-test",
    "type-test-bg",
    "type-prod",
    "type-prod-bg",
    "warn",
    "frame-dev",
    "frame-test",
    "frame-prod",
];

/// What a font of a theme is for.
pub const FONT_USES: &[&str] = &["ui", "mono"];
/// The largest font file of a package.
pub const MAX_FONT: u64 = 4 << 20;
/// The longest colour value.
const MAX_COLOR: usize = 64;
/// The functions a colour may use.
const COLOR_FUNCTIONS: &[&str] = &["rgb(", "rgba(", "hsl(", "hsla("];
/// The font formats a package may hold.
const FONT_EXTENSIONS: &[&str] = &["woff2", "woff", "ttf", "otf"];

/// A colour: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`, `rgba()`, `hsl()`, `hsla()` with
/// numbers only, or `transparent`. Everything else is refused, whatever a browser would make of
/// it: `url(...)`, `expression(...)`, `var(...)`, a `;` or a `}`.
#[must_use]
pub fn is_color(value: &str) -> bool {
    if value.len() > MAX_COLOR {
        return false;
    }
    if value == "transparent" {
        return true;
    }
    if let Some(hex) = value.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    COLOR_FUNCTIONS
        .iter()
        .find_map(|function| value.strip_prefix(function))
        .and_then(|rest| rest.strip_suffix(')'))
        .is_some_and(|args| {
            args.bytes().any(|b| b.is_ascii_digit())
                && args.bytes().all(|b| {
                    b.is_ascii_digit() || matches!(b, b'.' | b',' | b'%' | b'/' | b' ' | b'-')
                })
        })
}

/// Why a theme cannot be installed; `Ok` when it can. The name of the token or value is given, so
/// that the author sees which one.
pub(crate) fn check(theme: &ThemeDecl) -> Result<(), String> {
    for (variant, colors) in [("light", &theme.light), ("dark", &theme.dark)] {
        check_colors(variant, colors)?;
    }
    for (usage, path) in &theme.fonts {
        if !FONT_USES.contains(&usage.as_str()) {
            return Err(format!(
                "the theme has a font for {usage:?}: only for {FONT_USES:?}"
            ));
        }
        if !is_font_path(path) {
            return Err(format!(
                "the theme's font {path:?} is not a {FONT_EXTENSIONS:?} file inside the package"
            ));
        }
    }
    Ok(())
}

fn check_colors(variant: &str, colors: &BTreeMap<String, String>) -> Result<(), String> {
    for (token, value) in colors {
        if !TOKENS.contains(&token.as_str()) {
            return Err(format!("the theme has an unknown colour {variant}.{token}"));
        }
        if !is_color(value) {
            return Err(format!(
                "{variant}.{token} = {value:?} is not a colour (#rrggbb, rgb(), rgba(), hsl() or hsla(), with numbers)"
            ));
        }
    }
    Ok(())
}

/// A relative path that stays inside the package, to a file of a font format.
fn is_font_path(path: &str) -> bool {
    let extension = path.rsplit('.').next().unwrap_or_default();
    path.len() <= 200
        && FONT_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        && path.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
}

/// The file at `path` is a font, by what it starts with and not only by its name.
pub(crate) fn check_font(path: &str, bytes: &[u8]) -> Result<(), String> {
    let known: [&[u8]; 6] = [b"wOF2", b"wOFF", &[0, 1, 0, 0], b"true", b"typ1", b"OTTO"];
    if known.iter().any(|magic| bytes.starts_with(magic)) {
        Ok(())
    } else {
        Err(format!("{path} is not a font file"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme(dark: &[(&str, &str)], fonts: &[(&str, &str)]) -> ThemeDecl {
        let map = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect()
        };
        ThemeDecl {
            name: "T".to_owned(),
            light: BTreeMap::new(),
            dark: map(dark),
            fonts: map(fonts),
        }
    }

    #[test]
    fn accepts_colours_and_nothing_else() {
        for good in [
            "#fff",
            "#ffff",
            "#0B1D26",
            "#0b1d26cc",
            "rgb(1, 2, 3)",
            "rgb(1 2 3 / 50%)",
            "rgba(255, 141, 40, 0.14)",
            "hsl(200, 50%, 40%)",
            "hsla(200, 50%, 40%, .5)",
            "transparent",
        ] {
            assert!(is_color(good), "{good}");
        }
        for bad in [
            "",
            "red",
            "#ggg",
            "#12345",
            " #fff",
            "#fff ",
            "url(https://example.com/x.png)",
            "URL(x)",
            "expression(alert(1))",
            "var(--x)",
            "#fff; background: url(x)",
            "#fff}body{display:none",
            "rgb(1,2,3); color: red",
            "rgb(1,2,3)) url(x",
            "rgb(url(x))",
            "rgb()",
            "rgb(,)",
            "rgb(1,2,3",
            "rgb(1\\2c 2,3)",
            "color-mix(in srgb, #000 20%, #fff)",
            "/* */ #fff",
        ] {
            assert!(!is_color(bad), "{bad}");
        }
        assert!(!is_color(&format!("rgb({})", "1,".repeat(40))));
    }

    #[test]
    fn checks_tokens_and_values_by_name() {
        assert!(check(&theme(&[("bg", "#000"), ("accent", "#fff")], &[])).is_ok());
        let e = check(&theme(&[("glow", "#000")], &[])).unwrap_err();
        assert!(e.contains("dark.glow"), "{e}");
        let e = check(&theme(&[("bg", "url(x)")], &[])).unwrap_err();
        assert!(e.contains("dark.bg") && e.contains("not a colour"), "{e}");
        // The list is unique: a test of the page compares it with the app's.
        let mut sorted = TOKENS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), TOKENS.len());
    }

    #[test]
    fn fonts_are_files_of_the_package() {
        let ok = theme(&[("bg", "#000")], &[("mono", "fonts/Sea-Mono.woff2")]);
        assert!(check(&ok).is_ok());
        for (usage, path, why) in [
            ("title", "a.woff2", "for \"title\""),
            ("mono", "Fira Code", "not a"),
            ("mono", "../a.woff2", "not a"),
            ("mono", "/a.woff2", "not a"),
            ("mono", "fonts\\a.woff2", "not a"),
            ("mono", "https://example.com/a.woff2", "not a"),
            ("mono", "a.svg", "not a"),
            ("mono", "fonts/.a.woff2", "not a"),
        ] {
            let e = check(&theme(&[("bg", "#000")], &[(usage, path)])).unwrap_err();
            assert!(e.contains(why), "{path}: {e}");
        }
        assert!(check_font("a.woff2", b"wOF2....").is_ok());
        assert!(check_font("a.ttf", &[0, 1, 0, 0, 9]).is_ok());
        let e = check_font("a.woff2", b"<svg onload=x>").unwrap_err();
        assert!(e.contains("not a font"), "{e}");
    }
}

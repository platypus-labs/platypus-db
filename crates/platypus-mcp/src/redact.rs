//! Keeps secrets out of what agents read: a driver or a password command may echo a
//! password, or a URL with one, in its error message.

use platypus_core::Secret;

/// Secrets shorter than this are left alone: replacing every `1` or `ab` in a message
/// would garble it without protecting anything.
const MIN_SECRET_LEN: usize = 3;

const MASK: &str = "***";

/// The secrets of one connection, masked wherever they appear.
#[derive(Default, Clone)]
pub struct Redactor {
    secrets: Vec<String>,
}

impl std::fmt::Debug for Redactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Redactor")
            .field("secrets", &self.secrets.len())
            .finish()
    }
}

impl Redactor {
    /// Masks `secret` from now on.
    pub fn add(&mut self, secret: &str) {
        if secret.len() >= MIN_SECRET_LEN && !self.secrets.iter().any(|s| s == secret) {
            self.secrets.push(secret.to_owned());
            // Longest first, so a secret containing another is masked whole.
            self.secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
        }
    }

    pub fn add_secret(&mut self, secret: Option<&Secret>) {
        if let Some(secret) = secret {
            self.add(secret.expose());
        }
    }

    /// `text` with every known secret and the password of every URL masked.
    #[must_use]
    pub fn apply(&self, text: &str) -> String {
        let mut out = mask_url_passwords(text);
        for secret in &self.secrets {
            if out.contains(secret.as_str()) {
                out = out.replace(secret.as_str(), MASK);
            }
        }
        out
    }
}

/// Masks the password of URLs such as `postgres://user:password@host/db`.
fn mask_url_passwords(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("://") {
        let (head, tail) = rest.split_at(i + 3);
        out.push_str(head);
        // The authority ends at the first `/`, `?`, `#` or whitespace.
        let end = tail
            .find(|c: char| matches!(c, '/' | '?' | '#') || c.is_whitespace())
            .unwrap_or(tail.len());
        let authority = &tail[..end];
        match authority.rfind('@') {
            Some(at) => {
                let userinfo = &authority[..at];
                match userinfo.find(':') {
                    Some(colon) => {
                        out.push_str(&userinfo[..=colon]);
                        out.push_str(MASK);
                    }
                    None => out.push_str(userinfo),
                }
                out.push_str(&authority[at..]);
            }
            None => out.push_str(authority),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_known_secrets_and_url_passwords() {
        let mut r = Redactor::default();
        r.add("hunter2");
        r.add("x");
        r.add_secret(Some(&Secret::new("hunter2-and-more")));
        assert_eq!(
            r.apply("password hunter2-and-more and hunter2 for x"),
            "password *** and *** for x"
        );
        assert_eq!(
            r.apply("cannot open postgres://ada:s3cr@t@db:5432/app?sslmode=disable"),
            "cannot open postgres://ada:***@db:5432/app?sslmode=disable"
        );
        assert_eq!(
            r.apply("see https://example.com/a and mysql://bob@h/db"),
            "see https://example.com/a and mysql://bob@h/db"
        );
        assert_eq!(r.apply("no secret here"), "no secret here");
    }
}

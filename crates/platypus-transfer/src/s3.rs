//! Upload of a finished export to a bucket of S3, or of a service that speaks its API (GCS
//! with HMAC keys, Cloudflare R2, MinIO), in one `PUT` signed with AWS Signature Version 4.

use std::fmt::{self, Write as _};
use std::fs::File;
use std::io::Read as _;
use std::path::Path;

use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{ExportError, Result, io_error};

/// A bucket and the keys that write to it.
#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct Bucket {
    /// The service's address, e.g. `https://s3.eu-west-1.amazonaws.com` or
    /// `https://storage.googleapis.com`.
    pub endpoint: String,
    /// e.g. `eu-west-1`; `auto` for GCS and R2.
    pub region: String,
    pub bucket: String,
    /// The folder of the file in the bucket; empty for none.
    #[serde(default)]
    pub prefix: String,
    pub access_key: String,
    pub secret_key: String,
}

impl fmt::Debug for Bucket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bucket")
            .field("endpoint", &self.endpoint)
            .field("region", &self.region)
            .field("bucket", &self.bucket)
            .field("prefix", &self.prefix)
            .field("access_key", &self.access_key)
            .finish_non_exhaustive()
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes keys of any length");
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// The key that signs requests to `service` in `region` on `day` (`YYYYMMDD`).
fn signing_key(secret: &str, day: &str, region: &str, service: &str) -> Vec<u8> {
    let mut key = hmac(format!("AWS4{secret}").as_bytes(), day);
    for part in [region, service, "aws4_request"] {
        key = hmac(&key, part);
    }
    key
}

/// `text` encoded as S3 signs it: unreserved characters as they are, `/` too in a path.
fn encode(text: &str, path: bool) -> String {
    text.bytes().fold(String::new(), |mut s, b| {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) || (path && b == b'/') {
            s.push(char::from(b));
        } else {
            let _ = write!(s, "%{b:02X}");
        }
        s
    })
}

/// The `Authorization` of a `method` request with a body hashing to `payload` at `path` of
/// `host`, at `now` (`YYYYMMDDTHHMMSSZ`), signing the host and the `x-amz-*` headers sent
/// with it.
fn authorization(
    bucket: &Bucket,
    method: &str,
    (host, path): (&str, &str),
    (payload, now): (&str, &str),
) -> String {
    let day = &now[..8];
    let headers = format!("host:{host}\nx-amz-content-sha256:{payload}\nx-amz-date:{now}\n");
    let signed = "host;x-amz-content-sha256;x-amz-date";
    let request = format!("{method}\n{path}\n\n{headers}\n{signed}\n{payload}");
    let scope = format!("{day}/{}/s3/aws4_request", bucket.region);
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{now}\n{scope}\n{}",
        hex(&Sha256::digest(request.as_bytes()))
    );
    let key = signing_key(&bucket.secret_key, day, &bucket.region, "s3");
    format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed}, Signature={}",
        bucket.access_key,
        hex(&hmac(&key, &to_sign))
    )
}

/// The scheme and host (with a port other than the scheme's) of `endpoint`.
fn host(endpoint: &str) -> Result<(&str, &str)> {
    let invalid = || {
        ExportError::Options(format!(
            "the endpoint must be an address like https://s3.eu-west-1.amazonaws.com, not {endpoint:?}"
        ))
    };
    let (scheme, rest) = endpoint.trim_end_matches('/').split_once("://").ok_or_else(invalid)?;
    if !matches!(scheme, "http" | "https") || rest.is_empty() || rest.contains(['/', '?', '@']) {
        return Err(invalid());
    }
    // The client leaves the scheme's own port out of `Host`, which is signed.
    let default = if scheme == "https" { ":443" } else { ":80" };
    Ok((scheme, rest.strip_suffix(default).unwrap_or(rest)))
}

/// The text of `<tag>` in an S3 error body.
fn tag<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let start = xml.find(&format!("<{tag}>"))? + tag.len() + 2;
    let end = xml[start..].find(&format!("</{tag}>"))?;
    Some(&xml[start..start + end])
}

/// Sends the file at `path` to `bucket`, under its own name in the bucket's `prefix`.
pub fn put(path: &Path, bucket: &Bucket) -> Result<()> {
    let (scheme, host) = host(&bucket.endpoint)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("export");
    let prefix = bucket.prefix.trim_matches('/');
    let key = if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    };
    let object = format!("/{}/{}", encode(&bucket.bucket, false), encode(&key, true));
    let mut hash = Sha256::new();
    let mut file = File::open(path).map_err(io_error(path))?;
    let mut chunk = vec![0; 1 << 16];
    loop {
        let n = file.read(&mut chunk).map_err(io_error(path))?;
        if n == 0 {
            break;
        }
        hash.update(&chunk[..n]);
    }
    let payload = hex(&hash.finalize());
    let now = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let file = File::open(path).map_err(io_error(path))?;
    let length = file.metadata().map_err(io_error(path))?.len();
    let failed = |e: &dyn fmt::Display| ExportError::Upload(format!("{scheme}://{host}{object}: {e}"));
    let mut response = ureq::put(format!("{scheme}://{host}{object}"))
        .config()
        .http_status_as_error(false)
        .build()
        .header("x-amz-date", &now)
        .header("x-amz-content-sha256", &payload)
        .header(
            "Authorization",
            authorization(bucket, "PUT", (host, &object), (&payload, &now)),
        )
        .header("Content-Length", length.to_string())
        .send(ureq::SendBody::from_owned_reader(file))
        .map_err(|e| failed(&e))?;
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let body = response.body_mut().read_to_string().unwrap_or_default();
    let reason = match (tag(&body, "Code"), tag(&body, "Message")) {
        (Some(code), Some(message)) => format!("{code}: {message}"),
        _ => status.to_string(),
    };
    Err(failed(&format!("{} {reason}", status.as_u16())))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The SHA-256 of nothing.
    const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    fn bucket() -> Bucket {
        Bucket {
            endpoint: "https://s3.amazonaws.com".to_owned(),
            region: "us-east-1".to_owned(),
            bucket: "examplebucket".to_owned(),
            prefix: String::new(),
            access_key: "AKIAIOSFODNN7EXAMPLE".to_owned(),
            secret_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_owned(),
        }
    }

    #[test]
    fn signs_like_aws() {
        let auth = authorization(
            &bucket(),
            "PUT",
            ("s3.amazonaws.com", "/examplebucket/a%20b/c.csv"),
            (EMPTY, "20130524T000000Z"),
        );
        assert!(
            auth.starts_with(
                "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, \
                 SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature="
            ),
            "{auth}"
        );
        let signature = auth.rsplit('=').next().unwrap();
        assert_eq!(signature.len(), 64);
        // The signing key of the example in AWS's "Create a signed AWS API request".
        let key = signing_key(
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "20150830",
            "us-east-1",
            "iam",
        );
        assert_eq!(
            hex(&key),
            "c4afb1cc5771d871763a393e44b703571b55cc28424d1a5e86da6ed3c154a4b9"
        );
    }

    #[test]
    fn names_and_hosts() {
        assert_eq!(encode("a b/ü+c.csv", true), "a%20b/%C3%BC%2Bc.csv");
        assert_eq!(encode("a/b", false), "a%2Fb");
        assert_eq!(
            host("https://s3.eu-west-1.amazonaws.com/").unwrap(),
            ("https", "s3.eu-west-1.amazonaws.com")
        );
        assert_eq!(host("http://localhost:9000").unwrap(), ("http", "localhost:9000"));
        assert_eq!(host("https://example.com:443").unwrap(), ("https", "example.com"));
        for bad in ["s3.amazonaws.com", "ftp://x", "https://x/path", "https://user@x"] {
            assert!(host(bad).is_err(), "{bad}");
        }
        assert_eq!(
            tag("<Error><Code>AccessDenied</Code><Message>No</Message></Error>", "Code"),
            Some("AccessDenied")
        );
        assert!(!format!("{:?}", bucket()).contains("EXAMPLEKEY"));
    }

    /// A signed request with no body to `path` of the bucket's service.
    fn call(bucket: &Bucket, method: &str, path: &str) -> (u16, String) {
        let (scheme, host) = host(&bucket.endpoint).unwrap();
        let now = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let auth = authorization(bucket, method, (host, path), (EMPTY, &now));
        let request = ureq::http::Request::builder()
            .method(method)
            .uri(format!("{scheme}://{host}{path}"))
            .header("x-amz-date", &now)
            .header("x-amz-content-sha256", EMPTY)
            .header("Authorization", auth)
            .header("Content-Length", "0")
            .body(())
            .unwrap();
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        let mut response = agent.run(request).unwrap();
        let body = response.body_mut().read_to_string().unwrap();
        (response.status().as_u16(), body)
    }

    /// Against a real service, at `PLATYPUS_TEST_S3` (e.g. `http://localhost:59000` of
    /// `docker run -p 59000:9000 rustfs/rustfs`, whose keys are `rustfsadmin`), when it is set.
    #[test]
    fn uploads_to_a_bucket() {
        let Ok(endpoint) = std::env::var("PLATYPUS_TEST_S3") else {
            eprintln!("skipping S3: PLATYPUS_TEST_S3 is not set");
            return;
        };
        let bucket = Bucket {
            endpoint,
            region: "us-east-1".to_owned(),
            bucket: "platypus-test".to_owned(),
            prefix: "exports/2026".to_owned(),
            access_key: "rustfsadmin".to_owned(),
            secret_key: "rustfsadmin".to_owned(),
        };
        let (status, body) = call(&bucket, "PUT", "/platypus-test");
        assert!(status == 200 || body.contains("BucketAlready"), "{status} {body}");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a b.csv");
        std::fs::write(&path, "id,name\n1,ü\n").unwrap();
        put(&path, &bucket).unwrap();
        let (status, body) = call(&bucket, "GET", "/platypus-test/exports/2026/a%20b.csv");
        assert_eq!((status, body.as_str()), (200, "id,name\n1,ü\n"));
        let wrong = Bucket {
            secret_key: "nope".to_owned(),
            ..bucket
        };
        let err = put(&path, &wrong).unwrap_err().to_string();
        assert!(err.contains("403 SignatureDoesNotMatch"), "{err}");
        assert!(!err.contains("nope"), "{err}");
    }
}

//! The upload of an export: to a bucket of an S3-compatible store (`docker/compose.yml`,
//! profile `s3`, or `PLATYPUS_TEST_S3` as `http://host:port`), signed with the keys, and read
//! back; and to a signed URL, on an HTTP server the test starts. The store is skipped when it
//! is not reachable.

use std::fmt::Write as _;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;

use hmac::{Hmac, KeyInit, Mac};
use platypus_core::arrow_array::{Int64Array, RecordBatch, StringArray};
use platypus_core::arrow_schema::{DataType, Field, Schema};
use platypus_transfer::s3::Bucket;
use platypus_transfer::{Compression, ExportError, ExportOptions, Exporter, Format};
use sha2::{Digest, Sha256};

const DEFAULT_ENDPOINT: &str = "http://localhost:49000";
const ACCESS_KEY: &str = "platypus";
const SECRET_KEY: &str = "platypus-secret";
const REGION: &str = "us-east-1";

fn batch() -> RecordBatch {
    let schema = Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("name", DataType::Utf8, true),
    ]);
    RecordBatch::try_new(
        Arc::new(schema),
        vec![
            Arc::new(Int64Array::from(vec![1, 2, 3])),
            Arc::new(StringArray::from(vec![Some("a,b"), None, Some("naive")])),
        ],
    )
    .unwrap()
}

/// Exports the sample batch to `path` with `options`; the error is the export's or the upload's.
fn export(path: &Path, options: &ExportOptions) -> Result<u64, ExportError> {
    let batch = batch();
    let mut exporter = Exporter::create(path, &batch.schema(), options)?;
    exporter.write(&batch)?;
    exporter.finish()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).unwrap();
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// The current UTC time as `YYYYMMDDTHHMMSSZ`.
fn amz_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (days, rest) = (secs / 86400, secs % 86400);
    // Civil date from the day count (Howard Hinnant's algorithm).
    let z = i64::try_from(days).unwrap() + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// `GET /<bucket>/<key>` signed with SigV4 (header form); the status and body.
fn get_object(endpoint: &str, bucket: &str, key: &str) -> (u16, Vec<u8>) {
    let host = endpoint.trim_start_matches("http://");
    let path = format!("/{bucket}/{key}");
    let now = amz_now();
    let payload = hex(&Sha256::digest(b""));
    let signed = "host;x-amz-content-sha256;x-amz-date";
    let headers = format!("host:{host}\nx-amz-content-sha256:{payload}\nx-amz-date:{now}\n");
    let request = format!("GET\n{path}\n\n{headers}\n{signed}\n{payload}");
    let day = &now[..8];
    let scope = format!("{day}/{REGION}/s3/aws4_request");
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{now}\n{scope}\n{}",
        hex(&Sha256::digest(request.as_bytes()))
    );
    let mut k = hmac(format!("AWS4{SECRET_KEY}").as_bytes(), day);
    for part in [REGION, "s3", "aws4_request"] {
        k = hmac(&k, part);
    }
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={ACCESS_KEY}/{scope}, SignedHeaders={signed}, Signature={}",
        hex(&hmac(&k, &to_sign))
    );
    let mut response = ureq::get(format!("{endpoint}{path}"))
        .config()
        .http_status_as_error(false)
        .build()
        .header("x-amz-date", &now)
        .header("x-amz-content-sha256", &payload)
        .header("Authorization", authorization)
        .call()
        .unwrap();
    let status = response.status().as_u16();
    (status, response.body_mut().read_to_vec().unwrap())
}

fn reachable(endpoint: &str) -> bool {
    use std::net::ToSocketAddrs as _;
    let addr = endpoint.trim_start_matches("http://");
    addr.to_socket_addrs().ok().is_some_and(|mut all| {
        all.any(|a| TcpStream::connect_timeout(&a, std::time::Duration::from_secs(2)).is_ok())
    })
}

fn endpoint() -> Option<String> {
    if std::env::var("PLATYPUS_TEST_ONLY").is_ok_and(|only| only != "s3") {
        return None;
    }
    let (endpoint, required) = match std::env::var("PLATYPUS_TEST_S3") {
        Ok(e) => (e, true),
        Err(_) => (DEFAULT_ENDPOINT.to_owned(), false),
    };
    if reachable(&endpoint) {
        return Some(endpoint);
    }
    assert!(!required, "PLATYPUS_TEST_S3 is unreachable: {endpoint}");
    eprintln!("skipping s3 upload: unreachable at {endpoint}");
    None
}

fn bucket(endpoint: &str, prefix: &str) -> Bucket {
    Bucket {
        endpoint: endpoint.to_owned(),
        region: REGION.to_owned(),
        bucket: "platypus".to_owned(),
        prefix: prefix.to_owned(),
        access_key: ACCESS_KEY.to_owned(),
        secret_key: SECRET_KEY.to_owned(),
    }
}

#[test]
fn uploads_an_export_to_a_bucket_and_reads_it_back() {
    let Some(endpoint) = endpoint() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let name = format!("conf-{}.csv", std::process::id());
    let path = dir.path().join(&name);
    let options = ExportOptions {
        format: Format::Csv,
        bucket: Some(bucket(&endpoint, "/exports/nested/")),
        ..ExportOptions::default()
    };
    assert_eq!(export(&path, &options).unwrap(), 3);

    let (status, body) = get_object(&endpoint, "platypus", &format!("exports/nested/{name}"));
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    assert_eq!(body, std::fs::read(&path).unwrap());
    let text = String::from_utf8(body).unwrap();
    assert!(text.starts_with("id,name\n1,\"a,b\"\n2,\n3,naive"), "{text}");

    // Without a prefix the object sits at the root of the bucket; a gzip export is uploaded
    // compressed, under the compressed name.
    let gz = dir.path().join(format!("{name}.gz"));
    let options = ExportOptions {
        compression: Compression::Gzip,
        bucket: Some(bucket(&endpoint, "")),
        ..ExportOptions::default()
    };
    export(&gz, &options).unwrap();
    let (status, body) = get_object(&endpoint, "platypus", &format!("{name}.gz"));
    assert_eq!(status, 200);
    assert_eq!(body, std::fs::read(&gz).unwrap());
    assert_eq!(&body[..2], &[0x1f, 0x8b]);
}

#[test]
fn refuses_wrong_keys_and_a_missing_bucket() {
    let Some(endpoint) = endpoint() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("refused.csv");

    let mut wrong = bucket(&endpoint, "");
    wrong.secret_key = "not-the-secret".to_owned();
    let options = ExportOptions {
        bucket: Some(wrong),
        ..ExportOptions::default()
    };
    let err = export(&path, &options).unwrap_err().to_string();
    assert!(err.contains("upload failed"), "{err}");
    assert!(err.contains("403"), "{err}");
    assert!(!err.contains("not-the-secret"), "{err}");
    // Only the upload failed: the file stays on disk.
    assert!(path.exists());

    let mut missing = bucket(&endpoint, "");
    missing.bucket = "no-such-bucket".to_owned();
    let options = ExportOptions {
        bucket: Some(missing),
        ..ExportOptions::default()
    };
    let err = export(&path, &options).unwrap_err().to_string();
    assert!(err.contains("upload failed"), "{err}");
}

/// A one-request HTTP server answering `status`; returns its URL and the request it got.
fn signed_url_server(status: u16) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://{}/bucket/out.csv?X-Amz-Signature=secret-signature",
        listener.local_addr().unwrap()
    );
    let handle = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        let mut head = String::new();
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = v.trim().parse().unwrap();
            }
            head.push_str(&line);
            if line == "\r\n" {
                break;
            }
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        let reply =
            format!("HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        reader.get_mut().write_all(reply.as_bytes()).unwrap();
        head + &String::from_utf8(body).unwrap()
    });
    (url, handle)
}

#[test]
fn uploads_an_export_to_a_signed_url() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.csv");
    let (url, request) = signed_url_server(200);
    let options = ExportOptions {
        upload_url: Some(url),
        ..ExportOptions::default()
    };
    assert_eq!(export(&path, &options).unwrap(), 3);
    let request = request.join().unwrap();
    assert!(
        request.starts_with("PUT /bucket/out.csv?X-Amz-Signature=secret-signature"),
        "{request}"
    );
    let file = std::fs::read_to_string(&path).unwrap();
    assert!(request.ends_with(&file), "{request}");
    assert!(
        request
            .to_ascii_lowercase()
            .contains(&format!("content-length: {}", file.len())),
        "{request}"
    );

    // A refused upload says so, never with the signature, and the export stays on disk.
    let (url, request) = signed_url_server(403);
    let refused = dir.path().join("refused.csv");
    let options = ExportOptions {
        upload_url: Some(url),
        ..ExportOptions::default()
    };
    let err = export(&refused, &options).unwrap_err().to_string();
    request.join().unwrap();
    assert!(err.contains("403"), "{err}");
    assert!(!err.contains("secret-signature"), "{err}");
    assert!(refused.exists());

    // Nobody listening: a connection error, again without the signature.
    let options = ExportOptions {
        upload_url: Some("http://127.0.0.1:1/bucket/out.csv?X-Amz-Signature=secret-signature".to_owned()),
        ..ExportOptions::default()
    };
    let err = export(&dir.path().join("dead.csv"), &options)
        .unwrap_err()
        .to_string();
    assert!(err.contains("upload failed"), "{err}");
    assert!(!err.contains("secret-signature"), "{err}");
}

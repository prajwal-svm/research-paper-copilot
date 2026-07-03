//! S3-compatible blob access with hand-signed AWS Signature V4 over ureq.
//!
//! Spike decision (task 1.1): no S3 crate — the app already ships ureq, and
//! SigV4 is ~150 lines of deterministic hashing. This keeps the dependency
//! tree flat and every request loggable. Works against self-hosted MinIO
//! (path-style addressing, any region string) and Cloudflare R2.
//!
//! Conditional writes: `If-None-Match: *` create-only puts are supported by
//! recent MinIO and R2 — used for the manifest generation swap, with
//! list-and-verify as the portable fallback (engine-side).

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone)]
pub struct S3Config {
    /// e.g. "http://localhost:9000" (MinIO) or "https://<account>.r2.cloudflarestorage.com"
    pub endpoint: String,
    pub bucket: String,
    pub region: String, // "us-east-1" works for MinIO; "auto" for R2
    pub access_key: String,
    pub secret_key: String,
}

#[derive(Debug, thiserror::Error)]
pub enum S3Error {
    #[error("s3: HTTP {status}: {body}")]
    Status { status: u16, body: String },
    #[error("s3: {0}")]
    Network(String),
    /// Conditional put lost the race (object already exists).
    #[error("s3: precondition failed (object exists)")]
    PreconditionFailed,
}

fn now_amz() -> (String, String) {
    // (YYYYMMDDTHHMMSSZ, YYYYMMDD) without pulling a formatting dependency.
    let now = time::OffsetDateTime::now_utc();
    let date = format!("{:04}{:02}{:02}", now.year(), now.month() as u8, now.day());
    let stamp = format!(
        "{date}T{:02}{:02}{:02}Z",
        now.hour(),
        now.minute(),
        now.second()
    );
    (stamp, date)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// RFC 3986 encoding for path segments (S3 canonical URIs keep `/`).
fn uri_encode(path: &str, keep_slash: bool) -> String {
    path.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b'/' if keep_slash => "/".to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

pub struct S3Client {
    config: S3Config,
}

impl S3Client {
    pub fn new(config: S3Config) -> S3Client {
        S3Client { config }
    }

    pub fn host(&self) -> String {
        self.config
            .endpoint
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .to_string()
    }

    /// Sign and execute one request. `query` must be pre-sorted by key.
    fn request(
        &self,
        method: &str,
        key: &str,
        query: &[(String, String)],
        body: &[u8],
        extra_headers: &[(&str, &str)],
    ) -> Result<(u16, Vec<u8>), S3Error> {
        let (amz_date, date) = now_amz();
        let payload_hash = hex(&Sha256::digest(body));
        let host = self.host();
        let canonical_uri = format!(
            "/{}/{}",
            uri_encode(&self.config.bucket, false),
            uri_encode(key, true)
        );
        let canonical_query: String = query
            .iter()
            .map(|(k, v)| format!("{}={}", uri_encode(k, false), uri_encode(v, false)))
            .collect::<Vec<_>>()
            .join("&");

        // Canonical headers: host + x-amz-* (sorted), all lowercase.
        let mut headers: Vec<(String, String)> = vec![
            ("host".to_string(), host.clone()),
            ("x-amz-content-sha256".to_string(), payload_hash.clone()),
            ("x-amz-date".to_string(), amz_date.clone()),
        ];
        for (name, value) in extra_headers {
            if name.starts_with("x-amz-") || *name == "if-none-match" || *name == "if-match" {
                headers.push((name.to_string(), value.to_string()));
            }
        }
        headers.sort();
        let canonical_headers: String = headers.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
        let signed_headers: String = headers
            .iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>()
            .join(";");

        let canonical_request = format!(
            "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
        );
        let scope = format!("{date}/{}/s3/aws4_request", self.config.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
            hex(&Sha256::digest(canonical_request.as_bytes()))
        );
        let k_date = hmac(
            format!("AWS4{}", self.config.secret_key).as_bytes(),
            date.as_bytes(),
        );
        let k_region = hmac(&k_date, self.config.region.as_bytes());
        let k_service = hmac(&k_region, b"s3");
        let k_signing = hmac(&k_service, b"aws4_request");
        let signature = hex(&hmac(&k_signing, string_to_sign.as_bytes()));
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.config.access_key
        );

        let mut url = format!(
            "{}{canonical_uri}",
            self.config.endpoint.trim_end_matches('/')
        );
        if !canonical_query.is_empty() {
            url.push('?');
            url.push_str(&canonical_query);
        }
        let mut request = ureq::request(method, &url)
            .timeout(std::time::Duration::from_secs(60))
            .set("x-amz-date", &amz_date)
            .set("x-amz-content-sha256", &payload_hash)
            .set("Authorization", &authorization);
        for (name, value) in extra_headers {
            request = request.set(name, value);
        }
        let response = if body.is_empty() && method != "PUT" {
            request.call()
        } else {
            request.send_bytes(body)
        };
        match response {
            Ok(response) => {
                let status = response.status();
                let mut bytes = Vec::new();
                std::io::Read::read_to_end(&mut response.into_reader(), &mut bytes)
                    .map_err(|e| S3Error::Network(e.to_string()))?;
                Ok((status, bytes))
            }
            Err(ureq::Error::Status(status, response)) => {
                if status == 412 || status == 409 {
                    return Err(S3Error::PreconditionFailed);
                }
                let body = response.into_string().unwrap_or_default();
                Err(S3Error::Status {
                    status,
                    body: body.chars().take(300).collect(),
                })
            }
            Err(e) => Err(S3Error::Network(e.to_string())),
        }
    }

    /// Create the bucket if it doesn't exist (idempotent-ish; 409 = exists).
    pub fn ensure_bucket(&self) -> Result<(), S3Error> {
        match self.request("PUT", "", &[], &[], &[]) {
            Ok(_) => Ok(()),
            Err(S3Error::PreconditionFailed) => Ok(()), // 409 BucketAlreadyOwnedByYou
            Err(S3Error::Status { status: 409, .. }) => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn put(&self, key: &str, body: &[u8]) -> Result<(), S3Error> {
        self.request("PUT", key, &[], body, &[]).map(|_| ())
    }

    /// Create-only put (`If-None-Match: *`) — the manifest-swap primitive on
    /// MinIO/R2. [`S3Error::PreconditionFailed`] means another writer won.
    pub fn put_if_absent(&self, key: &str, body: &[u8]) -> Result<(), S3Error> {
        self.request("PUT", key, &[], body, &[("if-none-match", "*")])
            .map(|_| ())
    }

    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>, S3Error> {
        match self.request("GET", key, &[], &[], &[]) {
            Ok((_, bytes)) => Ok(Some(bytes)),
            Err(S3Error::Status { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn delete(&self, key: &str) -> Result<(), S3Error> {
        match self.request("DELETE", key, &[], &[], &[]) {
            Ok(_) => Ok(()),
            Err(S3Error::Status { status: 404, .. }) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// List keys under a prefix (ListObjectsV2, paginated).
    pub fn list(&self, prefix: &str) -> Result<Vec<String>, S3Error> {
        let mut keys = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut query: Vec<(String, String)> = vec![
                ("list-type".into(), "2".into()),
                ("prefix".into(), prefix.into()),
            ];
            if let Some(t) = &token {
                query.push(("continuation-token".into(), t.clone()));
            }
            query.sort();
            let (_, body) = self.request("GET", "", &query, &[], &[])?;
            let xml = String::from_utf8_lossy(&body);
            for part in xml.split("<Key>").skip(1) {
                if let Some(end) = part.find("</Key>") {
                    keys.push(part[..end].to_string());
                }
            }
            token = xml
                .split("<NextContinuationToken>")
                .nth(1)
                .and_then(|p| p.split("</NextContinuationToken>").next())
                .map(|s| s.to_string());
            let truncated = xml.contains("<IsTruncated>true</IsTruncated>");
            if !truncated || token.is_none() {
                break;
            }
        }
        Ok(keys)
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn spike_client() -> S3Client {
        S3Client::new(S3Config {
            endpoint: std::env::var("RPC_S3_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:19000".into()),
            bucket: "rpc-sync-test".into(),
            region: "us-east-1".into(),
            access_key: std::env::var("RPC_S3_ACCESS").unwrap_or_else(|_| "spikeuser".into()),
            secret_key: std::env::var("RPC_S3_SECRET").unwrap_or_else(|_| "spikepass123".into()),
        })
    }

    /// Live gate against a real S3 endpoint (MinIO container / Coolify / R2):
    ///   cargo test -p copilot-core --lib sync::s3 -- --ignored
    #[test]
    #[ignore = "needs a reachable S3 endpoint (see spike_client env vars)"]
    fn sigv4_put_get_list_delete_and_conditional_put() {
        let client = spike_client();
        client.ensure_bucket().unwrap();

        let key = format!("spike/{}.bin", uuid::Uuid::new_v4());
        client.put(&key, b"hello sigv4").unwrap();
        assert_eq!(client.get(&key).unwrap().unwrap(), b"hello sigv4");

        let listed = client.list("spike/").unwrap();
        assert!(listed.contains(&key), "{listed:?}");

        // Conditional create-only: second writer must lose.
        let manifest_key = format!("spike/manifest-{}.json", uuid::Uuid::new_v4());
        client.put_if_absent(&manifest_key, b"{\"gen\":1}").unwrap();
        let race = client.put_if_absent(&manifest_key, b"{\"gen\":1-loser}");
        assert!(
            matches!(race, Err(S3Error::PreconditionFailed)),
            "conditional put must fail when the object exists: {race:?}"
        );

        assert!(client.get("spike/never-existed").unwrap().is_none());
        client.delete(&key).unwrap();
        client.delete(&manifest_key).unwrap();
        assert!(client.get(&key).unwrap().is_none());
    }
}

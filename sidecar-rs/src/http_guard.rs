use std::collections::HashMap;
use crate::local_origins::LOCAL_ORIGINS;

fn hostname(host: &str) -> &str {
    if host.starts_with('[') {
        host.find(']').map(|i| &host[..=i]).unwrap_or(host)
    } else {
        host.split(':').next().unwrap_or(host)
    }
}

fn is_loopback(host: &str) -> bool {
    matches!(hostname(host), "127.0.0.1" | "localhost" | "[::1]")
}

pub fn allowed(headers: &HashMap<String, String>, remote_secret: &str) -> bool {
    if headers.get("x-panorama-remote").map(String::as_str) == Some(remote_secret) {
        return true;
    }
    if let Some(host) = headers.get("host") {
        if !is_loopback(host) {
            return false;
        }
    }
    if let Some(origin) = headers.get("origin") {
        return LOCAL_ORIGINS.contains(&origin.as_str());
    }
    if let Some(site) = headers.get("sec-fetch-site") {
        if site != "none" && site != "same-origin" {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    const SECRET: &str = "abc123";

    #[test]
    fn curl_loopback_no_origin_allowed() {
        let h = headers(&[("host", "127.0.0.1:9777")]);
        assert!(allowed(&h, SECRET));
    }

    #[test]
    fn localhost_port_allowed() {
        let h = headers(&[("host", "localhost:9877")]);
        assert!(allowed(&h, SECRET));
    }

    #[test]
    fn tauri_localhost_origin_with_cross_site_allowed() {
        let h = headers(&[
            ("host", "127.0.0.1:9777"),
            ("origin", "http://tauri.localhost"),
            ("sec-fetch-site", "cross-site"),
        ]);
        assert!(allowed(&h, SECRET));
    }

    #[test]
    fn tauri_scheme_origin_with_cross_site_allowed() {
        let h = headers(&[
            ("host", "127.0.0.1:9777"),
            ("origin", "tauri://localhost"),
            ("sec-fetch-site", "cross-site"),
        ]);
        assert!(allowed(&h, SECRET));
    }

    #[test]
    fn evil_origin_rejected() {
        let h = headers(&[
            ("host", "127.0.0.1:9777"),
            ("origin", "https://evil.example"),
        ]);
        assert!(!allowed(&h, SECRET));
    }

    #[test]
    fn null_origin_rejected() {
        let h = headers(&[("host", "127.0.0.1:9777"), ("origin", "null")]);
        assert!(!allowed(&h, SECRET));
    }

    #[test]
    fn no_origin_cross_site_fetch_rejected() {
        let h = headers(&[
            ("host", "127.0.0.1:9777"),
            ("sec-fetch-site", "cross-site"),
        ]);
        assert!(!allowed(&h, SECRET));
    }

    #[test]
    fn dns_rebind_host_rejected() {
        let h = headers(&[("host", "evil.example:9777")]);
        assert!(!allowed(&h, SECRET));
    }

    #[test]
    fn evil_host_correct_secret_allowed() {
        let h = headers(&[
            ("host", "evil.example:9777"),
            ("x-panorama-remote", SECRET),
        ]);
        assert!(allowed(&h, SECRET));
    }

    #[test]
    fn evil_host_wrong_secret_rejected() {
        let h = headers(&[
            ("host", "evil.example:9777"),
            ("x-panorama-remote", "wrongsecret"),
        ]);
        assert!(!allowed(&h, SECRET));
    }

    #[test]
    fn missing_host_allowed() {
        let h = headers(&[]);
        assert!(allowed(&h, SECRET));
    }
}

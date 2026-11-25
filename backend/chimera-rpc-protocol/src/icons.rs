//! Transport-neutral icon parsing and byte response metadata, from ref.
//! Temporary home until the shared RPC crate owns the icon entry.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use snafu::Snafu;

use crate::values::TrayIcon;

/// Input to the icon URL transport; versions refresh mutable URLs only.
#[derive(Debug, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IconRequest {
    Cached {
        url: String,
    },
    Tray {
        mode: TrayIcon,
        version: Option<u32>,
    },
    Widget {
        mode: TrayIcon,
        version: Option<u32>,
    },
}

/// Browser cache policy, independent of the underlying seven-day file cache.
#[derive(Debug, Clone, Copy)]
pub enum IconCachePolicy {
    Remote,
    Mutable,
}

impl IconCachePolicy {
    /// Authenticated resources are private; mutable icons must revalidate.
    pub fn header(self) -> &'static str {
        match self {
            Self::Remote => "private, max-age=3600",
            Self::Mutable => "private, no-cache",
        }
    }
}

/// Validated response metadata and untrusted image bytes.
pub struct IconBody {
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
    pub cache_policy: IconCachePolicy,
}

/// Required on both successful and failed responses, including cached content.
pub const SECURITY_HEADERS: [(&str, &str); 2] = [
    ("x-content-type-options", "nosniff"),
    ("content-security-policy", "sandbox; default-src 'none'"),
];

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("invalid icon path"))]
    ParseIconPath,
    #[snafu(display("invalid icon query"))]
    ParseIconQuery,
    #[snafu(display("icon URL must be absolute HTTP or HTTPS"))]
    ParseRemoteUrl,
    #[snafu(display("icon content type is not an allowed image type"))]
    CheckImageContentType,
}

/// Parses the single path/query grammar used by HTTP and desktop protocols.
///
/// # Errors
/// Rejects ambiguous fields, invalid modes/versions and non-HTTP remote URLs.
/// Error messages never contain the supplied URL or query.
pub fn parse(path: &str, query: Option<&str>) -> Result<IconRequest, Error> {
    let mut fields = unique_query(query.unwrap_or(""))?;
    let request = match path {
        "/cached" => {
            let url = required(&mut fields, "url")?;
            let parsed = url::Url::parse(&url).map_err(|_| Error::ParseRemoteUrl)?;
            if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
                return Err(Error::ParseRemoteUrl);
            }
            IconRequest::Cached { url }
        }
        "/tray" | "/widget" => {
            let mode = match required(&mut fields, "mode")?.as_str() {
                "normal" => TrayIcon::Normal,
                "tun" => TrayIcon::Tun,
                "system_proxy" => TrayIcon::SystemProxy,
                _ => return Err(Error::ParseIconQuery),
            };
            let version = fields
                .remove("version")
                .map(|value| {
                    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                        return Err(Error::ParseIconQuery);
                    }
                    value.parse::<u32>().map_err(|_| Error::ParseIconQuery)
                })
                .transpose()?;
            if path == "/tray" {
                IconRequest::Tray { mode, version }
            } else {
                IconRequest::Widget { mode, version }
            }
        }
        _ => return Err(Error::ParseIconPath),
    };
    if !fields.is_empty() {
        return Err(Error::ParseIconQuery);
    }
    Ok(request)
}

fn unique_query(query: &str) -> Result<BTreeMap<String, String>, Error> {
    // form_urlencoded deliberately tolerates malformed escapes. The entry does not.
    for (index, byte) in query.bytes().enumerate() {
        if byte == b'%'
            && !query
                .as_bytes()
                .get(index + 1..index + 3)
                .is_some_and(|digits| digits.iter().all(u8::is_ascii_hexdigit))
        {
            return Err(Error::ParseIconQuery);
        }
    }
    let mut fields = BTreeMap::new();
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if value.is_empty()
            || fields
                .insert(key.into_owned(), value.into_owned())
                .is_some()
        {
            return Err(Error::ParseIconQuery);
        }
    }
    Ok(fields)
}

fn required(fields: &mut BTreeMap<String, String>, key: &str) -> Result<String, Error> {
    fields.remove(key).ok_or(Error::ParseIconQuery)
}

pub fn allowed_content_type(mime: &str) -> Result<&'static str, Error> {
    let essence = mime.split(';').next().unwrap_or("").trim();
    [
        "image/png",
        "image/apng",
        "image/jpeg",
        "image/gif",
        "image/webp",
        "image/avif",
        "image/bmp",
        "image/x-icon",
        "image/vnd.microsoft.icon",
        "image/svg+xml",
    ]
    .into_iter()
    .find(|allowed| essence.eq_ignore_ascii_case(allowed))
    .ok_or(Error::CheckImageContentType)
}

#[cfg(test)]
mod tests {
    use super::{IconRequest, allowed_content_type, parse};

    #[test]
    fn icon_url_parser_matches_frontend_fixtures() {
        let request = parse("/cached", Some("url=https%3A%2F%2Ficons.example%2F%E7%8C%AB+icon.svg%3Fa%3D1%26token%3Da%2Bb%26next%3D%252F")).unwrap();
        let IconRequest::Cached { url } = request else {
            panic!("cached request")
        };
        assert_eq!(
            url,
            "https://icons.example/猫 icon.svg?a=1&token=a+b&next=%2F"
        );
        for (path, query, expected) in [
            (
                "/tray",
                "mode=normal&version=0",
                serde_json::json!({"kind":"tray","mode":"normal","version":0}),
            ),
            (
                "/widget",
                "mode=system_proxy&version=4294967295",
                serde_json::json!({"kind":"widget","mode":"system_proxy","version":4294967295u32}),
            ),
            (
                "/tray",
                "mode=tun",
                serde_json::json!({"kind":"tray","mode":"tun","version":null}),
            ),
        ] {
            assert_eq!(
                serde_json::to_value(parse(path, Some(query)).unwrap()).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn icon_parser_rejects_ambiguous_or_invalid_requests() {
        for (path, query) in [
            ("/other", "mode=normal"),
            ("/tray/normal", ""),
            ("/tray", ""),
            ("/tray", "mode="),
            ("/tray", "mode=unknown"),
            ("/tray", "mode=tun&mode=normal"),
            ("/tray", "mode=tun&%6Dode=normal"),
            ("/tray", "mode=tun&url=https://secret.example"),
            ("/tray", "mode=tun&version=-1"),
            ("/tray", "mode=tun&version=%2B1"),
            ("/tray", "mode=tun&version=4294967296"),
            ("/tray", "mode=tun&version="),
            ("/cached", "url=file%3A%2F%2F%2Fsecret"),
            ("/cached", "url=%2Frelative"),
            ("/cached", "url=https%3A%2F%2F"),
            ("/cached", "url=https://secret.example&x=1"),
            (
                "/cached",
                "url=https://secret.example&url=https://other.example",
            ),
            ("/cached", "url=%"),
            ("/cached", "url=%zz"),
        ] {
            let error = parse(path, Some(query)).unwrap_err();
            assert!(!error.to_string().contains("secret"), "{path} {query}");
        }
    }

    #[test]
    fn icon_entry_rejects_non_image_content_types() {
        for mime in [
            "",
            "text/html",
            "application/xhtml+xml",
            "image/*",
            "image/unknown",
            "text/plain",
        ] {
            assert!(allowed_content_type(mime).is_err());
        }
        for mime in [
            "image/png",
            "image/apng",
            "image/jpeg",
            "image/gif",
            "image/webp",
            "image/avif",
            "image/bmp",
            "image/x-icon",
            "image/vnd.microsoft.icon",
            "image/svg+xml",
        ] {
            assert_eq!(allowed_content_type(mime).unwrap(), mime);
        }
        assert_eq!(
            allowed_content_type("IMAGE/SVG+XML; charset=utf-8").unwrap(),
            "image/svg+xml"
        );
    }
}

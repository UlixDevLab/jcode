//! Literal loopback origin parsing, resolution pinning, and DNS validation.
//!
//! The exact-origin parser accepts only `http://127.0.0.1:<port>`,
//! `http://[::1]:<port>`, or `http://localhost:<port>` (the latter is
//! resolved via a caller-supplied lookup so the result can be tested
//! deterministically). Every other scheme, host, port, and userinfo is
//! rejected before the broker is even spawned.

use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use thiserror::Error;

/// One of the two literal loopback IP addresses.
///
/// `LoopbackAddress` carries the resolved address so the broker can pin the
/// exact destination for its lifetime, regardless of how the host was
/// specified at `open` time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LoopbackAddress {
    /// `127.0.0.1`.
    V4(Ipv4Addr),
    /// `::1`.
    V6(Ipv6Addr),
}

impl LoopbackAddress {
    /// Standard IPv4 loopback.
    pub const IPV4_LOOPBACK: Self = Self::V4(Ipv4Addr::new(127, 0, 0, 1));

    /// Standard IPv6 loopback.
    pub const IPV6_LOOPBACK: Self = Self::V6(Ipv6Addr::LOCALHOST);
}

/// A pinned loopback origin (scheme `http`, resolved loopback address,
/// port).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocalOrigin {
    /// Resolved loopback address.
    pub address: LoopbackAddress,
    /// Non-zero TCP port.
    pub port: u16,
}

/// A parsed-but-not-yet-pinned loopback URL.
///
/// Path, query, and fragment are intentionally dropped during parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedOrigin {
    /// Host literal kind.
    pub host: HostKind,
    /// Explicit TCP port (never zero).
    pub port: u16,
}

/// Host literal classification used by [`parse_loopback`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKind {
    /// Literal `127.0.0.1`.
    LiteralV4,
    /// Literal `[::1]`.
    LiteralV6,
    /// Hostname `localhost` (must be resolved via DNS).
    Localhost,
}

/// Failure modes for loopback URL parsing and DNS pinning.
#[derive(Debug, Error)]
pub enum OriginError {
    /// The string was not a syntactically valid URL.
    #[error("URL parse failed: {0}")]
    Url(#[from] url::ParseError),

    /// The URL scheme is not `http`.
    #[error("scheme must be http, got {0:?}")]
    SchemeNotHttp(String),

    /// The URL carried a userinfo component.
    #[error("URL must not contain a username or password")]
    UserInfoPresent,

    /// The URL did not carry an explicit port.
    #[error("URL must include an explicit port (default ports are not accepted)")]
    MissingPort,

    /// The URL carried port 0.
    #[error("port must be non-zero")]
    ZeroPort,

    /// The host was not a literal loopback (`127.0.0.1`, `[::1]`, or
    /// `localhost`).
    #[error("host {0:?} is not a literal loopback (must be 127.0.0.1, [::1], or localhost)")]
    HostNotLiteralLoopback(String),

    /// DNS resolution of `localhost` returned a non-loopback address.
    #[error("DNS resolution for {host:?} returned non-loopback address {addr}")]
    NonLoopbackDns {
        /// Hostname that was looked up.
        host: String,
        /// Address the lookup returned.
        addr: IpAddr,
    },

    /// DNS resolution failed entirely (no addresses or lookup error).
    #[error("DNS resolution failed for {host:?}")]
    DnsFailure {
        /// Hostname that was looked up.
        host: String,
    },
}

/// Parse a literal loopback URL.
///
/// The parser accepts exactly three host forms (`127.0.0.1`, `[::1]`, or
/// `localhost`) and rejects everything else: non-`http` schemes, userinfo,
/// missing or zero port, public / nonliteral hosts, and URL syntax
/// errors. Path, query, and fragment are silently dropped — the
/// [`ParsedOrigin`] never retains them.
pub fn parse_loopback(input: &str) -> Result<ParsedOrigin, OriginError> {
    let url = url::Url::parse(input)?;

    if url.scheme() != "http" {
        return Err(OriginError::SchemeNotHttp(url.scheme().to_string()));
    }

    if !url.username().is_empty() || url.password().is_some() {
        return Err(OriginError::UserInfoPresent);
    }

    // `url::Url` follows the WHATWG parser. It deliberately canonicalizes
    // ambiguous IPv4 spellings (`127.1`, integer and hexadecimal forms) and
    // removes an explicit default port. Neither behavior is acceptable at a
    // security boundary that promises one exact literal origin, so classify
    // the raw authority before trusting the normalized URL host.
    let authority = input
        .strip_prefix("http://")
        .ok_or_else(|| OriginError::SchemeNotHttp(url.scheme().to_string()))?
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let (host_kind, raw_port) = if let Some(port) = authority.strip_prefix("127.0.0.1:") {
        (HostKind::LiteralV4, port)
    } else if let Some(port) = authority.strip_prefix("[::1]:") {
        (HostKind::LiteralV6, port)
    } else if let Some(port) = authority.strip_prefix("localhost:") {
        (HostKind::Localhost, port)
    } else {
        return Err(OriginError::HostNotLiteralLoopback(
            "<redacted>".to_string(),
        ));
    };
    if raw_port.is_empty() {
        return Err(OriginError::MissingPort);
    }
    let port = raw_port
        .parse::<u16>()
        .map_err(|_| OriginError::MissingPort)?;
    if port == 0 {
        return Err(OriginError::ZeroPort);
    }
    if raw_port != port.to_string() {
        return Err(OriginError::MissingPort);
    }

    let normalized_matches = match (host_kind, url.host()) {
        (HostKind::Localhost, Some(url::Host::Domain("localhost"))) => true,
        (HostKind::LiteralV4, Some(url::Host::Ipv4(addr))) => addr == Ipv4Addr::new(127, 0, 0, 1),
        (HostKind::LiteralV6, Some(url::Host::Ipv6(addr))) => addr == Ipv6Addr::LOCALHOST,
        _ => false,
    };
    if !normalized_matches {
        return Err(OriginError::HostNotLiteralLoopback(
            "<redacted>".to_string(),
        ));
    }

    Ok(ParsedOrigin {
        host: host_kind,
        port,
    })
}

/// Pin a [`ParsedOrigin`] to a concrete [`LocalOrigin`] using a
/// caller-supplied DNS lookup.
///
/// Literal hosts skip the lookup entirely. The `localhost` hostname is
/// resolved via the supplied `lookup` callback; the result must be a
/// loopback address, otherwise [`OriginError::NonLoopbackDns`] is
/// returned. The `lookup` callback may surface its own errors as
/// [`OriginError::DnsFailure`].
pub fn pin_with_lookup(
    parsed: &ParsedOrigin,
    mut lookup: impl FnMut(&str) -> Result<IpAddr, OriginError>,
) -> Result<LocalOrigin, OriginError> {
    match parsed.host {
        HostKind::LiteralV4 => Ok(LocalOrigin {
            address: LoopbackAddress::IPV4_LOOPBACK,
            port: parsed.port,
        }),
        HostKind::LiteralV6 => Ok(LocalOrigin {
            address: LoopbackAddress::IPV6_LOOPBACK,
            port: parsed.port,
        }),
        HostKind::Localhost => {
            let ip = lookup("localhost")?;
            if !ip.is_loopback() {
                return Err(OriginError::NonLoopbackDns {
                    host: "localhost".to_string(),
                    addr: ip,
                });
            }
            let address = match ip {
                IpAddr::V4(v4) => LoopbackAddress::V4(v4),
                IpAddr::V6(v6) => LoopbackAddress::V6(v6),
            };
            Ok(LocalOrigin {
                address,
                port: parsed.port,
            })
        }
    }
}

//! RED gate tests for literal loopback origin parsing.
//!
//! These tests assert the exact-origin parser:
//!   * accepts `http://127.0.0.1:<port>`, `http://[::1]:<port>`, and
//!     `http://localhost:<port>`,
//!   * rejects public hosts, missing/zero port, https, file/data/javascript
//!     schemes, userinfo, nonliteral/wildcard hosts, and non-loopback DNS
//!     resolution.

use jcode_browser_broker::origin::{
    HostKind, LocalOrigin, LoopbackAddress, OriginError, ParsedOrigin, parse_loopback,
    pin_with_lookup,
};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

// ---- accept ---------------------------------------------------------------

#[test]
fn parse_accepts_ipv4_loopback() {
    let parsed = parse_loopback("http://127.0.0.1:8080/").expect("must accept");
    assert_eq!(parsed.port, 8080);
    assert!(matches!(parsed.host, HostKind::LiteralV4));
}

#[test]
fn parse_accepts_ipv6_loopback() {
    let parsed = parse_loopback("http://[::1]:3000/").expect("must accept");
    assert_eq!(parsed.port, 3000);
    assert!(matches!(parsed.host, HostKind::LiteralV6));
}

#[test]
fn parse_accepts_localhost_hostname() {
    let parsed = parse_loopback("http://localhost:7000").expect("must accept");
    assert_eq!(parsed.port, 7000);
    assert!(matches!(parsed.host, HostKind::Localhost));
}

#[test]
fn parse_accepts_explicit_default_port() {
    let parsed = parse_loopback("http://127.0.0.1:80/").expect("explicit port must survive");
    assert_eq!(parsed.port, 80);
    assert!(matches!(parsed.host, HostKind::LiteralV4));
}

#[test]
fn parse_drops_path_query_and_fragment() {
    // The exact-origin parser must not retain path / query / fragment.
    let parsed =
        parse_loopback("http://127.0.0.1:8080/some/path?q=1&r=2#frag").expect("must accept");
    assert_eq!(parsed.port, 8080);
    assert!(matches!(parsed.host, HostKind::LiteralV4));
}

// ---- reject ---------------------------------------------------------------

#[test]
fn parse_rejects_public_ipv4() {
    assert!(parse_loopback("http://8.8.8.8:80/").is_err());
}

#[test]
fn parse_rejects_public_ipv6() {
    assert!(parse_loopback("http://[2001:db8::1]:80/").is_err());
}

#[test]
fn parse_rejects_public_hostname() {
    assert!(parse_loopback("http://example.com:80/").is_err());
}

#[test]
fn parse_rejects_nonstandard_loopback_ipv4() {
    // 127.0.0.2 is in the 127/8 block but is not the literal loopback.
    assert!(parse_loopback("http://127.0.0.2:80/").is_err());
}

#[test]
fn parse_rejects_whatwg_ipv4_aliases() {
    for url in [
        "http://127.1:8080/",
        "http://2130706433:8080/",
        "http://0x7f000001:8080/",
        "http://017700000001:8080/",
    ] {
        assert!(
            parse_loopback(url).is_err(),
            "accepted ambiguous host {url}"
        );
    }
}

#[test]
fn parse_rejects_non_exact_localhost_spellings() {
    for url in [
        "http://LOCALHOST:8080/",
        "http://localhost.:8080/",
        "http://localhost:08080/",
    ] {
        assert!(
            parse_loopback(url).is_err(),
            "accepted non-exact host {url}"
        );
    }
}

#[test]
fn parse_rejects_nonstandard_loopback_ipv6() {
    assert!(parse_loopback("http://[::2]:80/").is_err());
}

#[test]
fn parse_rejects_zero_port() {
    assert!(parse_loopback("http://127.0.0.1:0/").is_err());
}

#[test]
fn parse_rejects_missing_port_for_default_scheme() {
    // http://127.0.0.1/ has no explicit port (80 is the default).
    // The parser must require an explicit port.
    assert!(parse_loopback("http://127.0.0.1/").is_err());
}

#[test]
fn parse_rejects_https() {
    assert!(parse_loopback("https://127.0.0.1:443/").is_err());
}

#[test]
fn parse_rejects_file_scheme() {
    assert!(parse_loopback("file:///etc/passwd").is_err());
}

#[test]
fn parse_rejects_data_scheme() {
    assert!(parse_loopback("data:text/html,<h1>x</h1>").is_err());
}

#[test]
fn parse_rejects_javascript_scheme() {
    assert!(parse_loopback("javascript:alert(1)").is_err());
}

#[test]
fn parse_rejects_userinfo_username_only() {
    assert!(parse_loopback("http://user@127.0.0.1:80/").is_err());
}

#[test]
fn parse_rejects_userinfo_username_and_password() {
    assert!(parse_loopback("http://user:pass@127.0.0.1:80/").is_err());
}

#[test]
fn parse_rejects_non_url_garbage() {
    assert!(parse_loopback("not-a-url").is_err());
}

#[test]
fn parse_rejects_wildcard_subdomain() {
    assert!(parse_loopback("http://*.example.com:80/").is_err());
}

// ---- pin_with_lookup -------------------------------------------------------

#[test]
fn pin_localhost_resolves_to_ipv4_loopback() {
    let parsed = ParsedOrigin {
        host: HostKind::Localhost,
        port: 8080,
    };
    let origin = pin_with_lookup(&parsed, |_name| Ok(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))))
        .expect("must accept");
    assert_eq!(origin.port, 8080);
    assert!(matches!(origin.address, LoopbackAddress::V4(_)));
}

#[test]
fn pin_localhost_resolves_to_ipv6_loopback() {
    let parsed = ParsedOrigin {
        host: HostKind::Localhost,
        port: 8080,
    };
    let origin =
        pin_with_lookup(&parsed, |_name| Ok(IpAddr::V6(Ipv6Addr::LOCALHOST))).expect("must accept");
    assert_eq!(origin.port, 8080);
    assert!(matches!(origin.address, LoopbackAddress::V6(_)));
}

#[test]
fn pin_localhost_rejects_non_loopback_dns() {
    let parsed = ParsedOrigin {
        host: HostKind::Localhost,
        port: 8080,
    };
    let err = pin_with_lookup(&parsed, |_name| Ok(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))))
        .expect_err("must reject non-loopback DNS resolution");
    match err {
        OriginError::NonLoopbackDns { host, addr } => {
            assert_eq!(host, "localhost");
            assert_eq!(addr, IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)));
        }
        other => panic!("expected NonLoopbackDns, got {other:?}"),
    }
}

#[test]
fn pin_localhost_rejects_dns_failure() {
    let parsed = ParsedOrigin {
        host: HostKind::Localhost,
        port: 8080,
    };
    let err = pin_with_lookup(&parsed, |_name| {
        Err(OriginError::DnsFailure {
            host: "localhost".to_string(),
        })
    })
    .expect_err("must surface DNS failure");
    assert!(matches!(err, OriginError::DnsFailure { .. }));
}

#[test]
fn pin_literal_ipv4_skips_dns_lookup() {
    let parsed = ParsedOrigin {
        host: HostKind::LiteralV4,
        port: 9000,
    };
    let origin = pin_with_lookup(&parsed, |_| {
        panic!("DNS lookup must not be invoked for literal hosts")
    })
    .expect("must accept");
    assert_eq!(origin.port, 9000);
    let expected = LoopbackAddress::V4(Ipv4Addr::new(127, 0, 0, 1));
    assert_eq!(origin.address, expected);
}

#[test]
fn pin_literal_ipv6_skips_dns_lookup() {
    let parsed = ParsedOrigin {
        host: HostKind::LiteralV6,
        port: 9000,
    };
    let origin = pin_with_lookup(&parsed, |_| {
        panic!("DNS lookup must not be invoked for literal hosts")
    })
    .expect("must accept");
    assert_eq!(origin.port, 9000);
    assert!(matches!(
        origin.address,
        LoopbackAddress::V6(Ipv6Addr::LOCALHOST)
    ));
}

// ---- LocalOrigin equality + display sanity --------------------------------

#[test]
fn local_origin_equality_is_structural() {
    let a = LocalOrigin {
        address: LoopbackAddress::V4(Ipv4Addr::new(127, 0, 0, 1)),
        port: 8080,
    };
    let b = LocalOrigin {
        address: LoopbackAddress::V4(Ipv4Addr::new(127, 0, 0, 1)),
        port: 8080,
    };
    assert_eq!(a, b);
}

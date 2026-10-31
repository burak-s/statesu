use std::net::IpAddr;

use crate::config::IpNet;

/// Client-IP resolution rules: the socket peer is authoritative unless the
/// immediate peer is a trusted proxy. Then `X-Forwarded-For` is walked
/// right-to-left while every already-resolved hop stays inside the trusted
/// list, so spoofed entries left of the real client are ignored.
pub fn client_ip(peer: IpAddr, forwarded_for: Option<&str>, trusted_proxies: &[IpNet]) -> IpAddr {
    let trusted = |ip: IpAddr| trusted_proxies.iter().any(|net| net.contains(ip));
    if !trusted(peer) {
        return peer;
    }
    match forwarded_for {
        None => peer,
        Some(header) => {
            let mut client = peer;
            for entry in header.split(',').map(str::trim).rev() {
                if !trusted(client) {
                    break;
                }
                match entry.parse::<IpAddr>() {
                    Ok(ip) => client = ip,
                    Err(_) => break,
                }
            }
            client
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(input: &str) -> IpNet {
        IpNet::parse(input).unwrap()
    }

    #[test]
    fn untrusted_peer_is_the_client() {
        let trusted = [net("10.0.0.0/8")];
        let peer: IpAddr = "192.0.2.7".parse::<IpAddr>().unwrap();
        assert_eq!(
            client_ip(peer, Some("1.2.3.4, 10.0.0.5"), &trusted),
            peer,
            "spoof attempt from an untrusted socket must be ignored"
        );
    }

    #[test]
    fn trusted_proxy_reads_the_rightmost_forwarded_entry() {
        let trusted = [net("10.0.0.0/8")];
        let proxy: IpAddr = "10.0.0.9".parse::<IpAddr>().unwrap();
        assert_eq!(
            client_ip(proxy, Some("1.2.3.4, 203.0.113.5"), &trusted),
            "203.0.113.5".parse::<IpAddr>().unwrap(),
            "left entries are untrusted input and must be ignored"
        );
    }

    #[test]
    fn chained_trusted_proxies_walk_right_to_left() {
        let trusted = [net("10.0.0.0/8")];
        let proxy: IpAddr = "10.0.0.9".parse::<IpAddr>().unwrap();
        assert_eq!(
            client_ip(proxy, Some("192.0.2.1, 10.1.1.3, 10.2.2.4"), &trusted),
            "192.0.2.1".parse::<IpAddr>().unwrap(),
            "hops that stay inside the trusted list keep being skipped"
        );
    }

    #[test]
    fn missing_header_keeps_the_proxy_ip() {
        let trusted = [net("10.0.0.0/8")];
        let proxy: IpAddr = "10.0.0.9".parse::<IpAddr>().unwrap();
        assert_eq!(client_ip(proxy, None, &trusted), proxy);
    }

    #[test]
    fn no_configured_proxies_never_trust_forwarded_headers() {
        let proxy: IpAddr = "10.0.0.9".parse::<IpAddr>().unwrap();
        assert_eq!(client_ip(proxy, Some("1.2.3.4"), &[]), proxy);
    }

    #[test]
    fn malformed_entries_stop_the_walk() {
        let trusted = [net("10.0.0.0/8")];
        let proxy: IpAddr = "10.0.0.9".parse::<IpAddr>().unwrap();
        assert_eq!(
            client_ip(proxy, Some("garbage, 10.1.1.1"), &trusted),
            "10.1.1.1".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn ipv6_networks_are_matched_separately() {
        let trusted = [net("2001:db8::/32")];
        let proxy: IpAddr = "2001:db8::9".parse::<IpAddr>().unwrap();
        assert_eq!(
            client_ip(proxy, Some("2620:fe::9"), &trusted),
            "2620:fe::9".parse::<IpAddr>().unwrap()
        );
        assert_eq!(client_ip(proxy, Some("not-an-ip"), &trusted), proxy);
        // An IPv4 peer is not covered by an IPv6-CIDR proxy trust entry.
        let ipv4_peer: IpAddr = "10.0.0.1".parse::<IpAddr>().unwrap();
        assert_eq!(client_ip(ipv4_peer, None, &trusted), ipv4_peer);
    }

    #[test]
    fn empty_forwarded_header_is_like_no_header() {
        let trusted = [net("10.0.0.0/8")];
        let proxy: IpAddr = "10.0.0.9".parse::<IpAddr>().unwrap();
        assert_eq!(client_ip(proxy, Some(""), &trusted), proxy);
    }
}

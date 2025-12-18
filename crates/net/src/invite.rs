//! Invite URL generation and parsing
//!
//! Invite format: exom://<host>:<port>/<hall-id>/<token>

use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;

use uuid::Uuid;

use crate::error::{Error, Result};

/// Parsed invite information
#[derive(Debug, Clone)]
pub struct InviteUrl {
    pub host: IpAddr,
    pub port: u16,
    pub hall_id: Uuid,
    pub token: String,
}

impl InviteUrl {
    /// Create a new invite URL
    pub fn new(host: IpAddr, port: u16, hall_id: Uuid, token: String) -> Self {
        Self {
            host,
            port,
            hall_id,
            token,
        }
    }

    /// Create from a socket address
    pub fn from_addr(addr: SocketAddr, hall_id: Uuid, token: String) -> Self {
        Self {
            host: addr.ip(),
            port: addr.port(),
            hall_id,
            token,
        }
    }

    /// Get the socket address for connection
    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.host, self.port)
    }

    /// Format as URL string
    pub fn to_url(&self) -> String {
        format!(
            "exom://{}:{}/{}/{}",
            self.host, self.port, self.hall_id, self.token
        )
    }

    /// Parse from URL string
    pub fn parse(s: &str) -> Result<Self> {
        // Strip protocol prefix
        let s = s
            .strip_prefix("exom://")
            .ok_or_else(|| Error::Protocol("Invalid invite URL: missing exom:// prefix".into()))?;

        // Split into parts: host:port/hall_id/token
        let parts: Vec<&str> = s.splitn(3, '/').collect();
        if parts.len() != 3 {
            return Err(Error::Protocol(
                "Invalid invite URL: expected host:port/hall_id/token".into(),
            ));
        }

        // Parse host:port
        let host_port = parts[0];
        let addr: SocketAddr = host_port.parse().map_err(|_| {
            Error::Protocol(format!("Invalid invite URL: bad address '{}'", host_port))
        })?;

        // Parse hall_id
        let hall_id = Uuid::from_str(parts[1]).map_err(|_| {
            Error::Protocol(format!("Invalid invite URL: bad hall_id '{}'", parts[1]))
        })?;

        // Token is the rest
        let token = parts[2].to_string();
        if token.is_empty() {
            return Err(Error::Protocol("Invalid invite URL: empty token".into()));
        }

        Ok(Self {
            host: addr.ip(),
            port: addr.port(),
            hall_id,
            token,
        })
    }
}

impl std::fmt::Display for InviteUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_url())
    }
}

impl FromStr for InviteUrl {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_invite_roundtrip() {
        let hall_id = Uuid::new_v4();
        let invite = InviteUrl::new(
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)),
            7331,
            hall_id,
            "abc123".to_string(),
        );

        let url = invite.to_url();
        let parsed = InviteUrl::parse(&url).unwrap();

        assert_eq!(parsed.host, invite.host);
        assert_eq!(parsed.port, invite.port);
        assert_eq!(parsed.hall_id, invite.hall_id);
        assert_eq!(parsed.token, invite.token);
    }

    #[test]
    fn test_invite_parse_ipv4() {
        let url = "exom://192.168.1.1:7331/550e8400-e29b-41d4-a716-446655440000/mytoken";
        let invite = InviteUrl::parse(url).unwrap();

        assert_eq!(invite.host, IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));
        assert_eq!(invite.port, 7331);
        assert_eq!(invite.token, "mytoken");
    }

    #[test]
    fn test_invite_parse_ipv6() {
        let url = "exom://[::1]:7331/550e8400-e29b-41d4-a716-446655440000/mytoken";
        let invite = InviteUrl::parse(url).unwrap();

        assert_eq!(invite.port, 7331);
        assert_eq!(invite.token, "mytoken");
    }

    #[test]
    fn test_invite_parse_invalid() {
        // Missing prefix
        assert!(InviteUrl::parse("http://localhost/abc/def").is_err());

        // Missing parts
        assert!(InviteUrl::parse("exom://localhost").is_err());

        // Bad UUID
        assert!(InviteUrl::parse("exom://localhost:7331/not-a-uuid/token").is_err());

        // Empty token
        assert!(
            InviteUrl::parse("exom://localhost:7331/550e8400-e29b-41d4-a716-446655440000/")
                .is_err()
        );
    }
}

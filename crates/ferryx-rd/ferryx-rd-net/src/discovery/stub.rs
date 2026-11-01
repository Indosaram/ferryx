use super::{DiscoveredHost, DiscoveryError};
use std::net::SocketAddr;

pub struct StubBrowser;

impl StubBrowser {
    pub fn new() -> Result<Self, DiscoveryError> {
        Err(DiscoveryError::Unavailable)
    }

    pub fn snapshot(&self) -> Result<Vec<DiscoveredHost>, DiscoveryError> {
        Err(DiscoveryError::Unavailable)
    }
}

pub struct StubAdvertiser;

impl StubAdvertiser {
    pub fn start(
        _name: &str,
        _tcp_port: u16,
        _udp_port: u16,
        _bind_addr: SocketAddr,
    ) -> Result<Self, DiscoveryError> {
        Err(DiscoveryError::Unavailable)
    }
}

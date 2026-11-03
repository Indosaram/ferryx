//! Minimal RFC 5389 STUN binding client.

use std::{
    io,
    net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs},
    time::Duration,
};

use openssl::rand::rand_bytes;
use thiserror::Error;
use tokio::{
    net::UdpSocket,
    time::{timeout_at, Instant},
};

const DEFAULT_SERVER: &str = "stun.l.google.com:19302";
const MAGIC_COOKIE: u32 = 0x2112_a442;
const HEADER_SIZE: usize = 20;
const BINDING_REQUEST: u16 = 0x0001;
const BINDING_SUCCESS_RESPONSE: u16 = 0x0101;
const XOR_MAPPED_ADDRESS: u16 = 0x0020;
const MAPPED_ADDRESS: u16 = 0x0001;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub enum StunError {
    #[error("failed to resolve STUN server: {0}")]
    Resolve(#[source] io::Error),
    #[error("STUN server resolved no IPv4 addresses")]
    NoAddress,
    #[error("STUN UDP I/O failed: {0}")]
    Io(#[source] io::Error),
    #[error("failed to generate STUN transaction ID: {0}")]
    Random(#[source] openssl::error::ErrorStack),
    #[error("STUN request timed out")]
    Timeout,
    #[error("invalid STUN response: {0}")]
    Invalid(&'static str),
    #[error("STUN response does not contain a mapped IPv4 address")]
    MissingMappedAddress,
}

#[derive(Debug, Clone)]
pub struct StunClient {
    server: String,
    request_timeout: Duration,
}

impl Default for StunClient {
    fn default() -> Self {
        Self::new(DEFAULT_SERVER)
    }
}

impl StunClient {
    pub fn new(server: impl Into<String>) -> Self {
        Self {
            server: server.into(),
            request_timeout: REQUEST_TIMEOUT,
        }
    }

    /// Performs a STUN binding on `socket` — the UDP socket the caller retains
    /// for peer traffic — so the returned mapped address describes a port that
    /// stays open. `socket` must be bound (any address); the STUN server is
    /// contacted with `send_to`, never `connect`, so the socket's peer is left
    /// untouched and other traffic on it is unaffected.
    pub async fn fetch_public_address(&self, socket: &UdpSocket) -> Result<SocketAddr, StunError> {
        let server = resolve_ipv4(&self.server)?;

        let mut transaction_id = [0_u8; 12];
        rand_bytes(&mut transaction_id).map_err(StunError::Random)?;
        let request = binding_request(transaction_id);
        socket
            .send_to(&request, server)
            .await
            .map_err(StunError::Io)?;

        let mut response = [0_u8; 2048];
        let deadline = Instant::now() + self.request_timeout;
        loop {
            let (count, source) = match timeout_at(deadline, socket.recv_from(&mut response)).await
            {
                Err(_) => return Err(StunError::Timeout),
                Ok(result) => result.map_err(StunError::Io)?,
            };
            // Ignore datagrams that are not from the STUN server; the
            // transaction ID check in `parse_binding_response` handles
            // spoofed replies bearing the server's address.
            if source != server {
                continue;
            }
            return parse_binding_response(&response[..count], transaction_id);
        }
    }
}

fn resolve_ipv4(server: &str) -> Result<SocketAddr, StunError> {
    server
        .to_socket_addrs()
        .map_err(StunError::Resolve)?
        .find(SocketAddr::is_ipv4)
        .ok_or(StunError::NoAddress)
}

fn binding_request(transaction_id: [u8; 12]) -> [u8; HEADER_SIZE] {
    let mut request = [0_u8; HEADER_SIZE];
    request[..2].copy_from_slice(&BINDING_REQUEST.to_be_bytes());
    request[4..8].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
    request[8..].copy_from_slice(&transaction_id);
    request
}

fn parse_binding_response(data: &[u8], transaction_id: [u8; 12]) -> Result<SocketAddr, StunError> {
    if data.len() < HEADER_SIZE {
        return Err(StunError::Invalid("truncated header"));
    }
    let message_type = u16::from_be_bytes([data[0], data[1]]);
    if message_type != BINDING_SUCCESS_RESPONSE {
        return Err(StunError::Invalid("not a binding success response"));
    }
    let message_length = u16::from_be_bytes([data[2], data[3]]) as usize;
    if HEADER_SIZE + message_length > data.len() {
        return Err(StunError::Invalid("truncated attributes"));
    }
    if data[4..8] != MAGIC_COOKIE.to_be_bytes() {
        return Err(StunError::Invalid("wrong magic cookie"));
    }
    if data[8..20] != transaction_id {
        return Err(StunError::Invalid("transaction ID mismatch"));
    }

    let attributes_end = HEADER_SIZE + message_length;
    let mut offset = HEADER_SIZE;
    while offset + 4 <= attributes_end {
        let attribute_type = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let attribute_length = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
        offset += 4;
        let value_offset = offset;
        let value_end = value_offset
            .checked_add(attribute_length)
            .ok_or(StunError::Invalid("attribute length overflow"))?;
        if value_end > attributes_end {
            return Err(StunError::Invalid("truncated attribute"));
        }

        let value = &data[value_offset..value_end];
        let parsed = match attribute_type {
            XOR_MAPPED_ADDRESS => parse_xor_mapped_address(value),
            MAPPED_ADDRESS => parse_mapped_address(value),
            _ => None,
        };
        if let Some(address) = parsed {
            return Ok(address);
        }

        let padded_length = attribute_length
            .checked_add(3)
            .ok_or(StunError::Invalid("attribute padding overflow"))?
            & !3;
        offset = value_offset
            .checked_add(padded_length)
            .ok_or(StunError::Invalid("attribute offset overflow"))?;
    }

    Err(StunError::MissingMappedAddress)
}

fn parse_xor_mapped_address(value: &[u8]) -> Option<SocketAddr> {
    if value.len() < 8 || value[1] != 0x01 {
        return None;
    }
    let port = u16::from_be_bytes([value[2], value[3]]) ^ (MAGIC_COOKIE >> 16) as u16;
    let encoded_ip = u32::from_be_bytes(value[4..8].try_into().ok()?);
    let ip = Ipv4Addr::from(encoded_ip ^ MAGIC_COOKIE);
    Some(SocketAddr::new(IpAddr::V4(ip), port))
}

fn parse_mapped_address(value: &[u8]) -> Option<SocketAddr> {
    if value.len() < 8 || value[1] != 0x01 {
        return None;
    }
    let port = u16::from_be_bytes([value[2], value[3]]);
    let ip = Ipv4Addr::new(value[4], value[5], value[6], value[7]);
    Some(SocketAddr::new(IpAddr::V4(ip), port))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response_with_attribute(
        transaction_id: [u8; 12],
        attribute_type: u16,
        value: &[u8],
    ) -> Vec<u8> {
        let padded_length = (value.len() + 3) & !3;
        let mut response = Vec::with_capacity(HEADER_SIZE + 4 + padded_length);
        response.extend_from_slice(&BINDING_SUCCESS_RESPONSE.to_be_bytes());
        response.extend_from_slice(&u16::try_from(4 + padded_length).unwrap().to_be_bytes());
        response.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        response.extend_from_slice(&transaction_id);
        response.extend_from_slice(&attribute_type.to_be_bytes());
        response.extend_from_slice(&u16::try_from(value.len()).unwrap().to_be_bytes());
        response.extend_from_slice(value);
        response.resize(HEADER_SIZE + 4 + padded_length, 0);
        response
    }

    #[test]
    fn binding_request_has_rfc5389_header() {
        let transaction_id = [7_u8; 12];
        let request = binding_request(transaction_id);
        assert_eq!(&request[..2], &BINDING_REQUEST.to_be_bytes());
        assert_eq!(&request[2..4], &[0, 0]);
        assert_eq!(&request[4..8], &MAGIC_COOKIE.to_be_bytes());
        assert_eq!(&request[8..], &transaction_id);
    }

    #[test]
    fn parses_xor_mapped_ipv4_address() {
        let transaction_id = [1_u8; 12];
        let address = Ipv4Addr::new(203, 0, 113, 9);
        let port = 40123_u16;
        let mut value = vec![0, 1];
        value.extend_from_slice(&(port ^ (MAGIC_COOKIE >> 16) as u16).to_be_bytes());
        value.extend_from_slice(&(u32::from(address) ^ MAGIC_COOKIE).to_be_bytes());
        let response = response_with_attribute(transaction_id, XOR_MAPPED_ADDRESS, &value);
        assert_eq!(
            parse_binding_response(&response, transaction_id).unwrap(),
            SocketAddr::new(IpAddr::V4(address), port)
        );
    }

    #[test]
    fn skips_padded_unknown_attribute_and_parses_fallback() {
        let transaction_id = [2_u8; 12];
        let mut response = response_with_attribute(transaction_id, 0x8022, b"abc");
        let mut mapped = vec![0, 1];
        mapped.extend_from_slice(&19730_u16.to_be_bytes());
        mapped.extend_from_slice(&[192, 0, 2, 5]);
        let attribute_length = 4 + mapped.len();
        let old_message_length = u16::from_be_bytes([response[2], response[3]]) as usize;
        response[2..4].copy_from_slice(
            &u16::try_from(old_message_length + attribute_length)
                .unwrap()
                .to_be_bytes(),
        );
        response.extend_from_slice(&MAPPED_ADDRESS.to_be_bytes());
        response.extend_from_slice(&u16::try_from(mapped.len()).unwrap().to_be_bytes());
        response.extend_from_slice(&mapped);
        assert_eq!(
            parse_binding_response(&response, transaction_id).unwrap(),
            "192.0.2.5:19730".parse().unwrap()
        );
    }

    #[test]
    fn rejects_wrong_transaction_id() {
        let response =
            response_with_attribute([3_u8; 12], MAPPED_ADDRESS, &[0, 1, 0, 1, 1, 2, 3, 4]);
        assert!(matches!(
            parse_binding_response(&response, [4_u8; 12]),
            Err(StunError::Invalid("transaction ID mismatch"))
        ));
    }
}

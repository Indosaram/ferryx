//! AES-256-GCM protection for v3 UDP datagrams.

use std::collections::HashMap;

use aes_gcm::{
    aead::{Aead, AeadInPlace, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use hkdf::Hkdf;
use ferryx_rd_proto::{PacketHeader, WireCodec};
use sha2::Sha256;
use thiserror::Error;

/// Best-effort DSCP request. Routers/WMM may ignore or rewrite this marking.
/// Set MAHO_DISABLE_QOS to any value to retain the operating-system default.
/// Call only during socket setup: cloned UDP sockets share this setting. Hosts
/// use AF41 for the whole media socket (including audio), not EF/AF41 switching,
/// because per-packet setsockopt races with concurrent sessions.
pub fn configure_media_qos(socket: &std::net::UdpSocket, dscp: u8) -> std::io::Result<()> {
    let disabled = qos_opt_out(std::env::var_os("MAHO_DISABLE_QOS").as_deref());
    let ipv6 = socket.local_addr()?.is_ipv6();
    match media_qos_option(QosTarget::CURRENT, dscp, ipv6, disabled)? {
        Some(option) => set_traffic_class(socket, option),
        None => Ok(()),
    }
}

/// Socket-option constant family for the traffic-class request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QosTarget {
    /// Linux and other non-Apple Unix targets.
    #[cfg(any(test, not(any(target_os = "macos", target_os = "ios", windows))))]
    Linux,
    /// macOS / iOS (BSD constants).
    #[cfg(any(test, target_os = "macos", target_os = "ios"))]
    Apple,
    /// Winsock: IPPROTO_IP/IP_TOS and IPPROTO_IPV6/IPV6_TCLASS.
    #[cfg(any(test, windows))]
    Windows,
}

impl QosTarget {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const CURRENT: Self = Self::Apple;
    #[cfg(windows)]
    const CURRENT: Self = Self::Windows;
    #[cfg(not(any(target_os = "macos", target_os = "ios", windows)))]
    const CURRENT: Self = Self::Linux;
}

/// Fully resolved `setsockopt` arguments for a traffic-class request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TrafficClassOption {
    level: i32,
    name: i32,
    value: i32,
}

/// Opt-out is any presence of the variable, including an empty value.
fn qos_opt_out(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some()
}

/// Pure option builder: `None` means leave the OS default untouched.
fn media_qos_option(
    target: QosTarget,
    dscp: u8,
    ipv6: bool,
    disabled: bool,
) -> std::io::Result<Option<TrafficClassOption>> {
    if disabled {
        return Ok(None);
    }
    if dscp > 0x3f {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("DSCP {dscp} exceeds 6 bits"),
        ));
    }
    // IPPROTO_IP = 0 and IPPROTO_IPV6 = 41 on every supported target.
    let (level, name) = match (target, ipv6) {
        #[cfg(any(test, not(any(target_os = "macos", target_os = "ios", windows))))]
        (QosTarget::Linux, false) => (0, 1), // IP_TOS
        #[cfg(any(test, not(any(target_os = "macos", target_os = "ios", windows))))]
        (QosTarget::Linux, true) => (41, 67), // IPV6_TCLASS
        #[cfg(any(test, target_os = "macos", target_os = "ios"))]
        (QosTarget::Apple, false) => (0, 3), // IP_TOS
        #[cfg(any(test, target_os = "macos", target_os = "ios"))]
        (QosTarget::Apple, true) => (41, 36), // IPV6_TCLASS
        #[cfg(any(test, windows))]
        (QosTarget::Windows, false) => (0, 3), // IP_TOS
        #[cfg(any(test, windows))]
        (QosTarget::Windows, true) => (41, 39), // IPV6_TCLASS
    };
    // DSCP occupies the upper six bits; ECN bits stay zero.
    Ok(Some(TrafficClassOption {
        level,
        name,
        value: i32::from(dscp) << 2,
    }))
}

#[cfg(unix)]
fn set_traffic_class(
    socket: &std::net::UdpSocket,
    option: TrafficClassOption,
) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    unsafe extern "C" {
        fn setsockopt(fd: i32, level: i32, name: i32, value: *const i32, len: u32) -> i32;
    }
    // SAFETY: value is an initialized integer of the supplied size; fd remains borrowed.
    let result = unsafe {
        setsockopt(
            socket.as_raw_fd(),
            option.level,
            option.name,
            &option.value,
            4,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(windows)]
fn set_traffic_class(
    socket: &std::net::UdpSocket,
    option: TrafficClassOption,
) -> std::io::Result<()> {
    use std::os::windows::io::AsRawSocket;
    #[link(name = "ws2_32")]
    unsafe extern "system" {
        fn setsockopt(socket: usize, level: i32, name: i32, value: *const u8, len: i32) -> i32;
        fn WSAGetLastError() -> i32;
    }
    // SAFETY: socket is borrowed and the pointer names a valid DWORD-sized integer.
    let result = unsafe {
        setsockopt(
            socket.as_raw_socket() as usize,
            option.level,
            option.name,
            (&option.value as *const i32).cast(),
            4,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(unsafe {
            WSAGetLastError()
        }))
    }
}

/// Send independently encrypted datagrams without changing their envelope or MTU.
/// Returns the successfully submitted prefix; callers must handle partial batches.
#[cfg(target_os = "linux")]
pub fn send_datagram_batch(
    socket: &std::net::UdpSocket,
    peer: std::net::SocketAddr,
    datagrams: &[Vec<u8>],
) -> std::io::Result<usize> {
    use std::ffi::c_void;
    use std::os::fd::AsRawFd;
    #[repr(C)]
    struct IoVec {
        base: *const u8,
        len: usize,
    }
    #[repr(C)]
    struct Msg {
        name: *const c_void,
        name_len: u32,
        iov: *mut IoVec,
        iov_len: usize,
        control: *const c_void,
        control_len: usize,
        flags: i32,
    }
    #[repr(C)]
    struct MultiMsg {
        msg: Msg,
        len: u32,
    }
    #[repr(C, align(8))]
    struct Address([u8; 28]);
    unsafe extern "C" {
        fn sendmmsg(fd: i32, messages: *mut MultiMsg, count: u32, flags: i32) -> i32;
    }
    let mut address = Address([0; 28]);
    let size = match peer {
        std::net::SocketAddr::V4(peer) => {
            address.0[..2].copy_from_slice(&2u16.to_ne_bytes());
            address.0[2..4].copy_from_slice(&peer.port().to_be_bytes());
            address.0[4..8].copy_from_slice(&peer.ip().octets());
            16
        }
        std::net::SocketAddr::V6(peer) => {
            address.0[..2].copy_from_slice(&10u16.to_ne_bytes());
            address.0[2..4].copy_from_slice(&peer.port().to_be_bytes());
            address.0[4..8].copy_from_slice(&peer.flowinfo().to_be_bytes());
            address.0[8..24].copy_from_slice(&peer.ip().octets());
            address.0[24..28].copy_from_slice(&peer.scope_id().to_ne_bytes());
            28
        }
    };
    let mut iov: Vec<_> = datagrams
        .iter()
        .map(|d| IoVec {
            base: d.as_ptr(),
            len: d.len(),
        })
        .collect();
    let mut messages: Vec<_> = iov
        .iter_mut()
        .map(|iov| MultiMsg {
            msg: Msg {
                name: address.0.as_ptr().cast(),
                name_len: size,
                iov,
                iov_len: 1,
                control: std::ptr::null(),
                control_len: 0,
                flags: 0,
            },
            len: 0,
        })
        .collect();
    if messages.is_empty() {
        return Ok(0);
    }
    let count = u32::try_from(messages.len())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    // SAFETY: all message, address and iovec backing storage lives through this call.
    // MSG_DONTWAIT prevents kernel-buffer congestion from blocking audio dispatch.
    let result = unsafe { sendmmsg(socket.as_raw_fd(), messages.as_mut_ptr(), count, 0x40) };
    if result < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(result as usize)
    }
}

pub const NONCE_SIZE: usize = 12;
pub const TAG_SIZE: usize = 16;
pub const REPLAY_WINDOW_SIZE: u64 = 4096;
const HEADER_SIZE: usize = PacketHeader::SIZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    ClientToHost,
    HostToClient,
}

impl Direction {
    fn info(self) -> &'static [u8] {
        // Wire-protocol v3 constants: these bytes are HKDF info inputs that
        // both peers must agree on. Never rename them.
        match self {
            Self::ClientToHost => b"erd/udp-c2h/v3",
            Self::HostToClient => b"erd/udp-h2c/v3",
        }
    }
}

#[derive(Debug, Error)]
pub enum DatagramError {
    #[error("pairing key must be 32 bytes")]
    InvalidMasterKey,
    #[error("session salt must be 16 bytes")]
    InvalidSessionSalt,
    #[error("datagram counter exhausted")]
    CounterExhausted,
    #[error("datagram is too short")]
    Truncated,
    #[error("datagram nonce prefix does not match this direction")]
    WrongDirection,
    #[error("datagram is duplicated or outside the replay window")]
    Replay,
    #[error("datagram authentication failed")]
    Authentication,
    #[error("invalid packet header: {0}")]
    InvalidHeader(#[from] ferryx_rd_proto::CodecError),
}

#[derive(Debug, Default)]
struct ReplayWindow {
    highest_received: u64,
    received_any: bool,
    blocks: HashMap<u64, u64>,
    #[cfg(test)]
    prune_visits: usize,
}

impl ReplayWindow {
    fn can_accept(&self, counter: u64) -> bool {
        if self.received_any
            && counter < self.highest_received
            && self.highest_received - counter >= REPLAY_WINDOW_SIZE
        {
            return false;
        }

        let block = counter / 64;
        let mask = 1_u64 << (counter % 64);
        self.blocks
            .get(&block)
            .map_or(true, |seen| seen & mask == 0)
    }

    fn record(&mut self, counter: u64) {
        let block = counter / 64;
        let mask = 1_u64 << (counter % 64);
        *self.blocks.entry(block).or_default() |= mask;

        let previous_oldest_block = self
            .highest_received
            .saturating_sub(REPLAY_WINDOW_SIZE.saturating_sub(1))
            / 64;
        if !self.received_any || counter > self.highest_received {
            self.highest_received = counter;
            self.received_any = true;
        }

        let oldest_counter = self
            .highest_received
            .saturating_sub(REPLAY_WINDOW_SIZE.saturating_sub(1));
        let oldest_block = oldest_counter / 64;
        if oldest_block > previous_oldest_block {
            self.blocks.retain(|block, _| {
                #[cfg(test)]
                {
                    self.prune_visits += 1;
                }
                *block >= oldest_block
            });
        }
    }
}

/// Direction-specific UDP traffic cipher with send-counter and replay state.
pub struct DatagramCipher {
    cipher: Aes256Gcm,
    nonce_prefix: [u8; 4],
    send_counter: u64,
    replay: ReplayWindow,
}

impl DatagramCipher {
    pub fn derive(
        master_key: &[u8],
        session_salt: &[u8],
        direction: Direction,
    ) -> Result<Self, DatagramError> {
        if master_key.len() != 32 {
            return Err(DatagramError::InvalidMasterKey);
        }
        if session_salt.len() != 16 {
            return Err(DatagramError::InvalidSessionSalt);
        }

        // Wire-protocol v3 constant: HKDF info input, never rename.
        let udp_ikm = hkdf_sha256(master_key, session_salt, b"erd/udp-ikm/v3", 32);
        let key = hkdf_sha256(&udp_ikm, session_salt, direction.info(), 32);
        let mut nonce_info = direction.info().to_vec();
        nonce_info.extend_from_slice(b"/nonce");
        let prefix = hkdf_sha256(&udp_ikm, session_salt, &nonce_info, 4);

        let mut nonce_prefix = [0_u8; 4];
        nonce_prefix.copy_from_slice(&prefix);
        Ok(Self {
            cipher: Aes256Gcm::new_from_slice(&key).expect("validated AES-256 key length"),
            nonce_prefix,
            send_counter: 0,
            replay: ReplayWindow::default(),
        })
    }

    /// Seals a payload as `nonce || ciphertext || tag`, authenticating `aad`.
    pub fn seal(&mut self, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, DatagramError> {
        let mut output = Vec::with_capacity(NONCE_SIZE + plaintext.len() + TAG_SIZE);
        self.seal_into(plaintext, aad, &mut output)?;
        Ok(output)
    }

    /// Appends `nonce || ciphertext || tag` to a caller-owned buffer.
    /// Existing bytes are preserved, including on error. With sufficient spare
    /// capacity this allocates nothing. Each attempt consumes a fresh counter;
    /// exhaustion leaves the buffer unchanged and never wraps the nonce.
    pub fn seal_into(
        &mut self,
        plaintext: &[u8],
        aad: &[u8],
        output: &mut Vec<u8>,
    ) -> Result<(), DatagramError> {
        self.send_counter = self
            .send_counter
            .checked_add(1)
            .ok_or(DatagramError::CounterExhausted)?;

        let mut nonce_bytes = [0_u8; NONCE_SIZE];
        nonce_bytes[..4].copy_from_slice(&self.nonce_prefix);
        nonce_bytes[4..].copy_from_slice(&self.send_counter.to_be_bytes());
        let start = output.len();
        output.reserve(NONCE_SIZE + plaintext.len() + TAG_SIZE);
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(plaintext);
        let tag = self.cipher.encrypt_in_place_detached(
            Nonce::from_slice(&nonce_bytes),
            aad,
            &mut output[start + NONCE_SIZE..],
        );
        match tag {
            Ok(tag) => output.extend_from_slice(&tag),
            Err(_) => {
                output.truncate(start);
                return Err(DatagramError::Authentication);
            }
        }
        Ok(())
    }

    /// Opens `nonce || ciphertext || tag`, rejecting tampering and replay.
    pub fn open(&mut self, sealed: &[u8], aad: &[u8]) -> Result<Vec<u8>, DatagramError> {
        if sealed.len() < NONCE_SIZE + TAG_SIZE {
            return Err(DatagramError::Truncated);
        }
        if sealed[..4] != self.nonce_prefix {
            return Err(DatagramError::WrongDirection);
        }

        let counter = u64::from_be_bytes(
            sealed[4..NONCE_SIZE]
                .try_into()
                .expect("nonce counter is eight bytes"),
        );
        if !self.replay.can_accept(counter) {
            return Err(DatagramError::Replay);
        }

        let plaintext = self
            .cipher
            .decrypt(
                Nonce::from_slice(&sealed[..NONCE_SIZE]),
                Payload {
                    msg: &sealed[NONCE_SIZE..],
                    aad,
                },
            )
            .map_err(|_| DatagramError::Authentication)?;
        self.replay.record(counter);
        Ok(plaintext)
    }

    /// Seals a complete v3 datagram: plaintext header plus protected payload.
    pub fn seal_datagram(
        &mut self,
        header: &PacketHeader,
        payload: &[u8],
    ) -> Result<Vec<u8>, DatagramError> {
        let header_bytes = header.encode()?;
        let mut datagram = Vec::with_capacity(HEADER_SIZE + NONCE_SIZE + payload.len() + TAG_SIZE);
        datagram.extend_from_slice(&header_bytes);
        self.seal_into(payload, &header_bytes, &mut datagram)?;
        Ok(datagram)
    }

    /// Opens a complete v3 datagram and returns its validated header and payload.
    pub fn open_datagram(
        &mut self,
        datagram: &[u8],
    ) -> Result<(PacketHeader, Vec<u8>), DatagramError> {
        if datagram.len() < HEADER_SIZE + NONCE_SIZE + TAG_SIZE {
            return Err(DatagramError::Truncated);
        }
        let header_bytes = &datagram[..HEADER_SIZE];
        let header = PacketHeader::decode(header_bytes)?;
        let payload = self.open(&datagram[HEADER_SIZE..], header_bytes)?;
        Ok((header, payload))
    }
}

pub(crate) fn hkdf_sha256(ikm: &[u8], salt: &[u8], info: &[u8], length: usize) -> Vec<u8> {
    let hkdf = Hkdf::<Sha256>::new(Some(salt), ikm);
    let mut output = vec![0_u8; length];
    hkdf.expand(info, &mut output)
        .expect("all protocol HKDF output lengths are valid");
    output
}

#[cfg(test)]
pub(crate) mod tests {
    use ferryx_rd_proto::{PacketType, WireCodec};

    use super::*;

    const MASTER_KEY: [u8; 32] = [0x42; 32];
    const SESSION_SALT: [u8; 16] = [0x24; 16];
    const AAD: &[u8] = b"maho-packet-header";

    fn matching_pair() -> (DatagramCipher, DatagramCipher) {
        (
            DatagramCipher::derive(&MASTER_KEY, &SESSION_SALT, Direction::ClientToHost).unwrap(),
            DatagramCipher::derive(&MASTER_KEY, &SESSION_SALT, Direction::ClientToHost).unwrap(),
        )
    }

    const AF41: u8 = 34;

    #[test]
    fn windows_qos_uses_ip_tos_and_ipv6_tclass() {
        let v4 = media_qos_option(QosTarget::Windows, AF41, false, false).unwrap();
        let v6 = media_qos_option(QosTarget::Windows, AF41, true, false).unwrap();
        assert_eq!(
            v4,
            Some(TrafficClassOption {
                level: 0,
                name: 3,
                value: 0x88
            })
        );
        assert_eq!(
            v6,
            Some(TrafficClassOption {
                level: 41,
                name: 39,
                value: 0x88
            })
        );
    }

    #[test]
    fn unix_qos_constants_match_platform_headers() {
        let cases = [
            (QosTarget::Linux, false, (0, 1)),
            (QosTarget::Linux, true, (41, 67)),
            (QosTarget::Apple, false, (0, 3)),
            (QosTarget::Apple, true, (41, 36)),
        ];
        for (target, ipv6, (level, name)) in cases {
            let option = media_qos_option(target, AF41, ipv6, false)
                .unwrap()
                .unwrap();
            assert_eq!(
                (option.level, option.name, option.value),
                (level, name, 0x88)
            );
        }
    }

    #[test]
    fn qos_value_shifts_dscp_and_rejects_more_than_six_bits() {
        for (dscp, value) in [(0, 0), (34, 0x88), (46, 0xb8), (63, 0xfc)] {
            let option = media_qos_option(QosTarget::Windows, dscp, false, false)
                .unwrap()
                .unwrap();
            assert_eq!(option.value, value);
        }
        let error = media_qos_option(QosTarget::Windows, 64, false, false).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn qos_opt_out_skips_every_target_without_env_mutation() {
        assert!(!qos_opt_out(None));
        assert!(qos_opt_out(Some(std::ffi::OsStr::new(""))));
        assert!(qos_opt_out(Some(std::ffi::OsStr::new("1"))));
        for target in [QosTarget::Linux, QosTarget::Apple, QosTarget::Windows] {
            for ipv6 in [false, true] {
                assert_eq!(media_qos_option(target, AF41, ipv6, true).unwrap(), None);
                // Opt-out wins even over an otherwise invalid DSCP.
                assert_eq!(media_qos_option(target, 0xff, ipv6, true).unwrap(), None);
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn batched_datagrams_keep_independent_encryption_and_order() {
        for bind in ["127.0.0.1:0", "[::1]:0"] {
            let tx = std::net::UdpSocket::bind(bind).unwrap();
            let rx = std::net::UdpSocket::bind(bind).unwrap();
            rx.set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let (mut cipher, mut decipher) = matching_pair();
            let batch: Vec<_> = (1..=8)
                .map(|sequence| {
                    cipher
                        .seal_datagram(
                            &PacketHeader::new(PacketType::FrameChunk, sequence, 0, 0),
                            &[sequence as u8; 32],
                        )
                        .unwrap()
                })
                .collect();
            assert_eq!(
                send_datagram_batch(&tx, rx.local_addr().unwrap(), &batch).unwrap(),
                8
            );
            for sequence in 1..=8 {
                let mut packet = [0; 1200];
                let size = rx.recv(&mut packet).unwrap();
                let (header, payload) = decipher.open_datagram(&packet[..size]).unwrap();
                assert_eq!(header.sequence, sequence);
                assert_eq!(payload, vec![sequence as u8; 32]);
            }
        }
    }

    #[test]
    fn hkdf_rfc5869_case_1() {
        let ikm = [0x0b; 22];
        let salt = (0x00_u8..=0x0c).collect::<Vec<_>>();
        let info = (0xf0_u8..=0xf9).collect::<Vec<_>>();
        assert_eq!(
            hkdf_sha256(&ikm, &salt, &info, 42),
            [
                0x3c, 0xb2, 0x5f, 0x25, 0xfa, 0xac, 0xd5, 0x7a, 0x90, 0x43, 0x4f, 0x64, 0xd0, 0x36,
                0x2f, 0x2a, 0x2d, 0x2d, 0x0a, 0x90, 0xcf, 0x1a, 0x5a, 0x4c, 0x5d, 0xb0, 0x2d, 0x56,
                0xec, 0xc4, 0xc5, 0xbf, 0x34, 0x00, 0x72, 0x08, 0xd5, 0xb8, 0x87, 0x18, 0x58, 0x65,
            ]
        );
    }

    #[test]
    fn hkdf_rfc5869_case_3() {
        let ikm = [0x0b; 22];
        assert_eq!(
            hkdf_sha256(&ikm, &[], &[], 42),
            [
                0x8d, 0xa4, 0xe7, 0x75, 0xa5, 0x63, 0xc1, 0x8f, 0x71, 0x5f, 0x80, 0x2a, 0x06, 0x3c,
                0x5a, 0x31, 0xb8, 0xa1, 0x1f, 0x5c, 0x5e, 0xe1, 0x87, 0x9e, 0xc3, 0x45, 0x4e, 0x5f,
                0x3c, 0x73, 0x8d, 0x2d, 0x9d, 0x20, 0x13, 0x95, 0xfa, 0xa4, 0xb6, 0x1a, 0x96, 0xc8,
            ]
        );
    }

    #[test]
    fn direction_derivation_matches_v3_vectors() {
        // Wire-protocol v3 constants: HKDF info inputs, never rename.
        let udp_ikm = hkdf_sha256(&MASTER_KEY, &SESSION_SALT, b"erd/udp-ikm/v3", 32);
        assert_eq!(
            hkdf_sha256(&udp_ikm, &SESSION_SALT, b"erd/udp-c2h/v3", 32),
            [
                0x7b, 0x6b, 0xe7, 0xd1, 0xaa, 0xe9, 0xb3, 0xd2, 0x3a, 0x4c, 0xb7, 0xcc, 0x9b, 0x56,
                0x44, 0xcd, 0x88, 0x89, 0x38, 0xdc, 0x81, 0x29, 0x1b, 0x6e, 0x7f, 0x76, 0x98, 0x08,
                0x37, 0xa0, 0x79, 0x99,
            ]
        );
        assert_eq!(
            hkdf_sha256(&udp_ikm, &SESSION_SALT, b"erd/udp-c2h/v3/nonce", 4),
            [0xe2, 0x6c, 0x23, 0xc1]
        );
        assert_eq!(
            hkdf_sha256(&udp_ikm, &SESSION_SALT, b"erd/udp-h2c/v3", 32),
            [
                0x13, 0x56, 0x91, 0xaf, 0x33, 0x8c, 0x20, 0x5f, 0x66, 0x92, 0x40, 0xb4, 0x6b, 0xc4,
                0x34, 0x86, 0x8d, 0x85, 0xfa, 0x66, 0xf8, 0x36, 0xe0, 0x10, 0xf9, 0x1d, 0x27, 0x05,
                0xac, 0xc9, 0xa5, 0x16,
            ]
        );
        assert_eq!(
            hkdf_sha256(&udp_ikm, &SESSION_SALT, b"erd/udp-h2c/v3/nonce", 4),
            [0xcb, 0xbf, 0xd2, 0xe6]
        );
    }

    #[test]
    fn seal_open_round_trip() {
        let (mut sender, mut receiver) = matching_pair();
        let plaintext = (0..1000).map(|n| (n % 251) as u8).collect::<Vec<_>>();
        let datagram = sender.seal(&plaintext, AAD).unwrap();
        assert_eq!(datagram.len(), plaintext.len() + NONCE_SIZE + TAG_SIZE);
        assert_eq!(receiver.open(&datagram, AAD).unwrap(), plaintext);
    }

    #[test]
    fn tampered_datagram_rejected() {
        let (mut sender, mut receiver) = matching_pair();
        let mut datagram = sender.seal(b"payload", AAD).unwrap();
        *datagram.last_mut().unwrap() ^= 0xff;
        assert!(matches!(
            receiver.open(&datagram, AAD),
            Err(DatagramError::Authentication)
        ));
    }

    #[test]
    fn aad_mismatch_rejected() {
        let (mut sender, mut receiver) = matching_pair();
        let datagram = sender.seal(b"payload", AAD).unwrap();
        assert!(matches!(
            receiver.open(&datagram, b"different"),
            Err(DatagramError::Authentication)
        ));
    }

    #[test]
    fn duplicate_datagram_rejected() {
        let (mut sender, mut receiver) = matching_pair();
        let datagram = sender.seal(b"once", AAD).unwrap();
        assert_eq!(receiver.open(&datagram, AAD).unwrap(), b"once");
        assert!(matches!(
            receiver.open(&datagram, AAD),
            Err(DatagramError::Replay)
        ));
    }

    #[test]
    fn stale_datagram_outside_window_rejected() {
        let (mut sender, mut receiver) = matching_pair();
        let mut first = Vec::new();
        let mut latest = Vec::new();
        for index in 0..=REPLAY_WINDOW_SIZE + 8 {
            let datagram = sender.seal(b"stale", AAD).unwrap();
            if index == 0 {
                first = datagram.clone();
            }
            latest = datagram;
        }
        assert_eq!(receiver.open(&latest, AAD).unwrap(), b"stale");
        assert!(matches!(
            receiver.open(&first, AAD),
            Err(DatagramError::Replay)
        ));
    }

    #[test]
    fn direction_keys_are_incompatible() {
        let mut sender =
            DatagramCipher::derive(&MASTER_KEY, &SESSION_SALT, Direction::ClientToHost).unwrap();
        let mut wrong_receiver =
            DatagramCipher::derive(&MASTER_KEY, &SESSION_SALT, Direction::HostToClient).unwrap();
        let datagram = sender.seal(b"cross", AAD).unwrap();
        assert!(matches!(
            wrong_receiver.open(&datagram, AAD),
            Err(DatagramError::WrongDirection)
        ));
    }

    #[test]
    fn complete_datagram_authenticates_header() {
        let header = PacketHeader::new(PacketType::FrameChunk, 7, 9, 1);
        let (mut sender, mut receiver) = matching_pair();
        let mut datagram = sender.seal_datagram(&header, b"payload").unwrap();
        let (opened_header, opened_payload) = receiver.open_datagram(&datagram).unwrap();
        assert_eq!(opened_header, header);
        assert_eq!(opened_payload, b"payload");

        let (mut sender, mut receiver) = matching_pair();
        datagram = sender.seal_datagram(&header, b"payload").unwrap();
        datagram[3] ^= 1;
        assert!(receiver.open_datagram(&datagram).is_err());
        assert_eq!(header.encode().unwrap().len(), PacketHeader::SIZE);
    }

    pub(crate) use crate::test_alloc::allocations;

    #[test]
    fn seal_uses_one_allocation() {
        let (mut sender, _) = matching_pair();
        let (sealed, count) = allocations(|| sender.seal(b"payload", AAD).unwrap());
        assert_eq!(sealed.len(), 7 + NONCE_SIZE + TAG_SIZE);
        assert_eq!(count, 1, "seal must allocate only its returned buffer");
    }

    #[test]
    fn media_decode_rejects_oversized_video_before_copy() {
        use ferryx_rd_proto::{CodecError, FrameChunk, MAX_VIDEO_CHUNK_BYTES};

        let input = vec![0; FrameChunk::HEADER_SIZE + MAX_VIDEO_CHUNK_BYTES + 1];
        let (result, count) = allocations(|| FrameChunk::decode(&input));
        assert!(matches!(
            result,
            Err(CodecError::LengthLimit { field: "video chunk data", actual, max })
                if actual == MAX_VIDEO_CHUNK_BYTES + 1 && max == MAX_VIDEO_CHUNK_BYTES
        ));
        assert_eq!(
            FrameChunk::decode(&input[..input.len() - 1])
                .unwrap()
                .data
                .len(),
            MAX_VIDEO_CHUNK_BYTES
        );
        assert!(FrameChunk::decode(&input[..FrameChunk::HEADER_SIZE])
            .unwrap()
            .data
            .is_empty());
        eprintln!("rejected oversized video allocations: {count}");
        assert_eq!(count, 0, "invalid video must be rejected before ownership");
    }

    #[test]
    fn media_decode_rejects_oversized_audio_before_copy() {
        use ferryx_rd_proto::{
            AudioFragment, AudioFragmentHeader, CodecError, MAX_AUDIO_FRAGMENT_BYTES,
        };

        let mut input = vec![0; AudioFragmentHeader::SIZE + MAX_AUDIO_FRAGMENT_BYTES + 1];
        input[6..8].copy_from_slice(&1_u16.to_le_bytes());
        let (result, count) = allocations(|| AudioFragment::decode(&input));
        assert!(matches!(
            result,
            Err(CodecError::LengthLimit { field: "audio fragment data", actual, max })
                if actual == MAX_AUDIO_FRAGMENT_BYTES + 1 && max == MAX_AUDIO_FRAGMENT_BYTES
        ));
        assert_eq!(
            AudioFragment::decode(&input[..input.len() - 1])
                .unwrap()
                .data
                .len(),
            MAX_AUDIO_FRAGMENT_BYTES
        );
        assert!(AudioFragment::decode(&input[..AudioFragmentHeader::SIZE])
            .unwrap()
            .data
            .is_empty());
        eprintln!("rejected oversized audio allocations: {count}");
        assert_eq!(count, 0, "invalid audio must be rejected before ownership");
    }

    #[test]
    fn media_decode_rejects_invalid_audio_header_before_copy() {
        use ferryx_rd_proto::{
            AudioFragment, AudioFragmentHeader, CodecError, MAX_AUDIO_FRAGMENT_BYTES,
        };

        let input = vec![0; AudioFragmentHeader::SIZE + MAX_AUDIO_FRAGMENT_BYTES + 1];
        let (result, count) = allocations(|| AudioFragment::decode(&input));
        assert!(matches!(
            result,
            Err(CodecError::InvalidValue {
                field: "audio fragment count",
                value: 0,
            })
        ));
        eprintln!("rejected invalid audio header allocations: {count}");
        assert_eq!(count, 0, "invalid header must be rejected before ownership");
    }

    #[test]
    fn seal_datagram_uses_two_allocations() {
        let (mut sender, _) = matching_pair();
        let header = PacketHeader::new(PacketType::Ping, 1, 100, 0);
        let (_, count) = allocations(|| sender.seal_datagram(&header, b"hello world").unwrap());
        assert_eq!(
            count, 2,
            "header codec plus final datagram; no intermediate AEAD buffer"
        );
    }

    #[test]
    fn exact_wire_bytes_match_baseline() {
        let (mut sender, _) = matching_pair();
        let header = PacketHeader::new(PacketType::Ping, 1, 100, 0);
        let datagram = sender.seal_datagram(&header, b"hello world").unwrap();
        assert_eq!(
            datagram,
            [
                0x1d, 0xec, 0x07, 0x01, 0, 0, 0, 0x64, 0, 0, 0, 0, 0xe2, 0x6c, 0x23, 0xc1, 0, 0, 0,
                0, 0, 0, 0, 1, 0x5a, 0xe8, 0xb6, 0x27, 0x27, 0x08, 0x4e, 0x4f, 0xa9, 0xf8, 0xf0,
                0x50, 0x29, 0xbc, 0x09, 0x7b, 0x7a, 0x51, 0x0d, 0xea, 0xb1, 0x34, 0xba, 0xa7, 0x87,
                0x2a, 0xdb,
            ]
        );
    }

    #[test]
    fn reusable_buffer_has_zero_allocations_and_preserves_prefix() {
        let (mut sender, mut receiver) = matching_pair();
        let mut output = Vec::with_capacity(2048);
        for payload in [b"payload".as_slice(), b"", b"short"] {
            output.clear();
            output.extend_from_slice(AAD);
            let (_, count) = allocations(|| sender.seal_into(payload, AAD, &mut output).unwrap());
            assert_eq!(count, 0);
            assert_eq!(&output[..AAD.len()], AAD);
            assert_eq!(receiver.open(&output[AAD.len()..], AAD).unwrap(), payload);
        }
    }

    #[test]
    fn every_wire_byte_is_authenticated_without_poisoning_replay() {
        let header = PacketHeader::new(PacketType::Ping, 1, 100, 0);
        let (mut sender, _) = matching_pair();
        let valid = sender.seal_datagram(&header, b"hello world").unwrap();
        for index in 0..valid.len() {
            let (_, mut receiver) = matching_pair();
            let mut tampered = valid.clone();
            tampered[index] ^= 1;
            let result = receiver.open_datagram(&tampered);
            match index {
                0..=1 => assert!(matches!(result, Err(DatagramError::InvalidHeader(_)))),
                12..=15 => assert!(matches!(result, Err(DatagramError::WrongDirection))),
                _ => assert!(matches!(result, Err(DatagramError::Authentication))),
            }
            assert_eq!(
                receiver.open_datagram(&valid).unwrap(),
                (header, b"hello world".to_vec())
            );
            assert!(matches!(
                receiver.open_datagram(&valid),
                Err(DatagramError::Replay)
            ));
        }
    }

    #[test]
    fn both_directions_match_legacy_aead_for_multiple_counters() {
        for direction in [Direction::ClientToHost, Direction::HostToClient] {
            let mut sender = DatagramCipher::derive(&MASTER_KEY, &SESSION_SALT, direction).unwrap();
            for counter in 1_u64..=3 {
                let payload = [0x73; 1200];
                let mut nonce = [0; NONCE_SIZE];
                nonce[..4].copy_from_slice(&sender.nonce_prefix);
                nonce[4..].copy_from_slice(&counter.to_be_bytes());
                let expected = sender
                    .cipher
                    .encrypt(
                        Nonce::from_slice(&nonce),
                        Payload {
                            msg: &payload,
                            aad: AAD,
                        },
                    )
                    .unwrap();
                let sealed = sender.seal(&payload, AAD).unwrap();
                assert_eq!(&sealed[..NONCE_SIZE], &nonce);
                assert_eq!(&sealed[NONCE_SIZE..], expected);
            }
        }
    }

    #[test]
    fn replay_accepts_reordered_window_edge_but_rejects_stale() {
        let (mut sender, mut receiver) = matching_pair();
        let stale = sender.seal(b"stale", AAD).unwrap();
        let edge = sender.seal(b"edge", AAD).unwrap();
        sender.send_counter = REPLAY_WINDOW_SIZE;
        let latest = sender.seal(b"latest", AAD).unwrap();
        assert_eq!(receiver.open(&latest, AAD).unwrap(), b"latest");
        assert_eq!(receiver.open(&edge, AAD).unwrap(), b"edge");
        assert!(matches!(
            receiver.open(&stale, AAD),
            Err(DatagramError::Replay)
        ));
        assert!(matches!(
            receiver.open(&edge, AAD),
            Err(DatagramError::Replay)
        ));
    }

    #[test]
    fn replay_prunes_only_when_window_block_advances() {
        let mut window = ReplayWindow::default();
        for counter in 0..REPLAY_WINDOW_SIZE {
            window.record(counter);
        }
        window.prune_visits = 0;
        for counter in REPLAY_WINDOW_SIZE..2 * REPLAY_WINDOW_SIZE {
            assert!(window.can_accept(counter));
            window.record(counter);
            assert!(!window.can_accept(counter));
        }
        // The window spans at most 65 blocks and advances 64 block
        // boundaries over these 4096 packets. Other packets cannot
        // evict a block and must not rescan the map.
        let maximum_visits = 65 * 64;
        eprintln!(
            "replay prune visits for 4096 packets: {}; maximum: {maximum_visits}",
            window.prune_visits
        );
        assert!(
            window.prune_visits <= maximum_visits,
            "replay pruning rescans blocks without an eviction boundary"
        );
        assert!(window.blocks.len() <= 65);
        assert!(!window.can_accept(0));
    }

    #[test]
    fn exhausted_counter_preserves_output_and_never_wraps() {
        let (mut sender, _) = matching_pair();
        sender.send_counter = u64::MAX - 1;
        let final_packet = sender.seal(b"last", AAD).unwrap();
        assert_eq!(&final_packet[4..NONCE_SIZE], &u64::MAX.to_be_bytes());
        let mut output = b"prefix".to_vec();
        for _ in 0..2 {
            assert!(matches!(
                sender.seal_into(b"no", AAD, &mut output),
                Err(DatagramError::CounterExhausted)
            ));
            assert_eq!(output, b"prefix");
            assert_eq!(sender.send_counter, u64::MAX);
        }
    }
}

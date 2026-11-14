//! HSFZ ("High-Speed-Fahrzeugzugang"), the diagnostic-over-Ethernet framing
//! BMW F/G-series gateways speak when an ENET cable is attached.
//!
//! Every frame is a 6-byte header (u32 BE payload length, u16 BE control word)
//! followed by the payload. Diagnostic payloads are `[source][target][UDS...]`.
//!
//! Control-word values and the alive-check reply come from public
//! reverse-engineering notes and still need confirming against a capture from
//! a real car.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::time::{Instant, timeout, timeout_at};

pub const DIAG_PORT: u16 = 6801;
pub const DISCOVERY_PORT: u16 = 6811;
/// Tester address E-Sys and ISTA use over ENET.
pub const TESTER_ADDR: u8 = 0xF4;

const HEADER_LEN: usize = 6;
/// Guards against a corrupt length field; real UDS messages are far smaller.
const MAX_PAYLOAD: usize = 64 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const ACK_TIMEOUT: Duration = Duration::from_secs(2);

pub mod control {
    pub const DIAG: u16 = 0x0001;
    pub const ACK: u16 = 0x0002;
    pub const VEHICLE_IDENT: u16 = 0x0011;
    pub const ALIVE_CHECK: u16 = 0x0012;
    /// 0x0040..=0x00FF are gateway error replies (bad tester address,
    /// control word, format, destination, size, or diag app not ready).
    pub const FIRST_ERROR: u16 = 0x0040;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O: {0}")]
    Io(#[from] io::Error),
    #[error("timed out waiting for {0}")]
    Timeout(&'static str),
    #[error("gateway rejected the message (control word 0x{0:04X})")]
    Rejected(u16),
    #[error("malformed frame: {0}")]
    Malformed(&'static str),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub control: u16,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn diag(source: u8, target: u8, data: &[u8]) -> Self {
        let mut payload = Vec::with_capacity(2 + data.len());
        payload.extend_from_slice(&[source, target]);
        payload.extend_from_slice(data);
        Self { control: control::DIAG, payload }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + self.payload.len());
        out.extend_from_slice(&(self.payload.len() as u32).to_be_bytes());
        out.extend_from_slice(&self.control.to_be_bytes());
        out.extend_from_slice(&self.payload);
        out
    }

    /// Decodes one frame from the front of `buf`, returning it with the number
    /// of bytes consumed, or `None` if `buf` doesn't hold a whole frame yet.
    pub fn decode(buf: &[u8]) -> Result<Option<(Frame, usize)>> {
        if buf.len() < HEADER_LEN {
            return Ok(None);
        }
        let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        if len > MAX_PAYLOAD {
            return Err(Error::Malformed("payload length too large"));
        }
        let total = HEADER_LEN + len;
        if buf.len() < total {
            return Ok(None);
        }
        let control = u16::from_be_bytes([buf[4], buf[5]]);
        let payload = buf[HEADER_LEN..total].to_vec();
        Ok(Some((Frame { control, payload }, total)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagMessage {
    pub source: u8,
    pub target: u8,
    pub data: Vec<u8>,
}

impl DiagMessage {
    fn from_payload(payload: &[u8]) -> Result<Self> {
        match payload {
            [source, target, data @ ..] => Ok(Self {
                source: *source,
                target: *target,
                data: data.to_vec(),
            }),
            _ => Err(Error::Malformed("diagnostic payload shorter than 2 bytes")),
        }
    }
}

/// A TCP session with the vehicle gateway.
pub struct Connection {
    stream: TcpStream,
    tester: u8,
    rx: Vec<u8>,
    /// Diagnostic messages that arrived while we were waiting for an ack.
    pending: VecDeque<DiagMessage>,
}

impl Connection {
    pub async fn connect(ip: IpAddr) -> Result<Self> {
        let addr = SocketAddr::new(ip, DIAG_PORT);
        let stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
            .await
            .map_err(|_| Error::Timeout("TCP connect"))??;
        stream.set_nodelay(true)?;
        Ok(Self {
            stream,
            tester: TESTER_ADDR,
            rx: Vec::new(),
            pending: VecDeque::new(),
        })
    }

    /// Sends a UDS request to `target` and waits for the gateway's ack.
    pub async fn send(&mut self, target: u8, data: &[u8]) -> Result<()> {
        tracing::debug!(target = format!("{target:02X}"), data = hex(data), "tx");
        let frame = Frame::diag(self.tester, target, data);
        self.stream.write_all(&frame.encode()).await?;

        let deadline = Instant::now() + ACK_TIMEOUT;
        loop {
            let frame = self.read_frame_until(deadline, "gateway ack").await?;
            match frame.control {
                control::ACK => return Ok(()),
                control::DIAG => self
                    .pending
                    .push_back(DiagMessage::from_payload(&frame.payload)?),
                control::ALIVE_CHECK => self.answer_alive_check().await?,
                c if c >= control::FIRST_ERROR => return Err(Error::Rejected(c)),
                _ => {}
            }
        }
    }

    /// Returns the next diagnostic message, or `None` if none arrives within `wait`.
    pub async fn recv(&mut self, wait: Duration) -> Result<Option<DiagMessage>> {
        if let Some(msg) = self.pending.pop_front() {
            return Ok(Some(msg));
        }
        let deadline = Instant::now() + wait;
        loop {
            let frame = match self.read_frame_until(deadline, "diagnostic response").await {
                Ok(frame) => frame,
                Err(Error::Timeout(_)) => return Ok(None),
                Err(e) => return Err(e),
            };
            match frame.control {
                control::DIAG => {
                    let msg = DiagMessage::from_payload(&frame.payload)?;
                    tracing::debug!(source = format!("{:02X}", msg.source), data = hex(&msg.data), "rx");
                    return Ok(Some(msg));
                }
                control::ALIVE_CHECK => self.answer_alive_check().await?,
                c if c >= control::FIRST_ERROR => return Err(Error::Rejected(c)),
                _ => {}
            }
        }
    }

    async fn answer_alive_check(&mut self) -> Result<()> {
        // Unconfirmed format: reply with our tester address so the gateway
        // keeps the session open.
        let reply = Frame {
            control: control::ALIVE_CHECK,
            payload: vec![0x00, self.tester],
        };
        self.stream.write_all(&reply.encode()).await?;
        Ok(())
    }

    async fn read_frame_until(&mut self, deadline: Instant, what: &'static str) -> Result<Frame> {
        timeout_at(deadline, self.read_frame())
            .await
            .map_err(|_| Error::Timeout(what))?
    }

    /// Cancel-safe: bytes are only buffered after `read` completes, so a
    /// timeout mid-read never loses part of a frame.
    async fn read_frame(&mut self) -> Result<Frame> {
        loop {
            if let Some((frame, used)) = Frame::decode(&self.rx)? {
                self.rx.drain(..used);
                return Ok(frame);
            }
            let mut chunk = [0u8; 4096];
            let n = self.stream.read(&mut chunk).await?;
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "gateway closed the connection").into());
            }
            self.rx.extend_from_slice(&chunk[..n]);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vehicle {
    pub ip: IpAddr,
    pub vin: Option<String>,
    /// The raw identification string the gateway sent back.
    pub ident: String,
}

/// Broadcasts a vehicle-identification request and collects replies for `wait`.
pub async fn discover(wait: Duration) -> Result<Vec<Vehicle>> {
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
    sock.set_broadcast(true)?;
    let probe = Frame {
        control: control::VEHICLE_IDENT,
        payload: Vec::new(),
    }
    .encode();
    // With no DHCP server on the link, car and laptop both self-assign
    // 169.254/16 addresses, so probe that subnet as well as the limited broadcast.
    for dst in [Ipv4Addr::BROADCAST, Ipv4Addr::new(169, 254, 255, 255)] {
        if let Err(e) = sock.send_to(&probe, (dst, DISCOVERY_PORT)).await {
            tracing::debug!(%dst, error = %e, "discovery probe failed");
        }
    }

    let mut found: Vec<Vehicle> = Vec::new();
    let deadline = Instant::now() + wait;
    let mut buf = [0u8; 1024];
    loop {
        let (n, from) = match timeout_at(deadline, sock.recv_from(&mut buf)).await {
            Err(_) => break,
            // Windows surfaces ICMP port-unreachable as ConnectionReset on UDP.
            Ok(Err(e)) if e.kind() == io::ErrorKind::ConnectionReset => continue,
            Ok(r) => r?,
        };
        let Ok(Some((frame, _))) = Frame::decode(&buf[..n]) else {
            continue;
        };
        if frame.control != control::VEHICLE_IDENT || found.iter().any(|v| v.ip == from.ip()) {
            continue;
        }
        let ident = String::from_utf8_lossy(&frame.payload).into_owned();
        tracing::debug!(ip = %from.ip(), %ident, "vehicle ident");
        found.push(Vehicle {
            ip: from.ip(),
            vin: parse_vin(&ident),
            ident,
        });
    }
    Ok(found)
}

/// The ident string carries the VIN after a `BMWVIN` marker.
fn parse_vin(ident: &str) -> Option<String> {
    let rest = &ident[ident.find("BMWVIN")? + "BMWVIN".len()..];
    let vin: String = rest.chars().take(17).collect();
    (vin.len() == 17 && vin.chars().all(|c| c.is_ascii_alphanumeric())).then_some(vin)
}

pub fn hex(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len() * 3);
    for (i, b) in data.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        let _ = write!(s, "{b:02X}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_frame_layout() {
        let bytes = Frame::diag(0xF4, 0x12, &[0x19, 0x02, 0xFF]).encode();
        assert_eq!(bytes, [0, 0, 0, 5, 0, 1, 0xF4, 0x12, 0x19, 0x02, 0xFF]);
    }

    #[test]
    fn decode_round_trip() {
        let frame = Frame::diag(0x12, 0xF4, &[0x59, 0x02, 0xFF]);
        let bytes = frame.encode();
        assert_eq!(Frame::decode(&bytes).unwrap(), Some((frame, bytes.len())));
    }

    #[test]
    fn decode_waits_for_whole_frame() {
        let bytes = Frame::diag(0x12, 0xF4, &[0x59, 0x02]).encode();
        assert_eq!(Frame::decode(&bytes[..3]).unwrap(), None);
        assert_eq!(Frame::decode(&bytes[..bytes.len() - 1]).unwrap(), None);
    }

    #[test]
    fn decode_leaves_trailing_bytes() {
        let mut bytes = Frame::diag(0x12, 0xF4, &[0x59]).encode();
        let first_len = bytes.len();
        bytes.extend(Frame::diag(0x18, 0xF4, &[0x59]).encode());
        let (_, used) = Frame::decode(&bytes).unwrap().unwrap();
        assert_eq!(used, first_len);
    }

    #[test]
    fn decode_rejects_huge_length() {
        let bytes = [0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x01];
        assert!(matches!(Frame::decode(&bytes), Err(Error::Malformed(_))));
    }

    #[test]
    fn vin_after_marker() {
        let ident = "DIAGADR10BMWMAC001A37000000BMWVIN5UXCR6C05KLL00000";
        assert_eq!(parse_vin(ident).as_deref(), Some("5UXCR6C05KLL00000"));
        assert_eq!(parse_vin("DIAGADR10"), None);
        assert_eq!(parse_vin("BMWVINSHORT"), None);
    }
}

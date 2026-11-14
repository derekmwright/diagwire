//! High-level diagnostic operations shared by the CLI and (next) the MCP server.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

pub use diagwire_hsfz as hsfz;
pub use diagwire_uds as uds;

/// Functional address the F/G-series gateway fans out to every ECU.
pub const FUNCTIONAL_ALL: u8 = 0xDF;

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);
/// How long to wait after each "response pending" (UDS P2* time).
const PENDING_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Transport(#[from] hsfz::Error),
    #[error(transparent)]
    Decode(#[from] uds::DecodeError),
    #[error("ECU 0x{ecu:02X} refused service 0x{service:02X}: {} (NRC 0x{nrc:02X})", uds::nrc_name(*nrc))]
    Negative { ecu: u8, service: u8, nrc: u8 },
    #[error("no response from ECU 0x{0:02X}")]
    NoResponse(u8),
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Session {
    conn: hsfz::Connection,
}

impl Session {
    pub async fn connect(ip: IpAddr) -> Result<Self> {
        Ok(Self {
            conn: hsfz::Connection::connect(ip).await?,
        })
    }

    /// Sends `request` to one ECU and returns the positive-response body,
    /// waiting out any "response pending" replies.
    pub async fn request(&mut self, ecu: u8, request: &[u8]) -> Result<Vec<u8>> {
        let service = *request.first().expect("UDS request must not be empty");
        self.conn.send(ecu, request).await?;
        let mut wait = RESPONSE_TIMEOUT;
        loop {
            let Some(msg) = self.conn.recv(wait).await? else {
                return Err(Error::NoResponse(ecu));
            };
            if msg.source != ecu {
                continue; // a straggler from an earlier functional request
            }
            match uds::classify(service, &msg.data)? {
                uds::Reply::Positive(body) => return Ok(body.to_vec()),
                uds::Reply::Pending => wait = PENDING_TIMEOUT,
                uds::Reply::Negative { nrc, .. } => return Err(Error::Negative { ecu, service, nrc }),
            }
        }
    }

    pub async fn read_dtcs(&mut self, ecu: u8, mask: u8) -> Result<uds::DtcReport> {
        let body = self.request(ecu, &uds::read_dtc_by_status_mask(mask)).await?;
        Ok(uds::parse_dtc_report(&body)?)
    }

    /// Asks every ECU for its fault memory at once and collects whoever
    /// answers within `window`. ECUs that stay silent simply don't appear.
    pub async fn scan_dtcs(&mut self, mask: u8, window: Duration) -> Result<BTreeMap<u8, Result<uds::DtcReport>>> {
        let request = uds::read_dtc_by_status_mask(mask);
        let service = request[0];
        self.conn.send(FUNCTIONAL_ALL, &request).await?;

        let deadline = Instant::now() + window;
        let mut results = BTreeMap::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            let Some(msg) = self.conn.recv(left).await? else {
                break;
            };
            let result = match uds::classify(service, &msg.data) {
                Ok(uds::Reply::Positive(body)) => uds::parse_dtc_report(body).map_err(Error::from),
                Ok(uds::Reply::Pending) => continue,
                Ok(uds::Reply::Negative { nrc, .. }) => Err(Error::Negative {
                    ecu: msg.source,
                    service,
                    nrc,
                }),
                Err(e) => Err(e.into()),
            };
            results.insert(msg.source, result);
        }
        Ok(results)
    }
}

/// Common F/G-series diagnostic addresses. Starting points only, to be
/// confirmed against a real scan.
pub fn ecu_name(addr: u8) -> Option<&'static str> {
    Some(match addr {
        0x10 => "Gateway",
        0x12 => "DME (engine)",
        0x18 => "EGS (transmission)",
        0x29 => "DSC (stability control)",
        _ => return None,
    })
}

//! High-level diagnostic operations shared by the CLI and (next) the MCP server.

use std::net::IpAddr;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::time::Instant;

pub use diagwire_hsfz as hsfz;
pub use diagwire_uds as uds;

pub mod redact;

#[cfg(test)]
mod tests;

/// The time limits of one operation (DR-0001 D10). Every value is chosen or
/// carried over from the spike; none is checked against an authority yet.
#[derive(Debug, Clone, Copy)]
struct Timing {
    /// From the start of the operation: the request written and acked.
    ack: Duration,
    /// Wait for the first response.
    response: Duration,
    /// Wait after each "response pending" (UDS P2*).
    pending: Duration,
    /// The whole operation. Fixed when it starts; nothing extends it.
    operation: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            ack: Duration::from_secs(2),
            response: Duration::from_secs(2),
            pending: Duration::from_secs(5),
            operation: Duration::from_secs(30),
        }
    }
}

/// How one single-ECU operation ended (DR-0001 D10). Only `Complete` carries
/// a result: every other outcome means the answer is unknown, never "nothing
/// to report".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome<T> {
    Complete(T),
    /// The ECU refused the request with this negative response code.
    Negative(u8),
    /// The ECU said "response pending" and no final reply arrived in time.
    PendingExpired,
    /// Nothing that counts as a response arrived in time.
    NoResponse,
    /// Something arrived that could not be validated. Nothing is salvaged
    /// from it.
    Malformed,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Transport(#[from] hsfz::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// One connection to a vehicle. After any outcome other than `Complete` or
/// `Negative` the connection is closed and the session is finished: every
/// later call fails with [`hsfz::Error::Disposed`].
pub struct Session<S = TcpStream> {
    conn: hsfz::Connection<S>,
    timing: Timing,
}

impl Session<TcpStream> {
    pub async fn connect(ip: IpAddr) -> Result<Self> {
        Ok(Self {
            conn: hsfz::Connection::connect(ip).await?,
            timing: Timing::default(),
        })
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> Session<S> {
    /// Runs a session over an already-open stream. Tests pass a fake here
    /// (DR-0001 D7).
    pub fn from_stream(stream: S) -> Self {
        Self {
            conn: hsfz::Connection::from_stream(stream),
            timing: Timing::default(),
        }
    }

    /// True once the connection has been closed after a failure.
    pub fn is_disposed(&self) -> bool {
        self.conn.is_disposed()
    }

    /// Reads the fault memory of one ECU.
    pub async fn read_dtcs(&mut self, ecu: u8, mask: u8) -> Result<Outcome<uds::DtcReport>> {
        self.transact(ecu, &uds::read_dtc_by_status_mask(mask), uds::parse_dtc_report)
            .await
    }

    /// One transaction with one ECU. `decode` validates the body of the
    /// positive response, and the connection is kept only if it accepts.
    async fn transact<T>(
        &mut self,
        ecu: u8,
        request: &[u8],
        decode: impl Fn(&[u8]) -> std::result::Result<T, uds::DecodeError>,
    ) -> Result<Outcome<T>> {
        let service = request[0];
        let start = Instant::now();
        let deadline = start + self.timing.operation;

        let ack_by = deadline.min(start + self.timing.ack);
        let mut tx = match self.conn.begin(ecu, request, ack_by).await {
            Ok(tx) => tx,
            Err(e) => return failed(ecu, e, false),
        };

        let mut pending = false;
        let mut wait = self.timing.response;
        loop {
            let by = deadline.min(Instant::now() + wait);
            let data = match tx.response(by).await {
                Ok(data) => data,
                Err(e) => return failed(ecu, e, pending),
            };
            // Returning without `finish` drops the transaction, which closes
            // the connection.
            match uds::classify(service, &data) {
                Ok(uds::Reply::Pending) => {
                    pending = true;
                    wait = self.timing.pending;
                }
                Ok(uds::Reply::Negative { nrc, .. }) => {
                    tx.finish();
                    return Ok(Outcome::Negative(nrc));
                }
                Ok(uds::Reply::Positive(body)) => match decode(body) {
                    Ok(value) => {
                        tx.finish();
                        return Ok(Outcome::Complete(value));
                    }
                    Err(e) => return Ok(malformed(ecu, &e)),
                },
                Err(e) => return Ok(malformed(ecu, &e)),
            }
        }
    }
}

/// Maps a transport failure onto an outcome. The transport has already
/// closed the connection.
fn failed<T>(ecu: u8, error: hsfz::Error, pending: bool) -> Result<Outcome<T>> {
    match error {
        hsfz::Error::Timeout(what) => {
            tracing::warn!(ecu = format!("{ecu:02X}"), "timed out waiting for {what}");
            Ok(if pending {
                Outcome::PendingExpired
            } else {
                Outcome::NoResponse
            })
        }
        hsfz::Error::Malformed(why) => Ok(malformed(ecu, &why)),
        other => Err(other.into()),
    }
}

fn malformed<T>(ecu: u8, why: &dyn std::fmt::Display) -> Outcome<T> {
    tracing::warn!(ecu = format!("{ecu:02X}"), "response could not be validated: {why}");
    Outcome::Malformed
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

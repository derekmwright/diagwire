//! UDS (ISO 14229) request builders and response parsers. Pure byte handling
//! with no I/O, so every transport can share it.

use std::fmt;

pub mod sid {
    pub const CLEAR_DTC: u8 = 0x14;
    pub const READ_DTC_INFO: u8 = 0x19;
    pub const READ_DATA_BY_ID: u8 = 0x22;
    pub const NEGATIVE_RESPONSE: u8 = 0x7F;
}

const POSITIVE_OFFSET: u8 = 0x40;
const REPORT_DTC_BY_STATUS_MASK: u8 = 0x02;
pub const NRC_RESPONSE_PENDING: u8 = 0x78;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("empty response")]
    Empty,
    #[error("response truncated")]
    Truncated,
    #[error("unexpected response SID 0x{got:02X} (expected 0x{expected:02X})")]
    UnexpectedService { expected: u8, got: u8 },
    #[error("unexpected subfunction 0x{0:02X}")]
    UnexpectedSubfunction(u8),
}

pub fn read_dtc_by_status_mask(mask: u8) -> Vec<u8> {
    vec![sid::READ_DTC_INFO, REPORT_DTC_BY_STATUS_MASK, mask]
}

/// Clears every DTC group on the target ECU.
pub fn clear_all_dtcs() -> Vec<u8> {
    vec![sid::CLEAR_DTC, 0xFF, 0xFF, 0xFF]
}

pub fn read_data_by_id(did: u16) -> Vec<u8> {
    let [hi, lo] = did.to_be_bytes();
    vec![sid::READ_DATA_BY_ID, hi, lo]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply<'a> {
    /// The response body after the positive-response SID.
    Positive(&'a [u8]),
    /// NRC 0x78: the ECU is still working and the real answer will follow.
    Pending,
    Negative { service: u8, nrc: u8 },
}

/// Classifies a response to a request for `service`.
pub fn classify(service: u8, data: &[u8]) -> Result<Reply<'_>, DecodeError> {
    let positive = service.wrapping_add(POSITIVE_OFFSET);
    match data {
        [] => Err(DecodeError::Empty),
        [sid::NEGATIVE_RESPONSE, svc, nrc, ..] if *svc == service => Ok(match *nrc {
            NRC_RESPONSE_PENDING => Reply::Pending,
            nrc => Reply::Negative { service, nrc },
        }),
        [sid::NEGATIVE_RESPONSE, svc, _, ..] => Err(DecodeError::UnexpectedService {
            expected: service,
            got: *svc,
        }),
        [sid::NEGATIVE_RESPONSE, ..] => Err(DecodeError::Truncated),
        [first, body @ ..] if *first == positive => Ok(Reply::Positive(body)),
        [first, ..] => Err(DecodeError::UnexpectedService {
            expected: positive,
            got: *first,
        }),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DtcStatus(pub u8);

impl DtcStatus {
    const FLAGS: [&'static str; 8] = [
        "testFailed",
        "testFailedThisOperationCycle",
        "pending",
        "confirmed",
        "testNotCompletedSinceLastClear",
        "testFailedSinceLastClear",
        "testNotCompletedThisOperationCycle",
        "warningIndicatorRequested",
    ];

    /// The fault is failing right now.
    pub fn test_failed(self) -> bool {
        self.0 & 0x01 != 0
    }

    pub fn confirmed(self) -> bool {
        self.0 & 0x08 != 0
    }

    pub fn warning_indicator(self) -> bool {
        self.0 & 0x80 != 0
    }

    pub fn flags(self) -> impl Iterator<Item = &'static str> {
        Self::FLAGS
            .into_iter()
            .enumerate()
            .filter(move |(bit, _)| self.0 & (1 << bit) != 0)
            .map(|(_, name)| name)
    }
}

impl fmt::Debug for DtcStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:02X} [", self.0)?;
        for (i, name) in self.flags().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            f.write_str(name)?;
        }
        f.write_str("]")
    }
}

/// A 3-byte DTC as BMW F/G-series ECUs report it, e.g. `130A04`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dtc {
    pub code: u32,
    pub status: DtcStatus,
}

impl fmt::Display for Dtc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:06X}", self.code)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DtcReport {
    /// Which status bits this ECU supports.
    pub availability_mask: u8,
    pub dtcs: Vec<Dtc>,
}

/// Parses the body of a positive ReadDTCInformation / reportDTCByStatusMask
/// response (everything after the 0x59 SID).
pub fn parse_dtc_report(body: &[u8]) -> Result<DtcReport, DecodeError> {
    let [sub, availability_mask, records @ ..] = body else {
        return Err(DecodeError::Truncated);
    };
    if *sub != REPORT_DTC_BY_STATUS_MASK {
        return Err(DecodeError::UnexpectedSubfunction(*sub));
    }
    let (records, rest) = records.as_chunks::<4>();
    if !rest.is_empty() {
        return Err(DecodeError::Truncated);
    }
    let dtcs = records
        .iter()
        .map(|&[hi, mid, lo, status]| Dtc {
            code: u32::from_be_bytes([0, hi, mid, lo]),
            status: DtcStatus(status),
        })
        .collect();
    Ok(DtcReport {
        availability_mask: *availability_mask,
        dtcs,
    })
}

pub fn nrc_name(nrc: u8) -> &'static str {
    match nrc {
        0x10 => "general reject",
        0x11 => "service not supported",
        0x12 => "subfunction not supported",
        0x13 => "incorrect message length or format",
        0x14 => "response too long",
        0x21 => "busy, repeat request",
        0x22 => "conditions not correct",
        0x24 => "request sequence error",
        0x31 => "request out of range",
        0x33 => "security access denied",
        0x35 => "invalid key",
        0x36 => "exceeded number of attempts",
        0x37 => "required time delay not expired",
        0x72 => "general programming failure",
        0x78 => "response pending",
        0x7E => "subfunction not supported in active session",
        0x7F => "service not supported in active session",
        _ => "unknown NRC",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_bytes() {
        assert_eq!(read_dtc_by_status_mask(0xFF), [0x19, 0x02, 0xFF]);
        assert_eq!(clear_all_dtcs(), [0x14, 0xFF, 0xFF, 0xFF]);
        assert_eq!(read_data_by_id(0xF190), [0x22, 0xF1, 0x90]);
    }

    #[test]
    fn classify_replies() {
        let svc = sid::READ_DTC_INFO;
        assert_eq!(classify(svc, &[0x59, 0x02, 0xFF]), Ok(Reply::Positive(&[0x02, 0xFF])));
        assert_eq!(classify(svc, &[0x7F, 0x19, 0x78]), Ok(Reply::Pending));
        assert_eq!(
            classify(svc, &[0x7F, 0x19, 0x22]),
            Ok(Reply::Negative { service: 0x19, nrc: 0x22 })
        );
        assert_eq!(classify(svc, &[]), Err(DecodeError::Empty));
        assert_eq!(classify(svc, &[0x7F, 0x19]), Err(DecodeError::Truncated));
        assert_eq!(
            classify(svc, &[0x62, 0xF1]),
            Err(DecodeError::UnexpectedService { expected: 0x59, got: 0x62 })
        );
    }

    #[test]
    fn parse_two_dtcs() {
        let body = [0x02, 0xFF, 0x13, 0x0A, 0x04, 0x2F, 0x48, 0x02, 0x00, 0x08];
        let report = parse_dtc_report(&body).unwrap();
        assert_eq!(report.availability_mask, 0xFF);
        assert_eq!(report.dtcs.len(), 2);
        assert_eq!(report.dtcs[0].to_string(), "130A04");
        assert!(report.dtcs[0].status.test_failed());
        assert!(report.dtcs[0].status.confirmed());
        assert_eq!(report.dtcs[1].to_string(), "480200");
        assert!(!report.dtcs[1].status.test_failed());
    }

    #[test]
    fn parse_empty_and_bad_reports() {
        assert_eq!(parse_dtc_report(&[0x02, 0xFF]).unwrap().dtcs, []);
        assert_eq!(parse_dtc_report(&[0x02]), Err(DecodeError::Truncated));
        assert_eq!(parse_dtc_report(&[0x02, 0xFF, 0x13]), Err(DecodeError::Truncated));
        assert_eq!(parse_dtc_report(&[0x0A, 0xFF]), Err(DecodeError::UnexpectedSubfunction(0x0A)));
    }

    #[test]
    fn status_flags() {
        let names: Vec<_> = DtcStatus(0x89).flags().collect();
        assert_eq!(names, ["testFailed", "confirmed", "warningIndicatorRequested"]);
        assert_eq!(format!("{:?}", DtcStatus(0x00)), "0x00 []");
    }
}

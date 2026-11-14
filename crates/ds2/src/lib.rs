//! DS2, the K-line protocol pre-CAN BMW modules speak (the E36 M3's MS41 DME
//! among them). A telegram is `[address][length][data...][checksum]`, where
//! `length` counts the whole telegram and the checksum XORs every byte before it.
//!
//! Framing only for now. The serial transport (9600 baud 8E1, stripping the
//! K-line echo of our own bytes) comes once real traffic has been captured
//! from the car through EDIABAS.

/// First data byte of a response telegram when the module accepted the request.
pub const STATUS_OK: u8 = 0xA0;

const OVERHEAD: usize = 3;
const MAX_DATA: usize = u8::MAX as usize - OVERHEAD;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("{0} data bytes won't fit in one telegram (max {MAX_DATA})")]
    TooLong(usize),
    #[error("length byte {0} is shorter than the minimum telegram")]
    BadLength(u8),
    #[error("checksum mismatch: computed 0x{computed:02X}, telegram says 0x{received:02X}")]
    BadChecksum { computed: u8, received: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Telegram<'a> {
    pub address: u8,
    pub data: &'a [u8],
}

pub fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0, |acc, b| acc ^ b)
}

pub fn encode(address: u8, data: &[u8]) -> Result<Vec<u8>, Error> {
    if data.len() > MAX_DATA {
        return Err(Error::TooLong(data.len()));
    }
    let mut out = Vec::with_capacity(data.len() + OVERHEAD);
    out.push(address);
    out.push((data.len() + OVERHEAD) as u8);
    out.extend_from_slice(data);
    out.push(checksum(&out));
    Ok(out)
}

/// Decodes one telegram from the front of `buf`, returning it with the number
/// of bytes consumed, or `None` if `buf` doesn't hold a whole telegram yet.
pub fn decode(buf: &[u8]) -> Result<Option<(Telegram<'_>, usize)>, Error> {
    let [address, len, ..] = *buf else {
        return Ok(None);
    };
    let total = len as usize;
    if total < OVERHEAD {
        return Err(Error::BadLength(len));
    }
    if buf.len() < total {
        return Ok(None);
    }
    let computed = checksum(&buf[..total - 1]);
    let received = buf[total - 1];
    if computed != received {
        return Err(Error::BadChecksum { computed, received });
    }
    let telegram = Telegram {
        address,
        data: &buf[2..total - 1],
    };
    Ok(Some((telegram, total)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_layout() {
        let t = encode(0x12, &[0x04, 0x01]).unwrap();
        assert_eq!(t, [0x12, 0x05, 0x04, 0x01, 0x12 ^ 0x05 ^ 0x04 ^ 0x01]);
    }

    #[test]
    fn round_trip() {
        let bytes = encode(0x12, &[STATUS_OK, 1, 2, 3]).unwrap();
        let (t, used) = decode(&bytes).unwrap().unwrap();
        assert_eq!(t, Telegram { address: 0x12, data: &[STATUS_OK, 1, 2, 3] });
        assert_eq!(used, bytes.len());
    }

    #[test]
    fn empty_data() {
        let bytes = encode(0x12, &[]).unwrap();
        assert_eq!(bytes.len(), 3);
        assert_eq!(decode(&bytes).unwrap().unwrap().0.data, &[] as &[u8]);
    }

    #[test]
    fn partial_and_corrupt() {
        let mut bytes = encode(0x12, &[0xA0, 0x55]).unwrap();
        assert_eq!(decode(&bytes[..1]).unwrap(), None);
        assert_eq!(decode(&bytes[..bytes.len() - 1]).unwrap(), None);
        *bytes.last_mut().unwrap() ^= 0xFF;
        assert!(matches!(decode(&bytes), Err(Error::BadChecksum { .. })));
        assert_eq!(decode(&[0x12, 0x02]), Err(Error::BadLength(2)));
    }

    #[test]
    fn length_limit() {
        assert!(encode(0x12, &[0; MAX_DATA]).is_ok());
        assert_eq!(encode(0x12, &[0; MAX_DATA + 1]), Err(Error::TooLong(MAX_DATA + 1)));
    }
}

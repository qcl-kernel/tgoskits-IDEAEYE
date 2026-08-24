//! AXNET/1 wire protocol, shared by the StarryOS client and the FreeRTOS
//! RTOS server.
//!
//! Frame layout (all multi-byte fields big-endian, identical to the C
//! implementation in the FreeRTOS project `app/axnet1.h`):
//!
//! ```text
//!   Magic         u16   0xA501
//!   Version       u8    1
//!   MsgType       u8    0x01..=0x05
//!   Flags         u16   reserved (0)
//!   PayloadLen    u32   length of Payload
//!   Sequence      u32   application sequence number
//!   TimestampUs   u64   sender timestamp in microseconds
//!   ErrorCode     u32   0 on success
//!   Payload       u8[PayloadLen]
//!   CRC32         u32   IEEE CRC-32 of everything above this field
//! ```

#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

pub const MAGIC: u16 = 0xA501;
pub const VERSION: u8 = 1;

pub const HEADER_LEN: usize = 26;
pub const CRC_LEN: usize = 4;
pub const MAX_PAYLOAD: usize = 4096;
pub const FRAME_MAX: usize = HEADER_LEN + MAX_PAYLOAD + CRC_LEN;

pub const MSG_CONTROL: u8 = 0x01;
pub const MSG_CONTROL_ACK: u8 = 0x02;
pub const MSG_STATUS: u8 = 0x03;
pub const MSG_ERROR: u8 = 0x04;
pub const MSG_HEARTBEAT: u8 = 0x05;

pub const ERR_OK: u32 = 0x0000;
pub const ERR_UNKNOWN_MSG: u32 = 0x0001;
pub const ERR_BAD_MAGIC: u32 = 0x0002;
pub const ERR_BAD_VERSION: u32 = 0x0003;
pub const ERR_BAD_CRC: u32 = 0x0004;
pub const ERR_PAYLOAD_TOO_LONG: u32 = 0x0005;
pub const ERR_INTERNAL: u32 = 0x1002;

/// A decoded AXNET/1 message. `payload` borrows the frame buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Message<'a> {
    pub msg_type: u8,
    pub flags: u16,
    pub sequence: u32,
    pub timestamp_us: u64,
    pub error_code: u32,
    pub payload: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    TooShort,
    BadMagic,
    BadVersion,
    BadCrc,
    PayloadTooLong,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Total frame length for a message with `payload_len` payload bytes.
pub const fn frame_len(payload_len: usize) -> usize {
    HEADER_LEN + payload_len + CRC_LEN
}

/// IEEE CRC-32 (reflected polynomial 0xEDB88320), as used by the C side.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    crc ^ 0xFFFF_FFFF
}

/// Encode a message into `dst`, which must be at least `frame_len(payload_len)`
/// bytes. Returns the total frame length.
pub fn encode_into(
    dst: &mut [u8],
    msg_type: u8,
    flags: u16,
    sequence: u32,
    timestamp_us: u64,
    error_code: u32,
    payload: &[u8],
) -> usize {
    let flen = frame_len(payload.len());
    debug_assert!(dst.len() >= flen);
    dst[0..2].copy_from_slice(&MAGIC.to_be_bytes());
    dst[2] = VERSION;
    dst[3] = msg_type;
    dst[4..6].copy_from_slice(&flags.to_be_bytes());
    dst[6..10].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    dst[10..14].copy_from_slice(&sequence.to_be_bytes());
    dst[14..22].copy_from_slice(&timestamp_us.to_be_bytes());
    dst[22..26].copy_from_slice(&error_code.to_be_bytes());
    dst[HEADER_LEN..HEADER_LEN + payload.len()].copy_from_slice(payload);
    let crc = crc32(&dst[..HEADER_LEN + payload.len()]);
    dst[HEADER_LEN + payload.len()..flen].copy_from_slice(&crc.to_be_bytes());
    flen
}

/// Convenience wrapper: encode into a freshly allocated `Vec<u8>`.
#[cfg(feature = "alloc")]
pub fn encode(
    msg_type: u8,
    flags: u16,
    sequence: u32,
    timestamp_us: u64,
    error_code: u32,
    payload: &[u8],
) -> alloc::vec::Vec<u8> {
    let mut buf = alloc::vec![0u8; frame_len(payload.len())];
    encode_into(
        &mut buf,
        msg_type,
        flags,
        sequence,
        timestamp_us,
        error_code,
        payload,
    );
    buf
}

/// Validate a complete frame of `flen` bytes held in `frame`. On success the
/// returned [`Message`] borrows `frame` for its payload.
pub fn decode<'a>(frame: &'a [u8], flen: usize) -> Result<Message<'a>, Error> {
    if flen < HEADER_LEN + CRC_LEN {
        return Err(Error::TooShort);
    }
    let payload_len = u32::from_be_bytes(frame[6..10].try_into().unwrap()) as usize;
    if payload_len > MAX_PAYLOAD {
        return Err(Error::PayloadTooLong);
    }
    if flen != frame_len(payload_len) {
        return Err(Error::BadCrc);
    }

    let magic = u16::from_be_bytes(frame[0..2].try_into().unwrap());
    if magic != MAGIC {
        return Err(Error::BadMagic);
    }
    if frame[2] != VERSION {
        return Err(Error::BadVersion);
    }

    let expected = u32::from_be_bytes(frame[HEADER_LEN + payload_len..flen].try_into().unwrap());
    if crc32(&frame[..HEADER_LEN + payload_len]) != expected {
        return Err(Error::BadCrc);
    }

    Ok(Message {
        msg_type: frame[3],
        flags: u16::from_be_bytes(frame[4..6].try_into().unwrap()),
        sequence: u32::from_be_bytes(frame[10..14].try_into().unwrap()),
        timestamp_us: u64::from_be_bytes(frame[14..22].try_into().unwrap()),
        error_code: u32::from_be_bytes(frame[22..26].try_into().unwrap()),
        payload: &frame[HEADER_LEN..HEADER_LEN + payload_len],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_check_value() {
        // "123456789" -> 0xCBF43926 for CRC-32/IEEE.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn roundtrip() {
        let payload = b"START";
        let flen = frame_len(payload.len());
        let mut buf = [0u8; FRAME_MAX];
        let n = encode_into(&mut buf, MSG_CONTROL, 0, 1001, 1234567890, ERR_OK, payload);
        assert_eq!(n, flen);
        let msg = decode(&buf, flen).unwrap();
        assert_eq!(msg.msg_type, MSG_CONTROL);
        assert_eq!(msg.sequence, 1001);
        assert_eq!(msg.timestamp_us, 1234567890);
        assert_eq!(msg.error_code, ERR_OK);
        assert_eq!(msg.payload, payload);
    }

    #[test]
    fn flipped_payload_byte_detected() {
        let flen = frame_len(5);
        let mut buf = [0u8; FRAME_MAX];
        encode_into(&mut buf, MSG_CONTROL, 0, 1, 0, ERR_OK, b"START");
        buf[HEADER_LEN] ^= 0xFF;
        assert_eq!(decode(&buf, flen), Err(Error::BadCrc));
    }

    #[test]
    fn bad_magic_and_version() {
        let flen = frame_len(0);
        let mut buf = [0u8; FRAME_MAX];
        encode_into(&mut buf, MSG_HEARTBEAT, 0, 7, 0, ERR_OK, &[]);
        buf[0] = 0x00;
        assert_eq!(decode(&buf, flen), Err(Error::BadMagic));
        buf[0] = 0xA5;
        buf[1] = 0x01;
        buf[2] = 9;
        assert_eq!(decode(&buf, flen), Err(Error::BadVersion));
    }
}

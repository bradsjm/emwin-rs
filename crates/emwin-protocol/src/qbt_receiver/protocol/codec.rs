//! Stateful codec for the QBT wire format.
//!
//! The decoder turns an arbitrary stream of XOR-obfuscated bytes into typed protocol events. It
//! owns resynchronization, frame classification, checksum validation, and the V2 decompression
//! boundary so higher layers never have to reason about partial wire state.

use crate::qbt_receiver::config::{QbtDecodeConfig, QbtV2CompressionPolicy};
use crate::qbt_receiver::error::QbtProtocolError;
use crate::qbt_receiver::protocol::checksum::verify_checksum;
use crate::qbt_receiver::protocol::compression::{decompress_zlib, has_zlib_header};
use crate::qbt_receiver::protocol::model::{
    QbtAuthMessage, QbtFrameEvent, QbtProtocolVersion, QbtProtocolWarning, QbtSegment,
};
use crate::qbt_receiver::protocol::server_list::parse_server_list_frame;
use bytes::{Bytes, BytesMut};
use regex::Regex;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use time::PrimitiveDateTime;
use time::macros::format_description;

/// Synchronization bytes that mark the start of a frame.
const SYNC_BYTES: &[u8; 6] = b"\0\0\0\0\0\0";

/// Size of the frame header in bytes.
const HEADER_SIZE: usize = 80;

/// Body size for V1 protocol frames (fixed 1024 bytes).
const V1_BODY_SIZE: usize = 1024;

/// Trait for decoders that consume raw QBT wire bytes.
pub trait QbtFrameDecoder {
    /// Feeds one chunk of XOR-obfuscated wire data into the decoder.
    fn feed(&mut self, chunk: &[u8]) -> Result<Vec<QbtFrameEvent>, QbtProtocolError>;

    /// Resets the decoder state.
    fn reset(&mut self);
}

/// Trait for encoders that build outbound QBT wire frames.
pub trait QbtFrameEncoder {
    /// Encodes an authentication message into its wire representation.
    fn encode_auth(&self, auth: &QbtAuthMessage) -> Result<Bytes, QbtProtocolError>;
}

/// Internal state machine for the streaming decoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecoderState {
    /// Looking for sync bytes (six null bytes).
    Resync,
    /// Skipping null padding after sync bytes.
    StartFrame,
    /// Detecting frame type from the first non-null byte.
    FrameType,
    /// Processing server list frame content.
    QbtServerList,
    /// Parsing 80-byte header for data blocks.
    BlockHeader,
    /// Reading body content based on header length.
    BlockBody,
    /// Validating checksum and preparing to emit segment.
    Validate,
}

/// Partially decoded data-block frame held between state-machine steps.
#[derive(Debug)]
struct PendingSegment {
    /// Filename from the /PF header field.
    filename: String,
    /// Block number from the /PN header field.
    block_number: u32,
    /// Total blocks from the /PT header field.
    total_blocks: u32,
    /// Checksum from the /CS header field.
    checksum: u32,
    /// Body length in bytes (1024 for V1, variable for V2).
    length: usize,
    /// Protocol version determined by presence of /DL field.
    version: QbtProtocolVersion,
    /// Body content bytes.
    content: Bytes,
    /// Timestamp parsed from the /FD header field.
    timestamp_utc: SystemTime,
    /// Warnings collected during parsing.
    warnings: Vec<QbtProtocolWarning>,
    /// Whether decompression failed (V2 only).
    decompression_failed: bool,
}

/// Stateful decoder for EMWIN protocol frames.
///
/// The type is intentionally incremental: callers can feed TCP-sized chunks and rely on the
/// decoder to retain unfinished frame state between calls.
#[derive(Debug)]
pub struct QbtProtocolDecoder {
    /// Decoder configuration.
    config: QbtDecodeConfig,
    /// Internal buffer for accumulating partial frames.
    buffer: BytesMut,
    /// Current decoder state.
    state: DecoderState,
    /// Pending segment being assembled (if any).
    pending: Option<PendingSegment>,
}

impl Default for QbtProtocolDecoder {
    fn default() -> Self {
        Self::new(QbtDecodeConfig::default())
    }
}

impl QbtProtocolDecoder {
    /// Creates a decoder with the supplied checksum and compression policy.
    pub fn new(config: QbtDecodeConfig) -> Self {
        Self {
            config,
            buffer: BytesMut::new(),
            state: DecoderState::Resync,
            pending: None,
        }
    }

    /// Dispatches one state-machine step against the buffered wire data.
    fn process_buffer(&mut self, out: &mut Vec<QbtFrameEvent>) -> Result<bool, QbtProtocolError> {
        match self.state {
            DecoderState::Resync => Ok(self.find_sync()),
            DecoderState::StartFrame => Ok(self.skip_padding()),
            DecoderState::FrameType => Ok(self.detect_frame_type()),
            DecoderState::QbtServerList => self.process_server_list(out),
            DecoderState::BlockHeader => self.process_block_header(),
            DecoderState::BlockBody => self.process_block_body(),
            DecoderState::Validate => self.validate_and_emit(out),
        }
    }

    /// Searches for the six-byte sync marker and retains a suffix for split markers.
    fn find_sync(&mut self) -> bool {
        if self.buffer.len() < SYNC_BYTES.len() {
            return false;
        }

        let pos = self
            .buffer
            .windows(SYNC_BYTES.len())
            .position(|window| window == SYNC_BYTES);

        if let Some(idx) = pos {
            let consume = idx + SYNC_BYTES.len();
            let _ = self.buffer.split_to(consume);
            self.state = DecoderState::StartFrame;
            true
        } else {
            let keep = SYNC_BYTES.len() - 1;
            if self.buffer.len() > keep {
                let consume = self.buffer.len() - keep;
                let _ = self.buffer.split_to(consume);
            }
            false
        }
    }

    /// Consumes any padding that follows a sync marker before frame classification.
    fn skip_padding(&mut self) -> bool {
        let mut skipped = 0usize;
        while self.buffer.first() == Some(&0) {
            let _ = self.buffer.split_to(1);
            skipped += 1;
        }
        if !self.buffer.is_empty() {
            self.state = DecoderState::FrameType;
            return true;
        }
        skipped > 0
    }

    /// Routes the decoder into the handler for the next frame kind.
    fn detect_frame_type(&mut self) -> bool {
        if self.buffer.is_empty() {
            return false;
        }

        if self.buffer[0] != b'/' {
            let _ = self.buffer.split_to(1);
            self.state = DecoderState::Resync;
            return true;
        }

        if self.buffer.len() < 3 {
            return false;
        }

        if self.buffer.starts_with(b"/PF") {
            self.state = DecoderState::BlockHeader;
            return true;
        }

        if self.buffer.starts_with(b"/Se") || self.buffer.starts_with(b"/ServerList/") {
            self.state = DecoderState::QbtServerList;
            return true;
        }

        let _ = self.buffer.split_to(1);
        self.state = DecoderState::Resync;
        true
    }

    fn process_server_list(
        &mut self,
        out: &mut Vec<QbtFrameEvent>,
    ) -> Result<bool, QbtProtocolError> {
        let Some(end_idx) = self.buffer.iter().position(|b| *b == 0) else {
            return Ok(false);
        };

        let frame = self.buffer.split_to(end_idx + 1);
        let content = String::from_utf8(frame[..end_idx].to_vec())
            .map_err(|e| QbtProtocolError::InvalidUtf8(e.to_string()))?;

        let (server_list, warnings) = parse_server_list_frame(&content)?;
        out.push(QbtFrameEvent::ServerListUpdate(server_list));
        out.extend(warnings.into_iter().map(QbtFrameEvent::Warning));
        self.state = DecoderState::StartFrame;
        Ok(true)
    }

    fn process_block_header(&mut self) -> Result<bool, QbtProtocolError> {
        if self.buffer.len() < HEADER_SIZE {
            return Ok(false);
        }

        let header_bytes = self.buffer.split_to(HEADER_SIZE);
        let pending = parse_header(&header_bytes, self.config.max_v2_body_size)?;
        self.pending = Some(pending);
        self.state = DecoderState::BlockBody;
        Ok(true)
    }

    fn process_block_body(&mut self) -> Result<bool, QbtProtocolError> {
        let Some(pending) = self.pending.as_mut() else {
            return Err(QbtProtocolError::InvalidFrameType);
        };

        if self.buffer.len() < pending.length {
            return Ok(false);
        }

        let body = self.buffer.split_to(pending.length);
        let mut content = body.to_vec();

        if pending.version == QbtProtocolVersion::V2 {
            let should_attempt = match self.config.compression_policy {
                QbtV2CompressionPolicy::RequireZlibHeader => has_zlib_header(&content),
                QbtV2CompressionPolicy::TryAlways => true,
            };

            if should_attempt {
                match decompress_zlib(&content) {
                    Ok(decompressed) => content = decompressed,
                    Err(err) => {
                        pending.decompression_failed = true;
                        pending
                            .warnings
                            .push(QbtProtocolWarning::DecompressionFailed {
                                filename: pending.filename.clone(),
                                block_number: pending.block_number,
                                reason: err.to_string(),
                            });
                    }
                }
            }
        }

        pending.content = Bytes::from(content);
        self.state = DecoderState::Validate;
        Ok(true)
    }

    fn validate_and_emit(
        &mut self,
        out: &mut Vec<QbtFrameEvent>,
    ) -> Result<bool, QbtProtocolError> {
        let Some(mut pending) = self.pending.take() else {
            return Err(QbtProtocolError::InvalidFrameType);
        };

        self.state = DecoderState::StartFrame;

        for warning in pending.warnings.drain(..) {
            out.push(QbtFrameEvent::Warning(warning));
        }

        if pending.decompression_failed {
            return Ok(true);
        }

        if pending.total_blocks == 0
            || pending.block_number == 0
            || pending.block_number > pending.total_blocks
        {
            return Ok(true);
        }

        if pending.filename.eq_ignore_ascii_case("FILLFILE.TXT") {
            return Ok(true);
        }

        let expected = if pending.version == QbtProtocolVersion::V1 {
            (pending.checksum & 0xFFFF) as i64
        } else {
            pending.checksum as i64
        };

        let valid_checksum = verify_checksum(&pending.content, expected);
        if !valid_checksum {
            out.push(QbtFrameEvent::Warning(
                QbtProtocolWarning::ChecksumMismatch {
                    filename: pending.filename.clone(),
                    block_number: pending.block_number,
                },
            ));
            return Ok(true);
        }

        if is_text_or_wmo(&pending.filename) {
            let trimmed = trim_text_padding(&pending.content);
            pending.content = Bytes::from(trimmed);
        }

        out.push(QbtFrameEvent::DataBlock(QbtSegment {
            filename: pending.filename,
            block_number: pending.block_number,
            total_blocks: pending.total_blocks,
            content: pending.content,
            checksum: pending.checksum,
            length: pending.length,
            version: pending.version,
            timestamp_utc: pending.timestamp_utc,
            source: None,
        }));

        Ok(true)
    }
}

impl QbtFrameDecoder for QbtProtocolDecoder {
    fn feed(&mut self, chunk: &[u8]) -> Result<Vec<QbtFrameEvent>, QbtProtocolError> {
        let decoded: Vec<u8> = chunk.iter().map(|b| b ^ 0xFF).collect();
        self.buffer.extend_from_slice(&decoded);

        let mut out = Vec::new();
        loop {
            let progressed = match self.process_buffer(&mut out) {
                Ok(progressed) => progressed,
                Err(err) => {
                    out.push(QbtFrameEvent::Warning(
                        QbtProtocolWarning::DecoderRecovered {
                            error: err.to_string(),
                        },
                    ));
                    self.pending = None;
                    self.state = DecoderState::Resync;
                    if matches!(err, QbtProtocolError::InvalidFrameType) && !self.buffer.is_empty()
                    {
                        let _ = self.buffer.split_to(1);
                        true
                    } else {
                        !self.buffer.is_empty()
                    }
                }
            };
            if !progressed {
                break;
            }
        }

        Ok(out)
    }

    fn reset(&mut self) {
        self.buffer.clear();
        self.pending = None;
        self.state = DecoderState::Resync;
    }
}

fn parse_header(input: &[u8], max_v2_body_size: usize) -> Result<PendingSegment, QbtProtocolError> {
    let header =
        std::str::from_utf8(input).map_err(|e| QbtProtocolError::InvalidUtf8(e.to_string()))?;
    let captures = header_regex()
        .captures(header)
        .ok_or(QbtProtocolError::InvalidHeader)?;

    let filename = captures
        .name("pf")
        .map(|m| m.as_str().to_string())
        .ok_or(QbtProtocolError::MissingField("/PF"))?;
    let block_number = capture_u32(&captures, "pn", "/PN")?;
    let total_blocks = capture_u32(&captures, "pt", "/PT")?;
    let checksum = capture_u32(&captures, "cs", "/CS")?;
    let fd = captures
        .name("fd")
        .map(|m| m.as_str())
        .ok_or(QbtProtocolError::MissingField("/FD"))?;

    let (timestamp_utc, warnings) = match parse_fd_timestamp(fd) {
        Some(ts) => (ts, Vec::new()),
        None => (
            SystemTime::now(),
            vec![QbtProtocolWarning::TimestampParseFallback {
                raw: fd.to_string(),
            }],
        ),
    };

    let dl = capture_optional_u32(&captures, "dl")?;
    let (version, length) = if let Some(dl) = dl {
        let len =
            usize::try_from(dl).map_err(|_| QbtProtocolError::InvalidBodyLength(dl as usize))?;
        if len == 0 || len > max_v2_body_size {
            return Err(QbtProtocolError::InvalidBodyLength(len));
        }
        (QbtProtocolVersion::V2, len)
    } else {
        (QbtProtocolVersion::V1, V1_BODY_SIZE)
    };

    Ok(PendingSegment {
        filename,
        block_number,
        total_blocks,
        checksum,
        length,
        version,
        content: Bytes::new(),
        timestamp_utc,
        warnings,
        decompression_failed: false,
    })
}

fn parse_fd_timestamp(fd: &str) -> Option<SystemTime> {
    static FD_FORMAT: &[time::format_description::BorrowedFormatItem<'_>] =
        format_description!("[month]/[day]/[year] [hour repr:12]:[minute]:[second] [period]");

    let parsed = PrimitiveDateTime::parse(fd, FD_FORMAT).ok()?;
    let utc = parsed.assume_utc();
    let seconds = utc.unix_timestamp();
    let nanos = utc.nanosecond();

    if seconds >= 0 {
        Some(UNIX_EPOCH + Duration::new(seconds as u64, nanos))
    } else {
        Some(UNIX_EPOCH - Duration::new(seconds.unsigned_abs(), nanos))
    }
}

fn header_regex() -> &'static Regex {
    static HEADER_RE: OnceLock<Regex> = OnceLock::new();
    HEADER_RE.get_or_init(|| {
        Regex::new(r"^/PF(?P<pf>[A-Za-z0-9._-]+)\s*/PN\s*(?P<pn>[0-9]+)\s*/PT\s*(?P<pt>[0-9]+)\s*/CS\s*(?P<cs>[0-9]+)\s*/FD(?P<fd>[0-9/: ]+[AP]M)\s*(?:/DL(?P<dl>[0-9]+)\s*)?\r\n\s*$")
            .expect("header regex must compile")
    })
}

fn capture_u32(
    caps: &regex::Captures<'_>,
    group: &str,
    field_tag: &'static str,
) -> Result<u32, QbtProtocolError> {
    caps.name(group)
        .ok_or(QbtProtocolError::MissingField(field_tag))?
        .as_str()
        .parse::<u32>()
        .map_err(|_| QbtProtocolError::InvalidHeader)
}

fn capture_optional_u32(
    caps: &regex::Captures<'_>,
    group: &str,
) -> Result<Option<u32>, QbtProtocolError> {
    let Some(value) = caps.name(group) else {
        return Ok(None);
    };
    value
        .as_str()
        .parse::<u32>()
        .map(Some)
        .map_err(|_| QbtProtocolError::InvalidHeader)
}

fn is_text_or_wmo(filename: &str) -> bool {
    let upper = filename.to_ascii_uppercase();
    upper.ends_with(".TXT") || upper.ends_with(".WMO")
}

fn trim_text_padding(content: &[u8]) -> Vec<u8> {
    let mut end = content.len();
    while end > 0 {
        let b = content[end - 1];
        if matches!(b, b'\0' | b' ' | b'\t' | b'\r' | b'\n') {
            end -= 1;
        } else {
            break;
        }
    }
    content[..end].to_vec()
}

#[cfg(test)]
mod tests {
    use super::{
        QbtDecodeConfig, QbtFrameDecoder, QbtFrameEvent, QbtProtocolDecoder, QbtProtocolWarning,
        SYNC_BYTES, V1_BODY_SIZE,
    };
    use crate::qbt_receiver::QbtV2CompressionPolicy;
    use crate::qbt_receiver::protocol::checksum::calculate_qbt_checksum;
    use bytes::Bytes;
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    fn xor_encode(input: &[u8]) -> Vec<u8> {
        input.iter().map(|b| b ^ 0xFF).collect()
    }

    fn build_header(
        filename: &str,
        block: u32,
        total: u32,
        checksum: u32,
        dl: Option<usize>,
    ) -> [u8; 80] {
        let mut raw = if let Some(len) = dl {
            format!(
                "/PF{filename} /PN {block} /PT {total} /CS {checksum} /FD01/01/2024 01:00:00 AM /DL{len}\r\n"
            )
        } else {
            format!(
                "/PF{filename} /PN {block} /PT {total} /CS {checksum} /FD01/01/2024 01:00:00 AM\r\n"
            )
        };
        while raw.len() < 80 {
            raw.push(' ');
        }
        let mut out = [0u8; 80];
        out.copy_from_slice(&raw.as_bytes()[..80]);
        out
    }

    fn frame_with_body(header: [u8; 80], body: &[u8]) -> Vec<u8> {
        let mut decoded = Vec::new();
        decoded.extend_from_slice(SYNC_BYTES);
        decoded.extend_from_slice(&header);
        decoded.extend_from_slice(body);
        xor_encode(&decoded)
    }

    #[test]
    fn find_sync_recovers_after_garbage() {
        let body = [b'A'; V1_BODY_SIZE];
        let checksum = calculate_qbt_checksum(&body) as u32;
        let header = build_header("A.TXT", 1, 1, checksum, None);

        let mut wire = xor_encode(b"garbage");
        wire.extend(frame_with_body(header, &body));

        let mut decoder = QbtProtocolDecoder::default();
        let events = decoder.feed(&wire).expect("decode should succeed");
        assert!(
            events
                .iter()
                .any(|e| matches!(e, QbtFrameEvent::DataBlock(_)))
        );
    }

    #[test]
    fn checksum_strict_drop() {
        let body = [b'A'; V1_BODY_SIZE];
        let bad_checksum = 0;
        let header = build_header("A.TXT", 1, 1, bad_checksum, None);
        let wire = frame_with_body(header, &body);

        let mut decoder = QbtProtocolDecoder::new(QbtDecodeConfig {
            checksum_policy: crate::qbt_receiver::config::QbtChecksumPolicy::StrictDrop,
            ..QbtDecodeConfig::default()
        });

        let events = decoder.feed(&wire).expect("decode should succeed");
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, QbtFrameEvent::DataBlock(_)))
        );
        assert!(events.iter().any(|e| matches!(
            e,
            QbtFrameEvent::Warning(QbtProtocolWarning::ChecksumMismatch { .. })
        )));
    }

    #[test]
    fn fillfile_filtered() {
        let body = [b'A'; V1_BODY_SIZE];
        let checksum = calculate_qbt_checksum(&body) as u32;
        let header = build_header("FILLFILE.TXT", 1, 1, checksum, None);
        let wire = frame_with_body(header, &body);

        let mut decoder = QbtProtocolDecoder::default();
        let events = decoder.feed(&wire).expect("decode should succeed");
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, QbtFrameEvent::DataBlock(_)))
        );
    }

    #[test]
    fn trim_padding_text_wmo() {
        let mut body = [0u8; V1_BODY_SIZE];
        body[..5].copy_from_slice(b"HELLO");
        body[5] = b' ';
        body[6] = b'\n';
        let checksum = calculate_qbt_checksum(&body) as u32;
        let header = build_header("A.WMO", 1, 1, checksum, None);
        let wire = frame_with_body(header, &body);

        let mut decoder = QbtProtocolDecoder::default();
        let events = decoder.feed(&wire).expect("decode should succeed");

        let data = events
            .iter()
            .find_map(|evt| match evt {
                QbtFrameEvent::DataBlock(seg) => Some(seg.content.clone()),
                _ => None,
            })
            .expect("expected data segment");

        assert_eq!(data, Bytes::from_static(b"HELLO"));
    }

    #[test]
    fn unknown_frame_resync() {
        let body = [b'A'; V1_BODY_SIZE];
        let checksum = calculate_qbt_checksum(&body) as u32;
        let header = build_header("A.TXT", 1, 1, checksum, None);

        let mut decoded = Vec::new();
        decoded.extend_from_slice(SYNC_BYTES);
        decoded.extend_from_slice(b"/XXTHIS_IS_UNKNOWN");
        decoded.extend_from_slice(SYNC_BYTES);
        decoded.extend_from_slice(&header);
        decoded.extend_from_slice(&body);
        let wire = xor_encode(&decoded);

        let mut decoder = QbtProtocolDecoder::default();
        let events = decoder.feed(&wire).expect("decode should succeed");
        assert!(
            events
                .iter()
                .any(|e| matches!(e, QbtFrameEvent::DataBlock(_)))
        );
    }

    #[test]
    fn v1_checksum_masking() {
        let mut body = [0u8; V1_BODY_SIZE];
        body[0] = 1;
        let header = build_header("mask.txt", 1, 1, 65_537, None);
        let wire = frame_with_body(header, &body);

        let mut decoder = QbtProtocolDecoder::new(QbtDecodeConfig {
            checksum_policy: crate::qbt_receiver::config::QbtChecksumPolicy::StrictDrop,
            ..QbtDecodeConfig::default()
        });

        let events = decoder.feed(&wire).expect("decode should succeed");
        assert!(
            events
                .iter()
                .any(|e| matches!(e, QbtFrameEvent::DataBlock(_)))
        );
    }

    #[test]
    fn v2_header_gate() {
        let body = b"HELLO";
        let checksum = calculate_qbt_checksum(body) as u32;
        let header = build_header("gated.dat", 1, 1, checksum, Some(body.len()));
        let wire = frame_with_body(header, body);

        let mut decoder = QbtProtocolDecoder::new(QbtDecodeConfig {
            compression_policy: QbtV2CompressionPolicy::RequireZlibHeader,
            ..QbtDecodeConfig::default()
        });

        let events = decoder.feed(&wire).expect("decode should succeed");
        let segment = events.iter().find_map(|evt| match evt {
            QbtFrameEvent::DataBlock(seg) => Some(seg),
            _ => None,
        });
        let segment = segment.expect("expected v2 segment");
        assert_eq!(segment.content, Bytes::from_static(b"HELLO"));
    }

    #[test]
    fn v2_compressed_roundtrip() {
        let plain = b"COMPRESSED_PAYLOAD";
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(plain)
            .expect("write to zlib encoder should work");
        let compressed = encoder.finish().expect("zlib finish should work");

        let checksum = calculate_qbt_checksum(plain) as u32;
        let header = build_header("zlib.dat", 1, 1, checksum, Some(compressed.len()));
        let wire = frame_with_body(header, &compressed);

        let mut decoder = QbtProtocolDecoder::default();
        let events = decoder.feed(&wire).expect("decode should succeed");
        let segment = events.iter().find_map(|evt| match evt {
            QbtFrameEvent::DataBlock(seg) => Some(seg),
            _ => None,
        });
        let segment = segment.expect("expected v2 segment");
        assert_eq!(segment.content, Bytes::from_static(plain));
    }

    #[test]
    fn v2_decompress_failure_drops_segment_and_emits_warning() {
        let bogus = vec![0x78, 0x9C, 0xFF, 0x00, 0x00];
        let checksum = calculate_qbt_checksum(&bogus) as u32;
        let header = build_header("badzlib.dat", 1, 1, checksum, Some(bogus.len()));
        let wire = frame_with_body(header, &bogus);

        let mut decoder = QbtProtocolDecoder::new(QbtDecodeConfig {
            compression_policy: QbtV2CompressionPolicy::RequireZlibHeader,
            ..QbtDecodeConfig::default()
        });

        let events = decoder.feed(&wire).expect("decode should succeed");
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, QbtFrameEvent::DataBlock(_)))
        );
        assert!(events.iter().any(|e| matches!(
            e,
            QbtFrameEvent::Warning(QbtProtocolWarning::DecompressionFailed { .. })
        )));
    }

    #[test]
    fn invalid_server_list_utf8_recovers_and_decodes_next_frame() {
        let body = [b'A'; V1_BODY_SIZE];
        let checksum = calculate_qbt_checksum(&body) as u32;
        let header = build_header("recover.txt", 1, 1, checksum, None);

        let mut decoded = Vec::new();
        decoded.extend_from_slice(SYNC_BYTES);
        decoded.extend_from_slice(b"/ServerList/");
        decoded.extend_from_slice(&[0xFF, 0xFE, 0x00]);
        decoded.extend_from_slice(SYNC_BYTES);
        decoded.extend_from_slice(&header);
        decoded.extend_from_slice(&body);
        let wire = xor_encode(&decoded);

        let mut decoder = QbtProtocolDecoder::default();
        let events = decoder.feed(&wire).expect("decode should recover");

        assert!(events.iter().any(|evt| matches!(
            evt,
            QbtFrameEvent::Warning(QbtProtocolWarning::DecoderRecovered { .. })
        )));
        assert!(
            events
                .iter()
                .any(|evt| matches!(evt, QbtFrameEvent::DataBlock(_)))
        );
    }

    #[test]
    fn frame_type_waits_for_partial_prefix() {
        let body = [b'A'; V1_BODY_SIZE];
        let checksum = calculate_qbt_checksum(&body) as u32;
        let header = build_header("split.txt", 1, 1, checksum, None);
        let wire = frame_with_body(header, &body);

        let mut decoder = QbtProtocolDecoder::default();
        let mut events = Vec::new();
        events.extend(
            decoder
                .feed(&wire[..8])
                .expect("first partial chunk should decode"),
        );
        events.extend(
            decoder
                .feed(&wire[8..])
                .expect("second chunk should decode"),
        );

        assert!(
            events
                .iter()
                .any(|e| matches!(e, QbtFrameEvent::DataBlock(_)))
        );
    }

    #[test]
    fn fd_parse_failure_emits_warning_and_uses_fallback() {
        let body = [b'A'; V1_BODY_SIZE];
        let checksum = calculate_qbt_checksum(&body) as u32;
        let mut raw =
            format!("/PFbadfd.txt /PN 1 /PT 1 /CS {checksum} /FD99/99/2024 99:99:99 AM\r\n");
        while raw.len() < 80 {
            raw.push(' ');
        }
        let mut header = [0u8; 80];
        header.copy_from_slice(&raw.as_bytes()[..80]);
        let wire = frame_with_body(header, &body);

        let mut decoder = QbtProtocolDecoder::default();
        let events = decoder.feed(&wire).expect("decode should succeed");

        assert!(events.iter().any(|evt| matches!(
            evt,
            QbtFrameEvent::Warning(QbtProtocolWarning::TimestampParseFallback { .. })
        )));
        assert!(
            events
                .iter()
                .any(|evt| matches!(evt, QbtFrameEvent::DataBlock(_)))
        );
    }
}

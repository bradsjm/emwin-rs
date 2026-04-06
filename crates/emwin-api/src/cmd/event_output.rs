//! Render protocol events into CLI-facing JSON payloads.

use emwin_parser::enrich_product;
use emwin_protocol::qbt_receiver::QbtFrameEvent;
use std::time::{SystemTime, UNIX_EPOCH};

/// Returns the stable event name used by CLI JSON output.
pub fn frame_event_name(event: &QbtFrameEvent) -> &'static str {
    match event {
        QbtFrameEvent::DataBlock(_) => "data_block",
        QbtFrameEvent::ServerListUpdate(_) => "server_list",
        QbtFrameEvent::Warning(_) => "warning",
        _ => "unknown",
    }
}

/// Converts a frame event into the JSON shape used by the CLI.
pub fn frame_event_to_json(event: &QbtFrameEvent, text_preview_chars: usize) -> serde_json::Value {
    match event {
        QbtFrameEvent::DataBlock(seg) => {
            let mut value = serde_json::json!({
                "type":"data_block",
                "filename":seg.filename,
                "block_number":seg.block_number,
                "total_blocks":seg.total_blocks,
                "length":seg.content.len(),
                "version": format!("{:?}", seg.version),
                "timestamp_utc": unix_seconds(seg.timestamp_utc),
            });
            if let Some(preview) = text_preview(&seg.filename, &seg.content, text_preview_chars) {
                value["preview"] = serde_json::Value::String(preview);
            }
            if let Ok(product_json) =
                serde_json::to_value(enrich_product(&seg.filename, &seg.content))
            {
                value["product"] = product_json;
            }
            value
        }
        QbtFrameEvent::ServerListUpdate(list) => serde_json::json!({
            "type":"server_list",
            "servers": list.servers,
            "sat_servers": list.sat_servers,
        }),
        QbtFrameEvent::Warning(w) => serde_json::json!({
            "type":"warning",
            "warning": format!("{:?}", w),
        }),
        _ => serde_json::json!({
            "type":"unknown",
        }),
    }
}

fn unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Builds a sanitized preview for text-like payloads.
///
/// The preview intentionally drops non-ASCII noise because it is used for human-facing logging,
/// not lossless output.
pub fn text_preview(filename: &str, bytes: &[u8], max_chars: usize) -> Option<String> {
    if max_chars == 0 || !is_text_like(filename) {
        return None;
    }

    let mut normalized = String::new();
    for ch in String::from_utf8_lossy(bytes).chars() {
        if normalized.chars().count() >= max_chars {
            break;
        }
        if ch.is_ascii_graphic() {
            normalized.push(ch);
            continue;
        }
        if ch.is_ascii_whitespace() {
            normalized.push(' ');
        }
    }

    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

/// Checks if a filename indicates a text-like file type.
fn is_text_like(filename: &str) -> bool {
    let upper = filename.to_ascii_uppercase();
    upper.ends_with(".TXT")
        || upper.ends_with(".WMO")
        || upper.ends_with(".XML")
        || upper.ends_with(".JSON")
}

#[cfg(test)]
mod tests {
    use super::text_preview;

    #[test]
    fn preview_strips_non_printable_and_non_ascii() {
        let bytes = b"HELLO\x00\x1f\x7f\nWORLD\t\xf0\x9f\x98\x80";
        let preview = text_preview("sample.txt", bytes, 200).expect("preview should exist");
        assert_eq!(preview, "HELLO WORLD");
    }

    #[test]
    fn preview_returns_none_when_no_printable_content() {
        let bytes = b"\x00\x01\x02\x7f\n\t\r";
        let preview = text_preview("sample.txt", bytes, 200);
        assert_eq!(preview, None);
    }
}

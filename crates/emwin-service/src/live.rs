use crate::archive::IncidentChange;
use crate::metadata::CompletedFileMetadata;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::SystemTime;
use utoipa::ToSchema;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Qbt,
    WxWire {
        message_id: String,
        subject: String,
        #[schema(value_type = String, format = DateTime)]
        delay_stamp_utc: Option<SystemTime>,
    },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ReceiverFrame {
    #[schema(example = "qbt")]
    pub receiver: String,
    #[schema(example = "server_list")]
    pub event_name: String,
    #[schema(value_type = Object)]
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct RetainedFile {
    pub data: Bytes,
    pub completed_at: SystemTime,
    pub metadata: CompletedFileMetadata,
}

#[derive(Debug, Clone)]
pub struct LiveBroadcastEvent {
    pub id: u64,
    pub kind: LiveEventKind,
}

#[derive(Debug, Clone)]
pub struct IncidentBroadcastEvent {
    pub id: u64,
    pub change: IncidentChange,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "receiver", rename_all = "snake_case")]
pub enum LiveTelemetry {
    Unavailable,
    #[schema(value_type = Object)]
    Qbt(Value),
    #[schema(value_type = Object)]
    WxWire(Value),
}

#[derive(Debug, Clone)]
pub enum LiveEventKind {
    Connected { endpoint: String },
    Disconnected,
    ReceiverFrame(ReceiverFrame),
    ProductAvailable(Box<CompletedFileMetadata>),
    Telemetry(LiveTelemetry),
    Error { message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PersistenceStats {
    pub queue_len: usize,
    pub queue_capacity: usize,
    pub enqueued_total: u64,
    pub evicted_total: u64,
    pub persisted_total: u64,
    pub failed_total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct LiveStatsSnapshot {
    pub uptime_secs: u64,
    pub data_blocks_total: u64,
    pub active_servers: usize,
    pub retained_files: usize,
    pub upstream_endpoint: Option<String>,
    pub persistence: Option<PersistenceStats>,
}

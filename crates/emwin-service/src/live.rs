use crate::archive::{BoxFuture, IncidentChange};
use crate::error::ServiceResult;
use crate::metadata::CompletedFileMetadata;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::SystemTime;
use tokio::sync::broadcast;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Qbt,
    WxWire {
        message_id: String,
        subject: String,
        delay_stamp_utc: Option<SystemTime>,
    },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiverFrame {
    pub receiver: String,
    pub event_name: String,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct RetainedFile {
    pub data: Vec<u8>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "receiver", rename_all = "snake_case")]
pub enum LiveTelemetry {
    Unavailable,
    Qbt(Value),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistenceStats {
    pub queue_len: usize,
    pub queue_capacity: usize,
    pub enqueued_total: u64,
    pub evicted_total: u64,
    pub persisted_total: u64,
    pub failed_total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveStatsSnapshot {
    pub uptime_secs: u64,
    pub data_blocks_total: u64,
    pub received_servers: usize,
    pub retained_files: usize,
    pub upstream_endpoint: Option<String>,
    pub persistence: Option<PersistenceStats>,
}

pub trait LiveEventService: Send + Sync {
    fn subscribe_events(&self) -> broadcast::Receiver<LiveBroadcastEvent>;
    fn telemetry_snapshot(&self) -> LiveTelemetry;
    fn stats_snapshot(&self) -> LiveStatsSnapshot;
    fn shutdown(&self) -> BoxFuture<'_, ServiceResult<()>>;
}

pub trait RetainedFileService: Send + Sync {
    fn list_retained_files(&self) -> Vec<CompletedFileMetadata>;
    fn get_retained_file(&self, filename: &str) -> Option<RetainedFile>;
}

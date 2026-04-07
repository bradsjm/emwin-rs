use crate::types::{AppState, IncidentBroadcastEvent, LiveBroadcastEvent, LiveEventKind};
use emwin_service::IncidentChange;
use std::sync::Arc;

pub(crate) fn publish(state: &Arc<AppState>, kind: LiveEventKind) {
    let id = state
        .next_event_id
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let _ = state.event_tx.send(LiveBroadcastEvent { id, kind });
}

pub(crate) fn publish_incident_change(state: &Arc<AppState>, change: IncidentChange) {
    let id = state
        .next_incident_event_id
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let _ = state
        .incident_event_tx
        .send(IncidentBroadcastEvent { id, change });
}

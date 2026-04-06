use super::types::{
    AppState, BroadcastEvent, EventKind, IncidentBroadcastEvent, IncidentEventPayload,
};
use std::sync::Arc;

pub(crate) fn publish(state: &Arc<AppState>, kind: EventKind) {
    let id = state
        .next_event_id
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let _ = state.event_tx.send(BroadcastEvent { id, kind });
}

pub(crate) fn publish_incident_change(state: &Arc<AppState>, payload: IncidentEventPayload) {
    let id = state
        .next_incident_event_id
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let _ = state
        .incident_event_tx
        .send(IncidentBroadcastEvent { id, payload });
}

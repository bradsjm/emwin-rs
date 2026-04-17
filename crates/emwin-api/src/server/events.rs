use super::types::{AppState, IncidentBroadcastEvent, IncidentEventPayload};
use std::sync::Arc;

pub(crate) fn publish_incident_change(state: &Arc<AppState>, payload: IncidentEventPayload) {
    let id = state
        .next_incident_event_id
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let _ = state
        .incident_event_tx
        .send(IncidentBroadcastEvent { id, payload });
}

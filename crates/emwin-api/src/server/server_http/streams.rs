use super::super::types::{
    AppState, ClientGuard, CompletedFileEventPayload, EventFilter, EventKind, EventsQuery,
    IncidentBroadcastEvent, IncidentEventFilter, IncidentEventPayload, IncidentEventsQuery,
};
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use emwin_service::LiveEventKind;
use futures::Stream;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[utoipa::path(
    get,
    path = "/v1/streams/products",
    tag = "streams",
    description = "Incremental SSE stream of completed products. Clients should fetch an initial snapshot from the resource endpoints, then attach the stream. `Last-Event-ID` is best-effort for short reconnect gaps only; lag warnings require a full resync.",
    params(EventsQuery),
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "Server-sent events stream of live feed activity.", body = crate::server::openapi::SseEventEnvelope, content_type = "text/event-stream"),
        (status = 400, description = "Event filter query validation failed.", body = String),
        (status = 429, description = "Concurrent SSE client limit reached.", body = String)
    )
)]
pub(super) async fn events_handler(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)> {
    let last_id = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    if query
        .min_size
        .zip(query.max_size)
        .is_some_and(|(min, max)| min > max)
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "min_size must be less than or equal to max_size".to_string(),
        ));
    }
    let filter =
        EventFilter::try_from_query(query).map_err(|err| (StatusCode::BAD_REQUEST, err.message))?;
    let guard = acquire_client_guard(&state, peer)?;
    let rx = state.services.subscribe_events();
    let shutdown_rx = state.shutdown_rx.clone();

    let stream = futures::stream::unfold(
        StreamState {
            state: Arc::clone(&state),
            rx: Some(rx),
            last_id,
            filter,
            shutdown_rx,
            peer,
            _guard: Some(guard),
        },
        move |mut st| async move {
            let rx = st.rx.as_mut()?;
            loop {
                tokio::select! {
                    _ = st.shutdown_rx.changed() => return None,
                    received = rx.recv() => match received {
                    Ok(event) => {
                        if event.id <= st.last_id {
                            continue;
                        }
                        let LiveEventKind::ProductAvailable(metadata) = event.kind else {
                            continue;
                        };
                        let event_kind =
                            EventKind::FileComplete(Box::new(CompletedFileEventPayload::from_metadata(*metadata)));
                        if !event_matches_filter(&st.filter, &event_kind) {
                            continue;
                        }

                        st.last_id = event.id;
                        let payload = match serde_json::to_string(&event_kind.to_json()) {
                            Ok(payload) => payload,
                            Err(_) => "{}".to_string(),
                        };
                        let sse = Event::default()
                            .id(event.id.to_string())
                            .event(event_kind.event_name())
                            .data(payload);
                        return Some((Ok(sse), st));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(dropped)) => {
                        super::super::log_info(
                            st.state.quiet,
                            &format!("sse client lagged peer={} dropped={}", st.peer, dropped),
                        );
                        let warning = Event::default().event("warning").data(
                            serde_json::json!({
                                "message": "client lagged; events dropped",
                                "dropped": dropped,
                                "peer": st.peer,
                            })
                            .to_string(),
                        );
                        return Some((Ok(warning), st));
                    }
                    }
                }
            }
        },
    );

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

#[utoipa::path(
    get,
    path = "/v1/streams/incidents",
    tag = "streams",
    description = "Incremental SSE stream of persisted incident projection changes. Clients should fetch an initial snapshot from the incident resource endpoints, then attach the stream. `Last-Event-ID` is best-effort for short reconnect gaps only; lag warnings require a full resync.",
    params(IncidentEventsQuery),
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "Server-sent events stream of persisted incident projection changes.", body = crate::server::openapi::SseEventEnvelope, content_type = "text/event-stream"),
        (status = 429, description = "Concurrent SSE client limit reached.", body = String),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
pub(super) async fn incident_events_handler(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<IncidentEventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)> {
    if state.services.subscribe_incident_changes().is_none() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "archive database is not configured".to_string(),
        ));
    }
    let guard = acquire_client_guard(&state, peer)?;

    let rx = state.incident_event_tx.subscribe();
    let shutdown_rx = state.shutdown_rx.clone();
    let last_id = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let filter = IncidentEventFilter::from_query(query);

    let stream = futures::stream::unfold(
        IncidentStreamState {
            state: Arc::clone(&state),
            rx: Some(rx),
            last_id,
            filter,
            shutdown_rx,
            peer,
            _guard: Some(guard),
        },
        move |mut st| async move {
            let rx = st.rx.as_mut()?;
            loop {
                tokio::select! {
                    _ = st.shutdown_rx.changed() => return None,
                    received = rx.recv() => match received {
                    Ok(event) => {
                        if event.id <= st.last_id {
                            continue;
                        }
                        if !st.filter.matches(&event.payload) {
                            continue;
                        }

                        st.last_id = event.id;
                        let payload = match serde_json::to_string(&event.payload) {
                            Ok(payload) => payload,
                            Err(_) => "{}".to_string(),
                        };
                        let sse = Event::default()
                            .id(event.id.to_string())
                            .event(IncidentEventPayload::EVENT_NAME)
                            .data(payload);
                        return Some((Ok(sse), st));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(dropped)) => {
                        super::super::log_info(
                            st.state.quiet,
                            &format!("incident sse client lagged peer={} dropped={}", st.peer, dropped),
                        );
                        let warning = Event::default().event("warning").data(
                            serde_json::json!({
                                "message": "client lagged; events dropped",
                                "dropped": dropped,
                                "peer": st.peer,
                            })
                            .to_string(),
                        );
                        return Some((Ok(warning), st));
                    }
                    }
                }
            }
        },
    );

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

pub(crate) fn event_matches_filter(filter: &EventFilter, event: &EventKind) -> bool {
    filter.matches(event)
}

pub(super) struct StreamState {
    pub(super) state: Arc<AppState>,
    pub(super) rx: Option<tokio::sync::broadcast::Receiver<emwin_service::LiveBroadcastEvent>>,
    pub(super) last_id: u64,
    pub(super) filter: EventFilter,
    pub(super) shutdown_rx: tokio::sync::watch::Receiver<bool>,
    pub(super) peer: SocketAddr,
    pub(super) _guard: Option<ClientGuard>,
}

pub(super) struct IncidentStreamState {
    pub(super) state: Arc<AppState>,
    pub(super) rx: Option<tokio::sync::broadcast::Receiver<IncidentBroadcastEvent>>,
    pub(super) last_id: u64,
    pub(super) filter: IncidentEventFilter,
    pub(super) shutdown_rx: tokio::sync::watch::Receiver<bool>,
    pub(super) peer: SocketAddr,
    pub(super) _guard: Option<ClientGuard>,
}

fn reserve_client_slot(
    state: &Arc<AppState>,
    peer: SocketAddr,
) -> Result<(), (StatusCode, String)> {
    if state
        .connected_clients
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            (current < state.max_clients).then_some(current + 1)
        })
        .is_err()
    {
        super::super::log_info(
            state.quiet,
            &format!("rejecting client; limit reached peer={peer}"),
        );
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "client limit reached".to_string(),
        ));
    }

    super::super::log_info(state.quiet, &format!("sse client connected peer={peer}"));
    Ok(())
}

fn acquire_client_guard(
    state: &Arc<AppState>,
    peer: SocketAddr,
) -> Result<ClientGuard, (StatusCode, String)> {
    reserve_client_slot(state, peer)?;
    Ok(ClientGuard {
        state: Arc::clone(state),
        peer,
    })
}

CREATE SCHEMA IF NOT EXISTS alerting;

CREATE TABLE IF NOT EXISTS alerting.contact_points (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    config_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT alerting_contact_points_kind_check CHECK (kind IN ('webhook', 'apprise'))
);

CREATE TABLE IF NOT EXISTS alerting.rules (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name TEXT NOT NULL,
    source_kind TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    criteria_json JSONB NOT NULL,
    trigger_policy_json JSONB NOT NULL,
    template_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT alerting_rules_source_kind_check CHECK (
        source_kind IN ('product_available', 'incident_change')
    )
);

CREATE TABLE IF NOT EXISTS alerting.rule_targets (
    rule_id BIGINT NOT NULL REFERENCES alerting.rules(id) ON DELETE CASCADE,
    contact_point_id BIGINT NOT NULL REFERENCES alerting.contact_points(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    PRIMARY KEY (rule_id, contact_point_id)
);

CREATE TABLE IF NOT EXISTS alerting.silences (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    criteria_json JSONB NOT NULL,
    starts_at TIMESTAMPTZ NOT NULL,
    ends_at TIMESTAMPTZ NOT NULL,
    reason TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS alerting.source_events (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    source_kind TEXT NOT NULL,
    source_id TEXT NOT NULL,
    payload_json JSONB NOT NULL,
    source_timestamp TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    claimed_at TIMESTAMPTZ,
    processed_at TIMESTAMPTZ,
    CONSTRAINT alerting_source_events_source_kind_check CHECK (
        source_kind IN ('product_available', 'incident_change')
    )
);

CREATE TABLE IF NOT EXISTS alerting.events (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    rule_id BIGINT NOT NULL REFERENCES alerting.rules(id) ON DELETE CASCADE,
    source_event_id BIGINT NOT NULL REFERENCES alerting.source_events(id) ON DELETE CASCADE,
    delivery_key TEXT NOT NULL,
    severity TEXT,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    payload_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT alerting_events_delivery_key_key UNIQUE (delivery_key)
);

CREATE TABLE IF NOT EXISTS alerting.delivery_attempts (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    alert_event_id BIGINT NOT NULL REFERENCES alerting.events(id) ON DELETE CASCADE,
    contact_point_id BIGINT NOT NULL REFERENCES alerting.contact_points(id) ON DELETE CASCADE,
    delivery_key TEXT NOT NULL,
    attempt_no INTEGER NOT NULL,
    status TEXT NOT NULL,
    claimed_at TIMESTAMPTZ,
    next_retry_at TIMESTAMPTZ,
    response_code INTEGER,
    response_excerpt TEXT,
    delivered_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT alerting_delivery_attempts_status_check CHECK (
        status IN ('pending', 'in_progress', 'delivered', 'retry_pending', 'failed', 'suppressed')
    )
);

CREATE INDEX IF NOT EXISTS alerting_rules_source_kind_enabled_idx
    ON alerting.rules (source_kind, enabled);
CREATE INDEX IF NOT EXISTS alerting_rule_targets_contact_point_idx
    ON alerting.rule_targets (contact_point_id, position);
CREATE INDEX IF NOT EXISTS alerting_silences_window_idx
    ON alerting.silences (starts_at, ends_at);
CREATE INDEX IF NOT EXISTS alerting_source_events_pending_idx
    ON alerting.source_events (source_kind, processed_at, claimed_at, source_timestamp);
CREATE INDEX IF NOT EXISTS alerting_source_events_source_key_idx
    ON alerting.source_events (source_kind, source_id, source_timestamp DESC);
CREATE INDEX IF NOT EXISTS alerting_events_rule_created_idx
    ON alerting.events (rule_id, created_at DESC);
CREATE INDEX IF NOT EXISTS alerting_events_source_event_idx
    ON alerting.events (source_event_id);
CREATE INDEX IF NOT EXISTS alerting_delivery_attempts_pending_idx
    ON alerting.delivery_attempts (status, next_retry_at, claimed_at, updated_at);
CREATE INDEX IF NOT EXISTS alerting_delivery_attempts_event_idx
    ON alerting.delivery_attempts (alert_event_id, contact_point_id);

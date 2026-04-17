# Alerting System Design

## Implementation Status

Backend V1 is now implemented in the workspace:

- `crates/emwin-alert` provides the worker runtime, templating, webhook delivery, Apprise delivery, and retry handling
- `crates/emwin-db` owns the `alerting` schema, durable source events, rule/contact-point/silence storage, simulation, and delivery-attempt state
- `crates/emwin-api` exposes `/v1/alerting/*` CRUD, simulation, audit, and contact-point test endpoints
- `crates/emwin-cli` exposes `alert-worker` and server-side `--alerting-apprise-api-url`

Current V1 limits:

- rule phases are `fire` only
- incident simulation is live-forward only from retained `alerting.source_events`
- contact-point test sends for Apprise require the server process to know the Apprise API base URL
- MCP and a UI remain out of scope

## Purpose

This document defines the alerting subsystem for `emwin-rs`.

The goal is to let operators define weather-driven alert rules that trigger notifications to external endpoints when incoming EMWIN-derived product or incident data matches configured criteria.

This proposal assumes:

- at-least-once delivery is acceptable
- delivery fanout must not interfere with ingest or API serving
- alerting configuration must be durable and auditable
- initial notification breadth should come from Apprise, not custom per-service adapters

## Summary of Decisions

- Alerting runs as a separate worker process or container in production.
- Postgres is the system of record for alerting state.
- `emwin-api` owns CRUD, simulation, and audit-read APIs.
- If AI-assisted configuration is added, MCP should be an AI-facing control-plane surface over the same alerting domain logic, not a separate rule engine.
- A new alert worker owns rule evaluation, queue claiming, rendering, delivery, and retries.
- `reqwest` plus `reqwest-middleware` and `reqwest-retry` is the outbound HTTP stack.
- Apprise is the default delivery gateway for chat, push, and email-style integrations.
- First-party webhook delivery remains native to `emwin-rs` so the payload contract stays under our control.
- Delivery semantics are at-least-once with stable per-event delivery keys.
- The existing typed EMWIN/VTEC/UGC filter grammar remains the base rule model.
- "Basic" and "Advanced" modes are two editors over one canonical stored rule shape.

## Why This Should Be Separate

Alerting should not run in-process with the live ingest runtime or HTTP server in deployed environments.

Reasons:

- delivery latency and retry storms must not degrade ingest or API responsiveness
- alert evaluation and delivery have different scaling characteristics than ingest
- alerting requires durable queues and audit records; the current live SSE streams are not durable replay logs
- contact-point secrets and delivery credentials benefit from a narrower operational blast radius
- Apprise is already an external runtime dependency, so the architecture is naturally multi-process

## Existing Capabilities This Design Reuses

The current repository already provides most of the normalized source data required for alerting:

- completed product events with rich parsed metadata
- incident change events
- durable archive-backed product and incident read models
- a shared typed filter grammar over product, body, geography, and severity fields

Relevant current surfaces:

- `product_available` event stream
- `incident_change` event stream
- archive products
- archive incidents
- archive features
- shared archive and live filter parsing

This means the valuable work is not parsing weather products again.
The valuable work is:

- durable event handoff
- rule modeling
- dedupe
- silence handling
- delivery
- auditability

## Non-Goals for V1

- exactly-once delivery
- distributed rate limiting across multiple worker replicas
- a generic cross-domain rules platform
- SMS or pager vendor-specific native adapters
- using SSE as the internal event transport for alerting
- keeping alert runtime state only in memory

## High-Level Architecture

### Components

#### `emwin-live`

Responsibilities:

- continue ingesting and normalizing weather products
- continue writing retained-file and archive metadata
- produce durable alert source events as part of the persistence path

#### `emwin-api`

Responsibilities:

- manage alert rules
- manage contact points
- manage silences
- provide rule simulation endpoints
- provide alert event and delivery audit APIs

#### MCP control plane

Responsibilities:

- expose AI-facing alerting configuration tools over HTTP
- translate user intent into proposed rule or contact-point changes
- require validation and simulation before persistence
- call the same alerting domain services used by the REST API
- enforce confirmation and redaction rules for mutating operations

#### `alert-worker`

Responsibilities:

- poll durable source events
- evaluate enabled rules
- apply silence and dedupe logic
- persist logical alert events
- enqueue and execute delivery attempts
- retry transient failures
- record final outcomes

#### `Apprise`

Responsibilities:

- provide notification fanout to broad third-party endpoint types
- accept simple rendered notification payloads from the alert worker

#### Postgres

Responsibilities:

- durable source-event handoff
- rule state
- contact-point state
- silence state
- logical alert events
- delivery attempts

### Process Topology

Recommended production topology:

- `emwin-cli server`
- optional separate MCP HTTP service or isolated MCP router
- `emwin-cli alert-worker`
- `caronc/apprise`
- Postgres

The alert worker may be horizontally scaled if queue-claiming is implemented correctly.
Source events and delivery attempts use lease-based claims, so crashed workers do not permanently strand rows.
Delivery attempts move through `in_progress` while a worker owns them and become claimable again only after the delivery lease expires.
Outbound webhook and Apprise calls use a default HTTP timeout; webhook contact points may override it with `timeout_secs`.

If MCP is added, it should not run inside the alert worker.
It belongs on the control plane, not the delivery plane.

## Data Flow

### 1. Source Event Persistence

When a completed product or incident change is durably accepted into the system, a normalized alert source event is written to Postgres.

Initial source kinds:

- `product_available`
- `incident_change`

The alert worker consumes these rows.

### 2. Rule Evaluation

The worker selects unprocessed source events whose claim lease is available, loads enabled rules for the matching source kind, and evaluates them against the normalized event payload.

### 3. Silence and Dedupe

If the event matches a silence, no delivery work is created.

If the event matches a rule but falls within the rule's dedupe or cooldown window, the match is recorded as suppressed and no delivery work is created.

### 4. Logical Alert Event Creation

Each surviving match becomes one logical alert event with:

- a stable rule id
- source event linkage
- a dedupe key
- rendered payload snapshot
- severity and labels

Product alert delivery keys are stable for the product source identity.
Incident alert delivery keys include the durable source event id so repeated updates for the same incident/action are not discarded by the delivery-key uniqueness constraint after cooldown expires.

### 5. Delivery Attempt Creation

For each target contact point attached to the rule, the system creates a delivery-attempt record.

### 6. Delivery Execution

The worker claims pending delivery attempts and sends them through:

- Apprise for generalized notification targets
- direct webhook delivery for first-party webhook targets

### 7. Retry and Finalization

Transient failures are retried with bounded exponential backoff and jitter.

Final outcomes are persisted for audit and troubleshooting.

## Delivery Semantics

The system is explicitly at-least-once.

This means:

- duplicate notifications can occur on timeout or retry boundaries
- every logical alert event must have a stable delivery key
- outbound requests should include that delivery key in headers and payloads where possible

Recommended delivery-key shape:

- `rule_id`
- logical event phase
- source identity

Examples:

- `rule_id + product_id`
- `rule_id + incident identity + incident action`
- `rule_id + phase + incident identity`

The delivery key should be stored in:

- `alert_events`
- `alert_delivery_attempts`
- outbound webhook headers
- outbound webhook or Apprise body metadata when possible

## Runtime and Failure Model

### Reliability Rules

- alerting must not depend on in-memory broadcasts
- rule evaluation must be restart-safe
- worker progress must be resumable after crash or deploy
- retries must be bounded
- queue claiming must prevent duplicate concurrent execution of the same delivery attempt

### Failure Isolation

Expected failures:

- Apprise unavailable
- downstream webhook timeout
- malformed target configuration
- Postgres transient failure
- worker crash during retry window

The design should isolate these failures to the alerting subsystem.

`emwin-live` and `emwin-api` should remain functional even if delivery is degraded.

## Persistence Model

The exact schema can evolve, but V1 needs these tables. They should be in a separate alerting schema.

### `alert_contact_points`

Purpose:

- durable storage for configured destinations

Suggested fields:

- `id`
- `name`
- `kind`
- `enabled`
- `config_json`
- `created_at`
- `updated_at`

Notes:

- `config_json` stores contact-point-specific settings
- secret-bearing fields must be redacted on read

### `alert_rules`

Purpose:

- durable storage for rule definitions

Suggested fields:

- `id`
- `name`
- `source_kind`
- `enabled`
- `criteria_json`
- `trigger_policy_json`
- `template_json`
- `created_at`
- `updated_at`

### `alert_rule_targets`

Purpose:

- link rules to one or more contact points

Suggested fields:

- `rule_id`
- `contact_point_id`
- `position`
- optional per-target overrides

### `alert_silences`

Purpose:

- temporary suppression of notifications

Suggested fields:

- `id`
- `match_json`
- `starts_at`
- `ends_at`
- `reason`
- `created_at`

### `alert_source_events`

Purpose:

- durable handoff from ingest/persistence into alert evaluation

Suggested fields:

- `id`
- `source_kind`
- `source_id`
- `payload_json`
- `source_timestamp`
- `created_at`
- `claimed_at`
- `processed_at`

### `alert_events`

Purpose:

- logical alert firings after evaluation, silence checks, and dedupe

Suggested fields:

- `id`
- `rule_id`
- `source_event_id`
- `event_phase`
- `dedupe_key`
- `severity`
- `labels_json`
- `title`
- `body`
- `payload_json`
- `created_at`

### `alert_delivery_attempts`

Purpose:

- durable per-target delivery tracking

Suggested fields:

- `id`
- `alert_event_id`
- `contact_point_id`
- `attempt_no`
- `status`
- `next_retry_at`
- `response_code`
- `response_excerpt`
- `delivered_at`
- `created_at`
- `updated_at`

## Rule Model

## Canonical Rule Shape

There should be one stored rule format.

It should contain:

- rule identity and metadata
- source kind
- criteria expression
- trigger policy
- template definition
- target bindings

### Source Kinds

Initial source kinds:

- `product_available`
- `incident_change`

Later source kinds could include:

- archive aggregate threshold events
- periodic digest sources

### Criteria

V1 criteria should reuse the existing typed field model already present in the codebase.

For products, this includes:

- filename
- source
- family
- artifact kind
- office
- header fields
- issue fields
- VTEC fields
- UGC geography
- HVTEC fields
- wind and hail thresholds
- point-radius and bounding-box filters
- payload size bounds

For incidents, this includes:

- office
- phenomena
- significance
- ETN
- current status
- action
- active window predicates

### Basic Mode

Basic mode should present the existing filterable weather concepts directly:

- "alert on tornado warnings"
- "alert on flash flood warnings in these counties"
- "alert on hail size >= 2.0 inches"
- "alert on incident updates from office OAX"

### Advanced Mode

Advanced mode should add compositional logic over the same typed field model:

- `all`
- `any`
- `not`
- grouped conditions
- temporal windows and thresholds where justified

V1 does not require a generic JSONLogic engine.
The typed model is simpler, more auditable, and more consistent with the existing repository.

## Trigger Policy

Each rule needs explicit trigger-policy settings.

Suggested policy fields:

- `cooldown`
- `repeat_interval`
- optional `max_repeat_count`
- optional `all_clear_duration`
- optional rule-specific severity

For V1, keep phases simple:

- `fire`

Later phases can add:

- `all_clear`
- scheduled digests
- escalation paths

## Contact Point Model

V1 should support two kinds of contact points.

### `apprise`

Use for:

- Slack
- Teams
- Discord
- Telegram
- ntfy
- Gotify
- email-style destinations
- other Apprise-supported targets

The worker sends rendered title/body and per-event metadata to Apprise.

`emwin-rs` remains the source of truth for:

- contact-point records
- rule bindings
- secrets
- audit history

Apprise should not become the primary configuration database.

### `webhook`

Use for:

- stable first-party machine-readable delivery
- customer integrations that need a documented contract
- signed payload verification

Webhook delivery should be native to `emwin-rs`.

## Delivery Implementation

### HTTP Stack

Use:

- `reqwest`
- `reqwest-middleware`
- `reqwest-retry`

These libraries give the worker:

- connection pooling
- explicit timeout control
- middleware composition
- bounded retry with exponential backoff and jitter

### Retry Classification

Retry only on transient failures:

- network failures
- connect and timeout failures
- `408`
- `429`
- `5xx`

Do not retry most `4xx`.

The worker should respect `Retry-After` when provided.

Retry counts must remain low.

### Streaming-Body Constraint

`reqwest-retry` requires cloneable requests and explicitly fails on streaming bodies.

That is acceptable for this subsystem because V1 notification payloads should be small in-memory JSON or text payloads.

Do not design V1 delivery around streaming request bodies.

## Webhook Contract

Webhook delivery should include:

- stable `delivery_key`
- event metadata
- rule metadata
- source metadata
- rendered message fields
- normalized weather payload snapshot

Suggested headers:

- `X-Emwin-Alert-Id`
- `X-Emwin-Delivery-Key`
- `X-Emwin-Contact-Point-Id`
- `X-Emwin-Signature`

Signature algorithm:

- HMAC SHA-256

The signature input should be deterministic and documented.

## API Surface

The alerting API belongs under `/v1/alerting`.

### Contact Points

- `GET /v1/alerting/contact-points`
- `POST /v1/alerting/contact-points`
- `GET /v1/alerting/contact-points/{id}`
- `PATCH /v1/alerting/contact-points/{id}`
- `DELETE /v1/alerting/contact-points/{id}`
- `POST /v1/alerting/contact-points/{id}/test`

### Rules

- `GET /v1/alerting/rules`
- `POST /v1/alerting/rules`
- `POST /v1/alerting/rules/simulate`
- `GET /v1/alerting/rules/{id}`
- `PATCH /v1/alerting/rules/{id}`
- `DELETE /v1/alerting/rules/{id}`
- `POST /v1/alerting/rules/{id}/simulate`

### Audit Reads

- `GET /v1/alerting/rules/{id}/events`
- `GET /v1/alerting/deliveries`

### Silences

- `GET /v1/alerting/silences`
- `POST /v1/alerting/silences`
- `DELETE /v1/alerting/silences/{id}`

Operational notes:

- `/v1/alerting/*` is available only when the server is running with Postgres-backed archive persistence
- `POST /v1/alerting/contact-points/{id}/test` uses direct webhook delivery or Apprise delivery from the API process
- Apprise test sends require `--alerting-apprise-api-url` or `EMWIN_APPRISE_API_URL` on `emwin-cli server`
- persisted simulation rejects incident windows earlier than the first retained `incident_change` source event

## MCP Control Plane

Exposing alerting configuration through MCP does not change the alert runtime architecture.
It changes the control-plane contract.

The MCP surface should be treated as an AI-facing interface for the same alerting subsystem, not as a second independent configuration path.

### Design Rules

- MCP must use the same canonical rule model as the REST API and UI
- MCP must call the same validation, simulation, and persistence logic as the REST API
- MCP must not bypass confirmation checks, secret redaction, or audit logging
- MCP must be outcome-oriented rather than a thin wrapper over raw CRUD endpoints
- MCP should stay in the alerting bounded context only

### Required Domain Capabilities

Adding MCP makes these capabilities mandatory rather than optional:

- draft a rule from structured intent
- validate a rule before saving it
- simulate a rule against archive data before saving it
- explain why a rule matched, failed validation, or was considered too broad
- redact secret-bearing fields from all read paths

### Recommended MCP Workflow

The expected AI-assisted flow is:

1. draft a candidate rule or contact-point configuration
2. validate the candidate
3. simulate the candidate over a requested time window
4. present the result and explanation to the user
5. persist only after explicit confirmation

The system should not allow a vague natural-language request to go directly to persistent rule creation without validation and simulation.

### Recommended MCP Tools

Keep the MCP server small and outcome-oriented.

Recommended tools:

- `list_rules`
- `get_rule`
- `draft_rule`
- `validate_rule`
- `simulate_rule`
- `create_rule`
- `update_rule`
- `list_contact_points`
- `create_contact_point`
- `test_contact_point`
- `create_silence`
- `delete_rule` with explicit `confirm: true`
- `delete_silence` with explicit `confirm: true`

Do not expose database-shaped tools or internal queue-management tools.

### Schema Conventions

MCP inputs should be flatter and stricter than the internal storage model.

Requirements:

- prefer flat fields and enums over deeply nested free-form objects
- constrain untrusted strings by length and pattern where practical
- make destructive operations require an explicit `confirm: true`
- return concise typed summaries instead of raw internal records
- paginate list-style results

### Safety Model

MCP increases the risk of accidental bad configurations, so the control plane needs explicit safeguards.

Required safeguards:

- never return secrets, tokens, or webhook signing material
- separate read-only and mutating MCP capabilities
- record scrubbed audit logs for each MCP tool invocation
- return specific validation errors instead of generic failures
- reject ambiguous or unsupported criteria rather than guessing

### Transport and Placement

This repository does not currently contain an MCP server implementation.

If MCP is added, the recommended placement is:

- a separate HTTP MCP service backed by shared alerting domain logic, or
- a clearly isolated MCP router in the API process backed by the same domain logic

In either case:

- the MCP surface belongs on the control plane
- the alert worker remains unchanged
- rule evaluation and delivery stay outside the MCP server

The important boundary is not the process count by itself.
The important boundary is that MCP must not become a parallel implementation of alerting rules, validation, or delivery.

## Simulation

Rule simulation is required.

Without it, operators will create noisy or broken rules blindly.

Simulation should:

- run a proposed rule against archive data over a chosen time window
- report total match count
- report sample matches
- estimate expected notification volume

The simulation result should make obvious when a rule is too broad.

## Frontend Scope

This repository does not currently contain a web frontend package, so the alerting UI is net-new.

Minimum screens:

- rule list
- rule builder
- contact point list and editor
- test-send dialog
- deliveries audit view
- silences view
- simulation preview

Basic and advanced rule builders must edit the same saved rule object.

## Security

### Secrets

Use secret-wrapping types for:

- webhook signing secrets
- Apprise tokens or embedded credentials
- future API keys

Do not echo secrets back through API reads.

### Logging

Never log:

- raw credentials
- raw signed payload secrets
- complete response bodies from downstream services unless explicitly redacted
- full MCP tool arguments when they contain secrets or high-noise user text

### Auth

Alerting APIs should reuse the existing bearer-auth model already present on `/v1/*`.

If MCP is added, it should use a distinct auth scope or credential set from general read-only API clients.

## Operational Model

### Scaling

Start with one worker process.

Horizontal scaling can be added by:

- durable row claiming
- bounded worker concurrency
- idempotent event creation rules

### Backpressure

The worker must use bounded concurrency for outbound sends.

Queue growth should be visible through metrics and audit queries.

### Health

The system should surface:

- pending source events
- pending delivery attempts
- failed deliveries
- oldest undelivered attempt age
- Apprise reachability

## Recommended Crate and Library Choices

### Recommended

- `reqwest`
- `reqwest-middleware`
- `reqwest-retry`
- `secrecy`
- `minijinja`
- `wiremock`
- `hmac`
- `sha2`
- `jsonschema`

### Deferred

- generic rules-engine crates
- cron schedulers
- vendor-specific notification SDKs

## Phased Rollout

### Phase 1

- add schema
- add alert-worker skeleton
- persist source events
- implement webhook contact points only

### Phase 2

- add Apprise contact points
- add retry and audit surfaces
- add simulation endpoint

### Phase 3

- build UI
- add richer dedupe and silence tooling
- add MCP control-plane surface backed by shared alerting domain logic

### Phase 4

- add all-clear phases
- add digesting or escalation if still justified

## Open Questions

- whether product-derived alert source events should be written directly from the live runtime or from the durable archive persistence boundary
- whether incident-change source events should be derived from the same persistence transaction that updates the incident projection
- whether V1 should include point-radius saved scopes as a first-class reusable object or leave all geometry inline in rules
- whether MCP should be deployed as a separate HTTP service from `emwin-api` or as an isolated router within the same process

## Recommendation

Proceed with:

- a separate `alert-worker` process or container
- Postgres-backed durable source-event and delivery state
- `reqwest` plus `reqwest-middleware` and `reqwest-retry`
- Apprise for broad notification support
- native signed webhooks for the stable machine-readable contract

This is the smallest design that is operationally serious.

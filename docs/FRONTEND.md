# Frontend Functional Design for Interactive Severe Weather Experience

## Purpose

This document defines the frontend design and interaction model for a future severe-weather application built on top of `emwin-rs`.

The frontend should communicate:

- the big-picture weather situation
- what is active now
- where risk is concentrating
- what is likely to intensify
- what products and evidence support that conclusion

This is not a file browser.
This is not a scrolling event console.
This should behave like an operational weather intelligence workspace.

## Product Positioning

Primary users:

- emergency management and operations users
- weather-aware analysts
- technically fluent public-sector or infrastructure users

Primary question set:

- What is happening now?
- Where are the hotspots?
- What is intensifying?
- What kind of severe weather is involved?
- What evidence supports this?
- What changed in the last few minutes or hours?

## Core Design Principles

- Map-first, not table-first
- Incident-first, not raw-product-first
- Show guidance, active warnings, and observed impacts as separate layers
- Show confidence and data quality explicitly
- Use progressive disclosure
  - national view
  - hotspot
  - incident
  - product
  - raw detail

## Information Architecture

### 1. Situation Overview

The landing view should be a national or regional map.

It should show:

- active incident footprints
- active watch polygons
- SPC outlook areas
- ERO areas
- MCD polygons
- LSR report points or clusters
- optional radar and satellite graphic overlays when available

It should also show a compact top-level summary ribbon:

- active incidents by hazard type
- new incidents in rolling window
- updated incidents in rolling window
- active watches
- LSR count
- high-severity wind/hail count
- top offices or states by activity

### 2. Hotspot Exploration

The map must support hotspot discovery.

Hotspots can be represented as:

- clusters
- hex bins
- ranked regions
- state summaries

Selecting a hotspot should open a side panel showing:

- active incidents in that area
- most common hazard types
- latest actions
- most recent significant products
- observed reports vs forecast/outlook products

### 3. Incident Workspace

Incident detail should be the main drill-down state.

For one incident, show:

- incident identity
  - office
  - VTEC phenomena
  - significance
  - ETN
- current status
- latest action
- issued/start/end/last-updated times
- latest geometry and affected geography
- incident product timeline
- related parsed hazard data
- issue/QC status

### 4. Product Inspector

Product detail should be a secondary drill-down from incidents and hotspots.

For one product, show:

- product family
- artifact kind
- title
- source
- office
- header metadata
- parsed body or artifact
- linked geometry
- linked raw payload
- parser issues and affected lines when present

## Required Primary Screens

### Overview Screen

This is the default screen.

Layout:

- top trend ribbon
- dominant map canvas
- collapsible live activity rail
- collapsible filter tray

Primary interactions:

- hover for lightweight summary
- click to pin hotspot or incident
- scrub time window
- toggle map layers
- pause/resume live mode

### Incident Detail Screen

This can be a routed page or a large docked workspace panel.

Layout:

- incident header
- map inset
- product timeline
- evidence tabs
  - warning geometry
  - UGC areas
  - motion
  - reports
  - issues

### Product Detail Screen

This should be a technical inspector.

Layout:

- metadata header
- parsed summary block
- geometry panel
- structured detail panel
- raw payload access
- issue panel

## Visual Communication Model

The design should separate these classes clearly:

- Active warning/advisory geometry
  - strongest emphasis
- Watch products
  - secondary emphasis
- Forecast/outlook guidance
  - broader, softer visual treatment
- Observed impacts
  - point-based and density-based treatment
- Technical or low-urgency products
  - low emphasis

### Recommended Layer Semantics

- Warnings and incident geometry:
  - saturated, high-contrast outlines and fills
- Watches:
  - bold but distinct from warnings
- Outlook guidance:
  - wider translucent areas
- LSR reports:
  - point symbols with clustering at low zoom
- Motion vectors:
  - directional linework, only when selected
- Quality issues:
  - amber or red badges, never hidden

### Recommended Big-Picture Framing

The overview should read as:

- “current active risk”
- “near-term expansion risk”
- “observed impact concentration”

Do not mix those into one undifferentiated symbol layer.

## Interaction Details

### Live Mode

The UI should support a live mode driven by SSE.

Requirements:

- show connection state
- show last update time
- allow pause without losing context
- allow resume to current live state
- surface lag/drop warnings from SSE

### Filters

Filters should be deep-linkable in the URL.

Minimum filters:

- event type
- office
- state
- family
- VTEC phenomena
- VTEC significance
- watch type
- issue presence
- issue code
- wind/hail threshold
- time window
- map location or point-radius

### Time Navigation

The experience should support:

- live rolling mode
- recent-window mode such as 15m, 1h, 3h, 12h, 24h
- incident timeline stepping

Do not require a full historical time machine in the first version unless the backend adds archive search and summary endpoints.

### Drill-Down Flow

Required drill-down path:

1. map hotspot or region
2. incident summary
3. incident timeline
4. product detail
5. raw payload or issue detail

The user should never feel forced to open raw text just to understand severity or geography.

## Data Use Rules

### Use Summary First

Use summary payloads for:

- map markers
- cluster counts
- ribbons
- lists
- filter chips

Only fetch archive product detail when:

- the user selects a product
- the user opens an incident timeline entry
- the UI needs full parsed artifact detail

### Treat Geometry as Optional

The frontend must handle these cases:

- polygon present
- only point data present
- only UGC geography present
- only office/state context present
- outlook present without usable polygon detail

Fallback order:

1. polygon
2. point or track
3. UGC-derived representative geography
4. incident metadata only

### Show Data Quality

Each incident and product should expose:

- parser issue count
- issue code summary
- warnings when geometry is partial
- warnings when reference time is missing or derived parsing is degraded

Do not silently suppress problematic products.

## Product-Type-Specific Presentation

### VTEC Generic Products

Show:

- hazard label
- action
- office
- significance
- ETN
- polygon
- UGC areas
- HVTEC where present
- motion vectors
- wind/hail tags

### LSR

Show as observed reports.

Use:

- point symbols
- event text
- magnitude
- source
- remarks

### MCD

Show as near-term mesoscale guidance.

Use:

- polygon
- areas affected
- concerning
- watch issuance probability
- most probable tornado/hail/gust tags

### SPC Outlook

Show as broad forecast risk layers.

Use:

- category
- threshold
- day
- outlook type
  - convective or fire weather

### ERO

Show as hydrologic guidance layers.

Use:

- day
- threshold
- categorical risk

### SAW / SEL / WWP

Use these to explain watch lifecycle and confidence.

Show:

- watch number
- watch type
- issue/cancel state
- replacement watch linkage
- watch probability details
- PDS state

## Map and Spatial UX Requirements

- Support national and regional scales cleanly
- Cluster dense point products
- Avoid rendering every low-level event symbol at wide zoom
- Keep selected incident geometry visually dominant
- Allow layer toggles without hiding the base narrative
- Preserve selection while live data updates continue

## Performance Requirements

- Virtualize large side-panel lists
- Incrementally render live updates
- Avoid full-map rerenders for each SSE event
- Batch event ingestion into short visual update windows
- Lazy-load archive detail

## Accessibility and Usability Requirements

- Keyboard navigable filter controls
- Visible focus states
- Color is not the only severity signal
- Provide text labels and legends for hazard categories
- Mobile support required for read-only monitoring workflows
- Desktop is the primary authoring and analysis surface

## Aesthetic Direction

The visual system should feel like an operations-grade weather wall, not a SaaS admin dashboard.

Recommended direction:

- editorial and cartographic rather than card-heavy
- disciplined typography
- restrained but strong color hierarchy
- large map-first composition
- animated updates only where they add meaning

Avoid:

- generic cards as the main organizational device
- purple-gradient SaaS styling
- neon threat colors on dark backgrounds without hierarchy
- cluttered legends and persistent chrome

## MVP Scope

Approve this scope for first delivery:

- live overview map
- filterable live incident list
- hotspot side panel
- incident detail workspace
- product inspector
- issue visibility
- basic trend ribbon from available live and incident data

## Scope That Requires Backend Expansion

Do not promise these in the first version unless the backend is extended:

- true historical archive exploration across all products
- durable hotspot analytics across arbitrary time windows
- nationwide trend charts driven by server-side summaries
- fully normalized map-layer APIs for every product family

## Approval

Approve a frontend built to this document.

Reject any implementation that starts with:

- a raw product table homepage
- an event-log homepage
- a generic analytics dashboard shell

The correct shape is a map-first severe-weather intelligence application with incident-first drill-down.

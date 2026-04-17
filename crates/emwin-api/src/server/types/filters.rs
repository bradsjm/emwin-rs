use super::payloads::{EventKind, IncidentEventPayload};
use super::query::{EventsQuery, IncidentEventsQuery};
use emwin_service::IncidentChangeAction;
use emwin_service::{FileEventFilter, FileFilterInput};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct EventFilter {
    pub(crate) event_names: Option<BTreeSet<String>>,
    pub(crate) file: FileEventFilter,
}

impl EventFilter {
    #[cfg(test)]
    pub(crate) fn from_query(query: EventsQuery) -> Self {
        Self::try_from_query(query).expect("query should compile")
    }

    pub(crate) fn try_from_query(query: EventsQuery) -> Result<Self, EventFilterQueryError> {
        let event_names = csv_values(query.event.as_deref(), normalize_lower);
        let file_input = FileFilterInput::from(query);
        let file =
            FileEventFilter::try_from_input(&file_input).map_err(|err| EventFilterQueryError {
                message: err.message,
            })?;

        Ok(Self { event_names, file })
    }

    pub(crate) fn matches(&self, event: &EventKind) -> bool {
        if let Some(event_names) = &self.event_names {
            let event_name = normalize_lower(event.event_name());
            if !event_names.contains(&event_name) {
                return false;
            }
        }

        if !self.file.has_constraints() {
            return true;
        }

        match event {
            EventKind::FileComplete(file) => self.file.matches_metadata(&file.metadata),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EventFilterQueryError {
    pub(crate) message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct IncidentEventFilter {
    pub(crate) actions: Option<BTreeSet<String>>,
    pub(crate) offices: Option<BTreeSet<String>>,
    pub(crate) phenomena: Option<BTreeSet<String>>,
    pub(crate) significance: Option<BTreeSet<String>>,
    pub(crate) statuses: Option<BTreeSet<String>>,
    pub(crate) etns: Option<BTreeSet<i64>>,
}

impl IncidentEventFilter {
    pub(crate) fn from_query(query: IncidentEventsQuery) -> Self {
        Self {
            actions: csv_values(query.action.as_deref(), normalize_lower),
            offices: csv_values(query.office.as_deref(), normalize_upper),
            phenomena: csv_values(query.phenomena.as_deref(), normalize_upper),
            significance: csv_values(query.significance.as_deref(), normalize_upper),
            statuses: csv_values(query.status.as_deref(), normalize_lower),
            etns: csv_i64_values(query.etn.as_deref()),
        }
    }

    pub(crate) fn matches(&self, event: &IncidentEventPayload) -> bool {
        if let Some(actions) = &self.actions
            && !actions
                .contains(normalize_lower(incident_change_action_name(event.action)).as_str())
        {
            return false;
        }
        if let Some(offices) = &self.offices
            && !offices.contains(event.incident.office.as_str())
        {
            return false;
        }
        if let Some(phenomena) = &self.phenomena
            && !phenomena.contains(event.incident.phenomena.as_str())
        {
            return false;
        }
        if let Some(significance) = &self.significance
            && !significance.contains(event.incident.significance.as_str())
        {
            return false;
        }
        if let Some(statuses) = &self.statuses
            && !statuses.contains(event.incident.current_status.as_str())
        {
            return false;
        }
        if let Some(etns) = &self.etns
            && !etns.contains(&event.incident.etn)
        {
            return false;
        }
        true
    }
}

fn csv_values(raw: Option<&str>, normalize: fn(&str) -> String) -> Option<BTreeSet<String>> {
    let values = raw
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(normalize)
        .collect::<BTreeSet<_>>();

    (!values.is_empty()).then_some(values)
}

fn normalize_lower(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn normalize_upper(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

fn incident_change_action_name(action: IncidentChangeAction) -> &'static str {
    match action {
        IncidentChangeAction::Created => "created",
        IncidentChangeAction::Updated => "updated",
    }
}

fn csv_i64_values(raw: Option<&str>) -> Option<BTreeSet<i64>> {
    let values = raw
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter_map(|value| value.parse::<i64>().ok())
        .collect::<BTreeSet<_>>();
    (!values.is_empty()).then_some(values)
}

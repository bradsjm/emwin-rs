use super::shared::normalize_lower;
use emwin_parser::ProductParseIssue;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct IssueFilter {
    pub(crate) has_issues: Option<bool>,
    pub(crate) kinds: Option<BTreeSet<String>>,
    pub(crate) codes: Option<BTreeSet<String>>,
}

impl IssueFilter {
    pub(crate) fn has_constraints(&self) -> bool {
        self.has_issues.is_some() || self.kinds.is_some() || self.codes.is_some()
    }

    pub(crate) fn matches(&self, issues: &[ProductParseIssue]) -> bool {
        if let Some(has_issues) = self.has_issues
            && has_issues == issues.is_empty()
        {
            return false;
        }

        if let Some(kinds) = &self.kinds
            && !issues
                .iter()
                .any(|issue| kinds.contains(&normalize_lower(issue.kind)))
        {
            return false;
        }

        if let Some(codes) = &self.codes
            && !issues
                .iter()
                .any(|issue| codes.contains(&normalize_lower(issue.code)))
        {
            return false;
        }

        true
    }
}

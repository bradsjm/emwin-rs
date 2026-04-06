#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SizeRange {
    pub(crate) min: Option<usize>,
    pub(crate) max: Option<usize>,
}

impl SizeRange {
    pub(crate) fn has_constraints(&self) -> bool {
        self.min.is_some() || self.max.is_some()
    }

    pub(crate) fn matches(&self, size: usize) -> bool {
        if let Some(min) = self.min
            && size < min
        {
            return false;
        }
        if let Some(max) = self.max
            && size > max
        {
            return false;
        }
        true
    }
}

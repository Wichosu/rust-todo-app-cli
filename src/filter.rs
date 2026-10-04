use chrono::Local;

use crate::todo::Priority;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StatusFilter {
    All,
    Pending,
    Completed,
}

impl StatusFilter {
    pub fn completed(self) -> Option<bool> {
        match self {
            StatusFilter::All => None,
            StatusFilter::Pending => Some(false),
            StatusFilter::Completed => Some(true),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            StatusFilter::All => "All",
            StatusFilter::Pending => "Pending",
            StatusFilter::Completed => "Completed",
        }
    }

    pub fn index(self) -> usize {
        match self {
            StatusFilter::All => 0,
            StatusFilter::Pending => 1,
            StatusFilter::Completed => 2,
        }
    }

    pub fn from_index(i: usize) -> StatusFilter {
        match i {
            1 => StatusFilter::Pending,
            2 => StatusFilter::Completed,
            _ => StatusFilter::All,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DateFilter {
    All,
    DueToday,
    Overdue,
    Before,
    After,
}

impl DateFilter {
    pub fn label(self) -> &'static str {
        match self {
            DateFilter::All => "All",
            DateFilter::DueToday => "Due Today",
            DateFilter::Overdue => "Overdue",
            DateFilter::Before => "Before",
            DateFilter::After => "After",
        }
    }

    pub fn index(self) -> usize {
        match self {
            DateFilter::All => 0,
            DateFilter::DueToday => 1,
            DateFilter::Overdue => 2,
            DateFilter::Before => 3,
            DateFilter::After => 4,
        }
    }

    pub fn from_index(i: usize) -> DateFilter {
        match i {
            1 => DateFilter::DueToday,
            2 => DateFilter::Overdue,
            3 => DateFilter::Before,
            4 => DateFilter::After,
            _ => DateFilter::All,
        }
    }

    pub fn needs_value(self) -> bool {
        matches!(self, DateFilter::Before | DateFilter::After)
    }
}

pub fn priority_index(p: Option<Priority>) -> usize {
    match p {
        None => 0,
        Some(Priority::Low) => 1,
        Some(Priority::Medium) => 2,
        Some(Priority::High) => 3,
    }
}

pub fn priority_from_index(i: usize) -> Option<Priority> {
    match i {
        0 => None,
        1 => Some(Priority::Low),
        2 => Some(Priority::Medium),
        _ => Some(Priority::High),
    }
}

pub fn cycle_index(current: usize, dir: i32, len: usize) -> usize {
    (current as i32 + dir).rem_euclid(len as i32) as usize
}

pub fn today() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

#[derive(Clone)]
pub struct TodoFilter {
    pub status: StatusFilter,
    pub priority: Option<Priority>,
    pub date: DateFilter,
    pub date_value: String,
}

impl TodoFilter {
    pub fn completed(&self) -> Option<bool> {
        self.status.completed()
    }

    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.status != StatusFilter::All {
            parts.push(self.status.label().to_string());
        }
        if let Some(p) = self.priority {
            parts.push(p.to_string());
        }
        match self.date {
            DateFilter::All => {}
            DateFilter::Before | DateFilter::After => {
                parts.push(format!("{} {}", self.date.label(), self.date_value));
            }
            d => parts.push(d.label().to_string()),
        }
        if parts.is_empty() {
            "All".to_string()
        } else {
            parts.join(" · ")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_index_round_trip() {
        for i in 0..3 {
            assert_eq!(StatusFilter::from_index(StatusFilter::from_index(i).index()).index(), i);
        }
        assert_eq!(StatusFilter::from_index(99), StatusFilter::All);
    }

    #[test]
    fn date_index_round_trip() {
        for i in 0..5 {
            assert_eq!(DateFilter::from_index(DateFilter::from_index(i).index()).index(), i);
        }
        assert_eq!(DateFilter::from_index(99), DateFilter::All);
    }

    #[test]
    fn status_completed_mapping() {
        assert_eq!(StatusFilter::All.completed(), None);
        assert_eq!(StatusFilter::Pending.completed(), Some(false));
        assert_eq!(StatusFilter::Completed.completed(), Some(true));
    }

    #[test]
    fn cycle_wraps_both_directions() {
        assert_eq!(cycle_index(0, -1, 5), 4);
        assert_eq!(cycle_index(4, 1, 5), 0);
        assert_eq!(cycle_index(2, 1, 5), 3);
        assert_eq!(cycle_index(2, -1, 5), 1);
        assert_eq!(cycle_index(0, 1, 3), 1);
    }

    #[test]
    fn priority_index_round_trip() {
        assert_eq!(priority_index(None), 0);
        assert_eq!(priority_from_index(0), None);
        assert_eq!(priority_from_index(priority_index(Some(Priority::Low))), Some(Priority::Low));
        assert_eq!(
            priority_from_index(priority_index(Some(Priority::Medium))),
            Some(Priority::Medium)
        );
        assert_eq!(
            priority_from_index(priority_index(Some(Priority::High))),
            Some(Priority::High)
        );
    }

    #[test]
    fn needs_value_only_before_after() {
        assert!(!DateFilter::All.needs_value());
        assert!(!DateFilter::DueToday.needs_value());
        assert!(!DateFilter::Overdue.needs_value());
        assert!(DateFilter::Before.needs_value());
        assert!(DateFilter::After.needs_value());
    }

    fn base_filter() -> TodoFilter {
        TodoFilter {
            status: StatusFilter::All,
            priority: None,
            date: DateFilter::All,
            date_value: String::new(),
        }
    }

    #[test]
    fn label_defaults_to_all() {
        assert_eq!(base_filter().label(), "All");
    }

    #[test]
    fn label_single_dimensions() {
        let mut f = base_filter();
        f.status = StatusFilter::Pending;
        assert_eq!(f.label(), "Pending");

        let mut f = base_filter();
        f.priority = Some(Priority::High);
        assert_eq!(f.label(), "H");

        let mut f = base_filter();
        f.date = DateFilter::Overdue;
        assert_eq!(f.label(), "Overdue");
    }

    #[test]
    fn label_combines_dimensions() {
        let mut f = base_filter();
        f.status = StatusFilter::Pending;
        f.priority = Some(Priority::High);
        assert_eq!(f.label(), "Pending · H");

        let mut f = base_filter();
        f.status = StatusFilter::Completed;
        f.date = DateFilter::Before;
        f.date_value = "2026-01-02".to_string();
        assert_eq!(f.label(), "Completed · Before 2026-01-02");

        let mut f = base_filter();
        f.priority = Some(Priority::Low);
        f.date = DateFilter::After;
        f.date_value = "2026-03-04".to_string();
        assert_eq!(f.label(), "L · After 2026-03-04");
    }
}

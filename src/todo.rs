use std::{error::Error, fmt, str::FromStr};

use rusqlite::{
    ToSql,
    types::{FromSql, FromSqlError},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: i64,
    pub text: String,
    pub completed: bool,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub priority: Priority,
    pub due_date: Option<String>,
    pub repeat: Repeat,
}

fn sort_key(t: &Todo) -> u8 {
    if t.completed {
        return 3;
    }
    match t.priority {
        Priority::High => 0,
        Priority::Medium => 1,
        Priority::Low => 2,
    }
}

pub fn sort_todos(todos: &mut [Todo]) {
    todos.sort_by_key(sort_key);
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Low,
    Medium,
    High,
}

impl Priority {
    // pub fn from_str(value: &str) -> Self {
    //     match value {
    //         "low" => Priority::Low,
    //         "medium" => Priority::Medium,
    //         "high" => Priority::High,
    //         _ => Priority::Medium,
    //     }
    // }

    pub fn as_str(&self) -> &str {
        match self {
            Priority::Low => "low",
            Priority::Medium => "medium",
            Priority::High => "high",
        }
    }
}

#[derive(Debug)]
pub struct ParsePriorityError {
    input: String,
}

impl Error for ParsePriorityError {}

impl fmt::Display for ParsePriorityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "\"{}\" is an Invalid priority (expected: low, medium, high)",
            self.input
        )
    }
}

impl FromStr for Priority {
    type Err = ParsePriorityError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "low" => Ok(Priority::Low),
            "medium" => Ok(Priority::Medium),
            "high" => Ok(Priority::High),
            _ => Err(ParsePriorityError {
                input: s.to_string(),
            }),
        }
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let symbol = match self {
            Priority::Low => "L",
            Priority::Medium => "M",
            Priority::High => "H",
        };

        write!(f, "{symbol}")
    }
}

impl FromSql for Priority {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let text = value.as_str()?;

        text.parse::<Priority>()
            .map_err(|_err| FromSqlError::InvalidType)
    }
}

impl ToSql for Priority {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        self.as_str().to_sql()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    #[default]
    None,
    Daily,
    Weekly,
    Monthly,
}

impl Repeat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Repeat::None => "none",
            Repeat::Daily => "daily",
            Repeat::Weekly => "weekly",
            Repeat::Monthly => "monthly",
        }
    }
}

#[derive(Debug)]
pub struct ParseRepeatError {
    input: String,
}

impl Error for ParseRepeatError {}

impl fmt::Display for ParseRepeatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "\"{}\" is an Invalid repeat (expected: none, daily, weekly, monthly)",
            self.input
        )
    }
}

impl FromStr for Repeat {
    type Err = ParseRepeatError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(Repeat::None),
            "daily" => Ok(Repeat::Daily),
            "weekly" => Ok(Repeat::Weekly),
            "monthly" => Ok(Repeat::Monthly),
            _ => Err(ParseRepeatError {
                input: s.to_string(),
            }),
        }
    }
}

impl fmt::Display for Repeat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromSql for Repeat {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let text = value.as_str()?;

        text.parse::<Repeat>()
            .map_err(|_err| FromSqlError::InvalidType)
    }
}

impl ToSql for Repeat {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        self.as_str().to_sql()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_priorities() {
        assert_eq!("low".parse::<Priority>().unwrap(), Priority::Low);
        assert_eq!("medium".parse::<Priority>().unwrap(), Priority::Medium);
        assert_eq!("high".parse::<Priority>().unwrap(), Priority::High);
    }

    #[test]
    fn parse_invalid_priority_reports_input() {
        let err = "bogus".parse::<Priority>().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("bogus"));
        assert!(msg.contains("low, medium, high"));
    }

    #[test]
    fn display_priority_symbol() {
        assert_eq!(Priority::Low.to_string(), "L");
        assert_eq!(Priority::Medium.to_string(), "M");
        assert_eq!(Priority::High.to_string(), "H");
    }

    #[test]
    fn as_str_is_lowercase() {
        assert_eq!(Priority::Low.as_str(), "low");
        assert_eq!(Priority::Medium.as_str(), "medium");
        assert_eq!(Priority::High.as_str(), "high");
    }

    fn todo(priority: Priority, completed: bool) -> Todo {
        Todo {
            id: 0,
            text: String::new(),
            completed,
            created_at: String::new(),
            completed_at: None,
            priority,
            due_date: None,
            repeat: Repeat::None,
        }
    }

    #[test]
    fn sort_orders_by_priority_then_completed() {
        let mut todos = vec![
            todo(Priority::Low, false),
            todo(Priority::High, false),
            todo(Priority::Medium, true),
            todo(Priority::Medium, false),
            todo(Priority::Low, true),
            todo(Priority::High, false),
        ];
        sort_todos(&mut todos);

        let order: Vec<(Priority, bool)> = todos.iter().map(|t| (t.priority, t.completed)).collect();
        assert_eq!(
            order,
            vec![
                (Priority::High, false),
                (Priority::High, false),
                (Priority::Medium, false),
                (Priority::Low, false),
                (Priority::Medium, true),
                (Priority::Low, true),
            ]
        );
    }

    #[test]
    fn sort_is_stable_for_equal_keys() {
        let mut todos: Vec<Todo> = (0..4).map(|i| {
            let mut t = todo(Priority::Medium, false);
            t.id = i;
            t
        }).collect();
        sort_todos(&mut todos);
        let ids: Vec<i64> = todos.iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![0, 1, 2, 3]);
    }

    #[test]
    fn parse_valid_repeats() {
        assert_eq!("none".parse::<Repeat>().unwrap(), Repeat::None);
        assert_eq!("daily".parse::<Repeat>().unwrap(), Repeat::Daily);
        assert_eq!("weekly".parse::<Repeat>().unwrap(), Repeat::Weekly);
        assert_eq!("monthly".parse::<Repeat>().unwrap(), Repeat::Monthly);
    }

    #[test]
    fn parse_invalid_repeat_reports_input() {
        let err = "yearly".parse::<Repeat>().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("yearly"));
        assert!(msg.contains("none, daily, weekly, monthly"));
    }

    #[test]
    fn repeat_default_is_none() {
        assert_eq!(Repeat::default(), Repeat::None);
    }

    #[test]
    fn repeat_as_str_is_lowercase() {
        assert_eq!(Repeat::None.as_str(), "none");
        assert_eq!(Repeat::Daily.as_str(), "daily");
        assert_eq!(Repeat::Weekly.as_str(), "weekly");
        assert_eq!(Repeat::Monthly.as_str(), "monthly");
        assert_eq!(Repeat::Daily.to_string(), "daily");
    }
}

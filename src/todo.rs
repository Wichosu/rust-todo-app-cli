use std::{error::Error, fmt, str::FromStr};

use rusqlite::{
    ToSql,
    types::{FromSql, FromSqlError},
};

pub struct Todo {
    pub id: i64,
    pub text: String,
    pub completed: bool,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub priority: Priority,
}

#[derive(Debug, Clone, Copy)]
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

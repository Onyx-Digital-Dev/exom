//! Database value parsing utilities
//!
//! Provides error-safe parsing of stored values.

use chrono::{DateTime, Utc};
use rusqlite::Error as SqlError;
use uuid::Uuid;

use crate::models::{HallRole, ParlorId};

/// Parse a UUID from a database string column
pub fn parse_uuid(s: &str) -> Result<Uuid, SqlError> {
    Uuid::parse_str(s).map_err(|e| {
        SqlError::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

/// Parse an optional UUID from a database string column
pub fn parse_uuid_opt(s: Option<String>) -> Result<Option<Uuid>, SqlError> {
    s.map(|s| parse_uuid(&s)).transpose()
}

/// Parse a DateTime from an RFC3339 string
pub fn parse_datetime(s: &str) -> Result<DateTime<Utc>, SqlError> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            SqlError::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })
}

/// Parse an optional DateTime from an RFC3339 string
pub fn parse_datetime_opt(s: Option<String>) -> Result<Option<DateTime<Utc>>, SqlError> {
    s.map(|s| parse_datetime(&s)).transpose()
}

/// Parse an optional ParlorId from a database string column
pub fn parse_parlor_id_opt(s: Option<String>) -> Result<Option<ParlorId>, SqlError> {
    s.map(|s| parse_uuid(&s).map(ParlorId)).transpose()
}

/// Convert a u8 to HallRole
pub fn role_from_u8(value: u8) -> HallRole {
    match value {
        5 => HallRole::HallBuilder,
        4 => HallRole::HallPrefect,
        3 => HallRole::HallModerator,
        2 => HallRole::HallAgent,
        _ => HallRole::HallFellow,
    }
}

/// Extension trait for converting rusqlite Results to Option
pub trait OptionalExt<T> {
    fn optional(self) -> Result<Option<T>, SqlError>;
}

impl<T> OptionalExt<T> for Result<T, SqlError> {
    fn optional(self) -> Result<Option<T>, SqlError> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(SqlError::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

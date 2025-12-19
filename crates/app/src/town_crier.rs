//! Town Crier - Presence announcer bot
//!
//! The first bot in Exom. Announces when members join or leave the hall
//! with calm, neutral messages. No emojis, no roleplay, no Butler voice.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use std::any::Any;

use exom_core::{Bot, BotAction, BotCapability, BotEvent, BotManifest, Database};
use rand::seq::SliceRandom;
use uuid::Uuid;

/// Town Crier bot - announces presence changes
pub struct TownCrier {
    manifest: BotManifest,
    /// Rate limiting: user_id -> last announcement time
    rate_limit: HashMap<Uuid, Instant>,
    /// Database for last_seen lookups
    db: Arc<Mutex<Database>>,
}

impl TownCrier {
    /// Create a new Town Crier bot
    pub fn new(db: Arc<Mutex<Database>>) -> Self {
        Self {
            manifest: BotManifest {
                id: "town-crier".to_string(),
                name: "Town Crier".to_string(),
                version: "1.0.0".to_string(),
                capabilities: vec![BotCapability::ListenPresence, BotCapability::EmitSystem],
            },
            rate_limit: HashMap::new(),
            db,
        }
    }

    /// Check if we should announce for this user (rate limiting)
    fn should_announce(&mut self, user_id: Uuid) -> bool {
        const RATE_LIMIT_SECS: u64 = 10;

        if let Some(last) = self.rate_limit.get(&user_id) {
            if last.elapsed() < Duration::from_secs(RATE_LIMIT_SECS) {
                return false;
            }
        }
        self.rate_limit.insert(user_id, Instant::now());
        true
    }

    /// Generate a join message
    fn join_message(&self, username: &str, is_first_time: bool, last_seen: Option<Duration>) -> String {
        let mut rng = rand::thread_rng();

        if is_first_time {
            // First time join messages
            let templates = [
                format!("{} joined the hall.", username),
                format!("{} is here.", username),
                format!("Welcome, {}.", username),
                format!("{} entered the space.", username),
                format!("First time in this hall: {}.", username),
            ];
            templates.choose(&mut rng).unwrap().clone()
        } else if let Some(duration) = last_seen {
            // Returning user messages
            let duration_str = format_duration(duration);
            let templates = [
                format!("{} is back. Last seen {} ago.", username, duration_str),
                format!("{} returned after {}.", username, duration_str),
                format!("Welcome back, {}. {} since last visit.", username, duration_str),
            ];
            templates.choose(&mut rng).unwrap().clone()
        } else {
            // Fallback to first-time messages if no last_seen data
            let templates = [
                format!("{} joined the hall.", username),
                format!("{} is here.", username),
                format!("Welcome, {}.", username),
            ];
            templates.choose(&mut rng).unwrap().clone()
        }
    }

    /// Generate a leave message
    fn leave_message(&self, username: &str) -> String {
        let mut rng = rand::thread_rng();

        let templates = [
            format!("{} stepped out.", username),
            format!("{} left the hall.", username),
            format!("{} disconnected.", username),
            format!("{} is away.", username),
            format!("{} signed off.", username),
            format!("{} left for now.", username),
            format!("{} has gone offline.", username),
        ];
        templates.choose(&mut rng).unwrap().clone()
    }

    /// Update last_seen timestamp for a user
    fn update_last_seen(&self, hall_id: Uuid, user_id: Uuid) {
        if let Ok(db) = self.db.try_lock() {
            let _ = db.last_seen().update(hall_id, user_id);
        }
    }

    /// Get last_seen duration for a user
    fn get_last_seen_duration(&self, hall_id: Uuid, user_id: Uuid) -> Option<Duration> {
        if let Ok(db) = self.db.try_lock() {
            db.last_seen().get_duration_since(hall_id, user_id).ok().flatten()
        } else {
            None
        }
    }
}

impl Bot for TownCrier {
    fn manifest(&self) -> &BotManifest {
        &self.manifest
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn on_event(&mut self, event: &BotEvent) -> Vec<BotAction> {
        // Check capability
        if !self.should_receive(event) {
            return Vec::new();
        }

        match event {
            BotEvent::MemberJoined {
                hall_id,
                user_id,
                username,
                is_first_time,
                last_seen_duration,
            } => {
                // Rate limit check
                if !self.should_announce(*user_id) {
                    return Vec::new();
                }

                // Generate message
                let message = self.join_message(username, *is_first_time, *last_seen_duration);

                // Update last_seen for next time
                self.update_last_seen(*hall_id, *user_id);

                vec![BotAction::EmitSystem {
                    hall_id: *hall_id,
                    content: message,
                }]
            }
            BotEvent::MemberLeft {
                hall_id,
                user_id,
                username,
            } => {
                // Rate limit check
                if !self.should_announce(*user_id) {
                    return Vec::new();
                }

                // Update last_seen timestamp
                self.update_last_seen(*hall_id, *user_id);

                // Generate message
                let message = self.leave_message(username);

                vec![BotAction::EmitSystem {
                    hall_id: *hall_id,
                    content: message,
                }]
            }
            // Town Crier doesn't handle scheduled ticks
            BotEvent::ScheduledTick { .. } => Vec::new(),
        }
    }
}

/// Format duration in human-readable form
/// Rules:
/// - < 1 min -> "just now"
/// - < 1 hr -> "12 minutes"
/// - < 1 day -> "3 hours"
/// - >= 1 day -> "1 day 11 hours"
/// Never show seconds
fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();

    if secs < 60 {
        return "just now".to_string();
    }

    let minutes = secs / 60;
    let hours = secs / 3600;
    let days = secs / 86400;

    if days >= 1 {
        let remaining_hours = (secs % 86400) / 3600;
        if remaining_hours > 0 {
            if days == 1 {
                format!("1 day {} hours", remaining_hours)
            } else {
                format!("{} days {} hours", days, remaining_hours)
            }
        } else {
            if days == 1 {
                "1 day".to_string()
            } else {
                format!("{} days", days)
            }
        }
    } else if hours >= 1 {
        if hours == 1 {
            "1 hour".to_string()
        } else {
            format!("{} hours", hours)
        }
    } else {
        if minutes == 1 {
            "1 minute".to_string()
        } else {
            format!("{} minutes", minutes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_just_now() {
        assert_eq!(format_duration(Duration::from_secs(0)), "just now");
        assert_eq!(format_duration(Duration::from_secs(30)), "just now");
        assert_eq!(format_duration(Duration::from_secs(59)), "just now");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(Duration::from_secs(60)), "1 minute");
        assert_eq!(format_duration(Duration::from_secs(120)), "2 minutes");
        assert_eq!(format_duration(Duration::from_secs(3599)), "59 minutes");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(Duration::from_secs(3600)), "1 hour");
        assert_eq!(format_duration(Duration::from_secs(7200)), "2 hours");
        assert_eq!(format_duration(Duration::from_secs(86399)), "23 hours");
    }

    #[test]
    fn test_format_duration_days() {
        assert_eq!(format_duration(Duration::from_secs(86400)), "1 day");
        assert_eq!(format_duration(Duration::from_secs(90000)), "1 day 1 hours");
        assert_eq!(format_duration(Duration::from_secs(172800)), "2 days");
        assert_eq!(format_duration(Duration::from_secs(180000)), "2 days 2 hours");  // 2 days + 7200s = 2h
    }
}

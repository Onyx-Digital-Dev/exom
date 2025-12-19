//! Bot runtime - manages bot lifecycle and event dispatch
//!
//! Minimal runtime for first-party bots. Capability enforcement happens here.

use std::sync::Arc;

use chrono::Local;
use exom_core::{Bot, BotAction, BotEvent};
use uuid::Uuid;

use crate::archivist::Archivist;
use crate::state::AppState;
use crate::town_crier::TownCrier;

/// Bot runtime - manages bots and dispatches events
pub struct BotRuntime {
    bots: Vec<Box<dyn Bot>>,
    state: Arc<AppState>,
    /// Index of Archivist bot for command handling
    archivist_index: Option<usize>,
    /// Last scheduled tick time (to avoid duplicate ticks in same minute)
    last_tick_minute: Option<(u16, u16)>, // (hour * 100 + minute, day)
}

impl BotRuntime {
    /// Create a new bot runtime with built-in bots
    pub fn new(state: Arc<AppState>) -> Self {
        let mut runtime = Self {
            bots: Vec::new(),
            state: state.clone(),
            archivist_index: None,
            last_tick_minute: None,
        };

        // Register Town Crier as built-in bot
        let town_crier = TownCrier::new(state.db.clone());
        runtime.register_bot(Box::new(town_crier));

        // Register Archivist as built-in bot
        let archivist = Archivist::new(state.db.clone());
        let archivist_idx = runtime.bots.len();
        runtime.register_bot(Box::new(archivist));
        runtime.archivist_index = Some(archivist_idx);

        runtime
    }

    /// Register a bot
    pub fn register_bot(&mut self, bot: Box<dyn Bot>) {
        tracing::info!(
            bot_id = %bot.manifest().id,
            bot_name = %bot.manifest().name,
            "Registered bot"
        );
        self.bots.push(bot);
    }

    /// Dispatch an event to all bots that can receive it
    pub fn dispatch(&mut self, event: &BotEvent) {
        // Collect all actions first to avoid borrow issues
        let mut all_actions: Vec<(String, BotAction)> = Vec::new();

        for bot in &mut self.bots {
            // Capability check - only deliver if bot has required capability
            if !bot.should_receive(event) {
                continue;
            }

            // Get actions from bot
            let bot_id = bot.manifest().id.clone();
            let actions = bot.on_event(event);

            for action in actions {
                all_actions.push((bot_id.clone(), action));
            }
        }

        // Execute actions after the mutable borrow is released
        for (bot_id, action) in all_actions {
            self.execute_action(bot_id, &action);
        }
    }

    /// Execute a bot action with capability enforcement
    fn execute_action(&self, bot_id: String, action: &BotAction) {
        match action {
            BotAction::EmitSystem { hall_id, content } => {
                // Add ephemeral system message
                self.state.add_system_message(*hall_id, content.clone());
                tracing::debug!(
                    bot_id = %bot_id,
                    hall_id = %hall_id,
                    "Bot emitted system message"
                );
            }
            BotAction::WriteFileToChest {
                hall_id,
                path,
                contents,
            } => {
                // Write file to hall chest
                let chest = self.state.chest.lock().unwrap();
                match chest.write_file(*hall_id, path, contents) {
                    Ok(file_path) => {
                        tracing::info!(
                            bot_id = %bot_id,
                            hall_id = %hall_id,
                            path = %file_path.display(),
                            "Bot wrote file to chest"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            bot_id = %bot_id,
                            hall_id = %hall_id,
                            error = %e,
                            "Bot failed to write file to chest"
                        );
                    }
                }
            }
        }
    }

    /// Handle member joined event
    pub fn on_member_joined(&mut self, hall_id: Uuid, user_id: Uuid, username: String) {
        // Look up last_seen to determine if first time
        let (is_first_time, last_seen_duration) = {
            let db = self.state.db.lock().unwrap();
            match db.last_seen().get_duration_since(hall_id, user_id) {
                Ok(Some(duration)) => (false, Some(duration)),
                Ok(None) => (true, None),
                Err(_) => (true, None),
            }
        };

        let event = BotEvent::MemberJoined {
            hall_id,
            user_id,
            username,
            is_first_time,
            last_seen_duration,
        };

        self.dispatch(&event);
    }

    /// Handle member left event
    pub fn on_member_left(&mut self, hall_id: Uuid, user_id: Uuid, username: String) {
        let event = BotEvent::MemberLeft {
            hall_id,
            user_id,
            username,
        };

        self.dispatch(&event);
    }

    /// Handle a potential slash command from chat
    /// Returns true if the message was a command that was handled
    pub fn handle_command(&mut self, hall_id: Uuid, user_id: Uuid, message: &str) -> bool {
        // Check if it's an archive command
        if message.starts_with("/archive") || message.starts_with("/set-archive") {
            if let Some(idx) = self.archivist_index {
                // Get archivist and handle command
                if let Some(archivist) = self.bots.get_mut(idx) {
                    // Downcast to Archivist
                    if let Some(archivist) = archivist.as_any_mut().downcast_mut::<Archivist>() {
                        if let Some(action) = archivist.handle_command(hall_id, message, user_id) {
                            self.execute_action("archivist".to_string(), &action);
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Dispatch scheduled tick to bots
    /// Called periodically (e.g., every minute) by the app
    pub fn tick_scheduled(&mut self, hall_id: Uuid) {
        let now = Local::now();
        let current_hhmm = (now.format("%H").to_string().parse::<u16>().unwrap_or(0) * 100)
            + now.format("%M").to_string().parse::<u16>().unwrap_or(0);
        let current_day = now.format("%j").to_string().parse::<u16>().unwrap_or(0);

        // Avoid duplicate ticks in the same minute
        if let Some((last_hhmm, last_day)) = self.last_tick_minute {
            if last_hhmm == current_hhmm && last_day == current_day {
                return;
            }
        }
        self.last_tick_minute = Some((current_hhmm, current_day));

        let event = BotEvent::ScheduledTick {
            hall_id,
            current_time_hhmm: current_hhmm,
        };

        self.dispatch(&event);
    }

    /// Check for missed archive runs and catch up
    /// Called on app startup
    pub fn check_missed_runs(&mut self, hall_id: Uuid) {
        // Get archive config for hall
        let config = {
            let db = self.state.db.lock().unwrap();
            db.archive_config().get(hall_id).ok().flatten()
        };

        if let Some(config) = config {
            if !config.enabled {
                return;
            }

            // Check if we missed a run
            if let Some(last_run) = config.last_run_at {
                let now = chrono::Utc::now();
                let hours_since = (now - last_run).num_hours();

                // If more than 25 hours since last run, we missed one
                if hours_since > 25 {
                    tracing::info!(
                        hall_id = %hall_id,
                        hours_since = hours_since,
                        "Catching up missed archive run"
                    );

                    // Trigger archive now
                    if let Some(idx) = self.archivist_index {
                        if let Some(archivist) = self.bots.get_mut(idx) {
                            if let Some(archivist) = archivist.as_any_mut().downcast_mut::<Archivist>()
                            {
                                if let Some(action) =
                                    archivist.handle_command(hall_id, "/archive-now", Uuid::nil())
                                {
                                    self.execute_action("archivist".to_string(), &action);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

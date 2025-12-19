//! Bot runtime - manages bot lifecycle and event dispatch
//!
//! Minimal runtime for first-party bots. Capability enforcement happens here.
//! Bots interact with Exom only through the skeleton defined in exom_core::bot.

use std::sync::Arc;

use chrono::Local;
use exom_core::{Bot, BotAction, BotCapability, BotEvent};
use uuid::Uuid;

use crate::archivist::Archivist;
use crate::state::AppState;
use crate::town_crier::TownCrier;

/// Bot runtime - manages bots and dispatches events
pub struct BotRuntime {
    bots: Vec<Box<dyn Bot>>,
    state: Arc<AppState>,
    /// Last scheduled tick time (to avoid duplicate ticks in same minute)
    last_tick_minute: Option<(u16, u16)>, // (hour * 100 + minute, day)
}

impl BotRuntime {
    /// Create a new bot runtime with built-in bots
    pub fn new(state: Arc<AppState>) -> Self {
        let mut runtime = Self {
            bots: Vec::new(),
            state: state.clone(),
            last_tick_minute: None,
        };

        // Register built-in bots
        runtime.register_bot(Box::new(TownCrier::new(state.db.clone())));
        runtime.register_bot(Box::new(Archivist::new(state.db.clone())));

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
            BotAction::EmitSystemMessage { hall_id, content } => {
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
            BotAction::SpawnExternalTool {
                hall_id,
                tool_id,
                command,
                args,
                cwd: _cwd, // TODO: support working directory
            } => {
                // Spawn external tool (opens in NEW WINDOW - not embedded)
                let mut tools = self.state.tools.lock().unwrap();
                match tools.launch(
                    *hall_id,
                    tool_id.clone(),
                    command,
                    args,
                    Some(bot_id.clone()),
                ) {
                    Ok(id) => {
                        tracing::info!(
                            bot_id = %bot_id,
                            hall_id = %hall_id,
                            tool_id = %tool_id,
                            spawned_id = %id,
                            "Bot spawned external tool (new window)"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            bot_id = %bot_id,
                            hall_id = %hall_id,
                            tool_id = %tool_id,
                            error = %e,
                            "Bot failed to spawn external tool"
                        );
                    }
                }
            }
            BotAction::StopExternalTool { hall_id, tool_id } => {
                // Stop a running tool
                let mut tools = self.state.tools.lock().unwrap();
                match tools.stop_by_id(tool_id) {
                    Ok(()) => {
                        tracing::info!(
                            bot_id = %bot_id,
                            hall_id = %hall_id,
                            tool_id = %tool_id,
                            "Bot stopped external tool"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            bot_id = %bot_id,
                            hall_id = %hall_id,
                            tool_id = %tool_id,
                            error = %e,
                            "Bot failed to stop external tool"
                        );
                    }
                }
            }
            // Other actions not yet implemented
            _ => {
                tracing::debug!(
                    bot_id = %bot_id,
                    action = ?action,
                    "Bot action not yet implemented"
                );
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
        // Only process slash commands
        if !message.starts_with('/') {
            return false;
        }

        // Try each bot that can handle commands
        for bot in &mut self.bots {
            // Check if bot has HandleCommands capability
            if !bot.has_capability(BotCapability::HandleCommands) {
                continue;
            }

            // Check if this bot's prefixes match
            let prefixes = bot.command_prefixes();
            let matches = prefixes.iter().any(|p| message.starts_with(p));
            if !matches {
                continue;
            }

            // Try to handle the command
            if let Some(actions) = bot.handle_command(hall_id, user_id, message) {
                let bot_id = bot.manifest().id.clone();
                for action in actions {
                    self.execute_action(bot_id.clone(), &action);
                }
                return true;
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

    /// Notify bots that a hall has been connected
    /// Bots can use this for startup tasks (e.g., checking missed runs)
    pub fn on_hall_connected(&mut self, hall_id: Uuid) {
        let event = BotEvent::HallConnected { hall_id };
        self.dispatch(&event);
    }
}

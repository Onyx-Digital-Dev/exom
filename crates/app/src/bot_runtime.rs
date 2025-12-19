//! Bot runtime - manages bot lifecycle and event dispatch
//!
//! Minimal runtime for first-party bots. Capability enforcement happens here.
//! Bots interact with Exom only through the skeleton defined in exom_core::bot.

use std::collections::HashSet;
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
        // Include the bot's capabilities for enforcement during execution
        let mut all_actions: Vec<(String, HashSet<BotCapability>, BotAction)> = Vec::new();

        for bot in &mut self.bots {
            // Capability check - only deliver if bot has required capability
            if !bot.should_receive(event) {
                continue;
            }

            // Get actions from bot
            let bot_id = bot.manifest().id.clone();
            let bot_caps: HashSet<BotCapability> =
                bot.manifest().capabilities.iter().copied().collect();
            let actions = bot.on_event(event);

            for action in actions {
                all_actions.push((bot_id.clone(), bot_caps.clone(), action));
            }
        }

        // Execute actions after the mutable borrow is released
        for (bot_id, bot_caps, action) in all_actions {
            self.execute_action(&bot_id, &bot_caps, &action);
        }
    }

    /// Execute a bot action with capability enforcement
    fn execute_action(&self, bot_id: &str, bot_caps: &HashSet<BotCapability>, action: &BotAction) {
        // CRITICAL: Enforce capability check before executing any action
        let required_cap = action.required_capability();
        if !bot_caps.contains(&required_cap) {
            tracing::warn!(
                bot_id = %bot_id,
                required = ?required_cap,
                "Bot attempted action without required capability - DENIED"
            );
            return;
        }

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
                // Write file to hall chest - use safe mutex handling
                let chest = match self.state.chest.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        tracing::error!(bot_id = %bot_id, "Chest mutex poisoned, recovering");
                        poisoned.into_inner()
                    }
                };
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
                cwd: _cwd,
            } => {
                // Spawn external tool (opens in NEW WINDOW - not embedded)
                let mut tools = match self.state.tools.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        tracing::error!(bot_id = %bot_id, "Tools mutex poisoned, recovering");
                        poisoned.into_inner()
                    }
                };
                match tools.launch(
                    *hall_id,
                    tool_id.clone(),
                    command,
                    args,
                    Some(bot_id.to_string()),
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
                // Stop a running tool - use safe mutex handling
                let mut tools = match self.state.tools.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        tracing::error!(bot_id = %bot_id, "Tools mutex poisoned, recovering");
                        poisoned.into_inner()
                    }
                };
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
        // Look up last_seen to determine if first time - use safe mutex handling
        let (is_first_time, last_seen_duration) = {
            let db = match self.state.db.lock() {
                Ok(guard) => guard,
                Err(poisoned) => {
                    tracing::error!("DB mutex poisoned in on_member_joined, recovering");
                    poisoned.into_inner()
                }
            };
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
                let bot_caps: HashSet<BotCapability> =
                    bot.manifest().capabilities.iter().copied().collect();
                for action in actions {
                    self.execute_action(&bot_id, &bot_caps, &action);
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

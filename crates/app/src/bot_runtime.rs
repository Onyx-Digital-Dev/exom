//! Bot runtime - manages bot lifecycle and event dispatch
//!
//! Minimal runtime for first-party bots. Capability enforcement happens here.

use std::sync::Arc;

use exom_core::{Bot, BotAction, BotEvent};
use uuid::Uuid;

use crate::state::AppState;
use crate::town_crier::TownCrier;

/// Bot runtime - manages bots and dispatches events
pub struct BotRuntime {
    bots: Vec<Box<dyn Bot>>,
    state: Arc<AppState>,
}

impl BotRuntime {
    /// Create a new bot runtime with built-in bots
    pub fn new(state: Arc<AppState>) -> Self {
        let mut runtime = Self {
            bots: Vec::new(),
            state: state.clone(),
        };

        // Register Town Crier as built-in bot
        let town_crier = TownCrier::new(state.db.clone());
        runtime.register_bot(Box::new(town_crier));

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
        }
    }

    /// Handle member joined event
    pub fn on_member_joined(
        &mut self,
        hall_id: Uuid,
        user_id: Uuid,
        username: String,
    ) {
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
}

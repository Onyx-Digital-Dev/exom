//! Archivist - Nightly archive summarizer bot
//!
//! Creates markdown summaries of chat activity. No LLM, no AI - just
//! deterministic extractive summarization using simple sentence scoring.

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Duration, Local, Utc};
use exom_core::{
    ArchiveConfig, ArchiveOutput, ArchiveWindow, Bot, BotAction, BotCapability, BotEvent,
    BotManifest, Database, MessageDisplay,
};
use uuid::Uuid;

// ============================================================================
// EXTRACTIVE SUMMARIZER (no AI, deterministic)
// ============================================================================

/// A scored sentence for extractive summarization
#[derive(Debug, Clone)]
struct ScoredSentence {
    text: String,
    speaker: String,
    score: f64,
}

/// Simple TF-IDF-like scoring for sentences
/// Uses term frequency and inverse document frequency to rank sentences
fn score_sentences(messages: &[MessageDisplay], max_sentences: usize) -> Vec<ScoredSentence> {
    if messages.is_empty() {
        return Vec::new();
    }

    // Build vocabulary and document frequency
    let mut doc_freq: HashMap<String, usize> = HashMap::new();
    let mut sentences: Vec<(String, String, Vec<String>)> = Vec::new(); // (speaker, text, terms)

    for msg in messages {
        // Split message into sentences (simple split on . ! ?)
        let sents = split_sentences(&msg.content);
        for sent in sents {
            let terms = tokenize(&sent);
            if terms.is_empty() {
                continue;
            }

            // Update document frequency
            let unique_terms: HashSet<_> = terms.iter().collect();
            for term in &unique_terms {
                *doc_freq.entry((*term).clone()).or_insert(0) += 1;
            }

            sentences.push((msg.sender_username.clone(), sent, terms));
        }
    }

    if sentences.is_empty() {
        return Vec::new();
    }

    let num_docs = sentences.len() as f64;

    // Score each sentence using TF-IDF-like scoring
    let mut scored: Vec<ScoredSentence> = sentences
        .into_iter()
        .map(|(speaker, text, terms)| {
            // Term frequency in this sentence
            let mut tf: HashMap<&str, f64> = HashMap::new();
            for term in &terms {
                *tf.entry(term.as_str()).or_insert(0.0) += 1.0;
            }

            // TF-IDF score
            let mut score = 0.0;
            for (term, freq) in tf {
                let df = doc_freq.get(term).copied().unwrap_or(1) as f64;
                let idf = (num_docs / df).ln() + 1.0;
                score += freq * idf;
            }

            // Normalize by sentence length to avoid bias toward long sentences
            score /= (terms.len() as f64).sqrt();

            // Boost sentences with certain keywords indicating importance
            let text_lower = text.to_lowercase();
            if text_lower.contains("important")
                || text_lower.contains("key")
                || text_lower.contains("note")
            {
                score *= 1.3;
            }

            ScoredSentence {
                text,
                speaker,
                score,
            }
        })
        .collect();

    // Sort by score descending
    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    // Take top N
    scored.truncate(max_sentences);

    scored
}

/// Split text into sentences
fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();

    for c in text.chars() {
        current.push(c);
        if c == '.' || c == '!' || c == '?' {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() && trimmed.len() > 10 {
                sentences.push(trimmed);
            }
            current = String::new();
        }
    }

    // Add remaining text if it's substantial
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() && trimmed.len() > 20 {
        sentences.push(trimmed);
    }

    sentences
}

/// Tokenize text into lowercase terms
fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() > 2) // Skip short words
        .map(|s| s.to_string())
        .collect()
}

/// Extract questions (lines ending with ?)
fn extract_questions(messages: &[MessageDisplay], max_questions: usize) -> Vec<(String, String)> {
    let mut questions = Vec::new();

    for msg in messages {
        for line in msg.content.lines() {
            let trimmed = line.trim();
            if trimmed.ends_with('?') && trimmed.len() > 10 {
                questions.push((msg.sender_username.clone(), trimmed.to_string()));
                if questions.len() >= max_questions {
                    return questions;
                }
            }
        }
    }

    questions
}

/// Extract decisions/TODOs using keyword heuristics
fn extract_decisions(messages: &[MessageDisplay], max_items: usize) -> Vec<(String, String)> {
    let keywords = [
        "decide", "decided", "decision",
        "let's", "lets",
        "todo", "to-do", "to do",
        "ship", "shipped",
        "fix", "fixed", "fixing",
        "next", "next step",
        "will", "going to", "gonna",
        "should", "must", "need to",
    ];

    let mut items = Vec::new();

    for msg in messages {
        let content_lower = msg.content.to_lowercase();
        for keyword in &keywords {
            if content_lower.contains(keyword) {
                // Find the sentence containing the keyword
                for line in msg.content.lines() {
                    let line_lower = line.to_lowercase();
                    if line_lower.contains(keyword) && line.trim().len() > 15 {
                        items.push((msg.sender_username.clone(), line.trim().to_string()));
                        if items.len() >= max_items {
                            return items;
                        }
                        break;
                    }
                }
                break;
            }
        }
    }

    items
}

/// Calculate activity statistics
fn calculate_stats(messages: &[MessageDisplay]) -> (usize, Vec<(String, usize)>) {
    let total = messages.len();
    let mut by_user: HashMap<String, usize> = HashMap::new();

    for msg in messages {
        *by_user.entry(msg.sender_username.clone()).or_insert(0) += 1;
    }

    let mut top_users: Vec<_> = by_user.into_iter().collect();
    top_users.sort_by(|a, b| b.1.cmp(&a.1));
    top_users.truncate(3);

    (total, top_users)
}

// ============================================================================
// MARKDOWN GENERATOR
// ============================================================================

/// Generate the archive markdown content
fn generate_archive_markdown(
    hall_name: &str,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    messages: &[MessageDisplay],
    highlight_count: usize,
) -> String {
    let mut md = String::new();

    // Header
    md.push_str(&format!("# Archive: {}\n\n", hall_name));
    md.push_str(&format!(
        "**Period:** {} to {}\n\n",
        start_time.format("%Y-%m-%d %H:%M UTC"),
        end_time.format("%Y-%m-%d %H:%M UTC")
    ));

    // Statistics
    let (total, top_users) = calculate_stats(messages);
    md.push_str("## Activity Summary\n\n");
    md.push_str(&format!("- **Total messages:** {}\n", total));
    md.push_str(&format!(
        "- **Unique participants:** {}\n",
        messages
            .iter()
            .map(|m| &m.sender_username)
            .collect::<HashSet<_>>()
            .len()
    ));

    if !top_users.is_empty() {
        md.push_str("- **Top contributors:**\n");
        for (user, count) in &top_users {
            md.push_str(&format!("  - {}: {} messages\n", user, count));
        }
    }
    md.push('\n');

    // Highlights (extractive summary)
    let highlights = score_sentences(messages, highlight_count);
    if !highlights.is_empty() {
        md.push_str("## Highlights\n\n");
        for hl in &highlights {
            md.push_str(&format!("- **{}:** {}\n", hl.speaker, hl.text));
        }
        md.push('\n');
    }

    // Open questions
    let questions = extract_questions(messages, 10);
    if !questions.is_empty() {
        md.push_str("## Open Questions\n\n");
        for (speaker, question) in &questions {
            md.push_str(&format!("- **{}:** {}\n", speaker, question));
        }
        md.push('\n');
    }

    // Decisions/TODOs
    let decisions = extract_decisions(messages, 15);
    if !decisions.is_empty() {
        md.push_str("## Decisions and Action Items\n\n");
        for (speaker, item) in &decisions {
            md.push_str(&format!("- **{}:** {}\n", speaker, item));
        }
        md.push('\n');
    }

    // Footer
    md.push_str("---\n");
    md.push_str(&format!(
        "*Generated by Archivist at {}*\n",
        Utc::now().format("%Y-%m-%d %H:%M UTC")
    ));

    md
}

// ============================================================================
// ARCHIVIST BOT
// ============================================================================

/// Archivist bot - generates nightly markdown summaries
pub struct Archivist {
    manifest: BotManifest,
    db: Arc<std::sync::Mutex<Database>>,
    /// Last tick time per hall (to avoid duplicate runs)
    last_tick: HashMap<Uuid, u16>,
}

impl Archivist {
    pub fn new(db: Arc<std::sync::Mutex<Database>>) -> Self {
        Self {
            manifest: BotManifest {
                id: "archivist".to_string(),
                name: "Archivist".to_string(),
                version: "1.0.0".to_string(),
                capabilities: vec![
                    BotCapability::EmitSystem,
                    BotCapability::ReadChatHistory,
                    BotCapability::WriteChest,
                    BotCapability::ReceiveScheduledTick,
                ],
            },
            db,
            last_tick: HashMap::new(),
        }
    }

    /// Check if archiving should run for a hall at the given time
    fn should_run(&self, hall_id: Uuid, current_time: u16) -> bool {
        let config = match self.get_config(hall_id) {
            Some(c) => c,
            None => return false,
        };

        // Not enabled
        if !config.enabled {
            return false;
        }

        // Check if time matches
        if config.archive_time != current_time {
            return false;
        }

        // Check if we already ran at this time
        if let Some(&last) = self.last_tick.get(&hall_id) {
            if last == current_time {
                return false;
            }
        }

        true
    }

    /// Get archive config for a hall
    fn get_config(&self, hall_id: Uuid) -> Option<ArchiveConfig> {
        let db = self.db.lock().ok()?;
        db.archive_config().get(hall_id).ok().flatten()
    }

    /// Get messages for the archive window
    fn get_messages_in_window(
        &self,
        hall_id: Uuid,
        config: &ArchiveConfig,
    ) -> Vec<MessageDisplay> {
        let db = match self.db.lock() {
            Ok(db) => db,
            Err(_) => return Vec::new(),
        };

        let now = Utc::now();
        let start_time = match config.archive_window {
            ArchiveWindow::Hours12 => now - Duration::hours(12),
            ArchiveWindow::Hours24 => now - Duration::hours(24),
            ArchiveWindow::SinceLastRun => config.last_run_at.unwrap_or(now - Duration::hours(24)),
        };

        // Get messages in time range
        // Note: We fetch more than needed and filter client-side
        match db.messages().list_for_hall(hall_id, 1000, None) {
            Ok(messages) => messages
                .into_iter()
                .filter(|m| m.timestamp >= start_time && m.timestamp <= now)
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Get hall name
    fn get_hall_name(&self, hall_id: Uuid) -> String {
        let db = match self.db.lock() {
            Ok(db) => db,
            Err(_) => return "Unknown Hall".to_string(),
        };

        match db.halls().find_by_id(hall_id) {
            Ok(Some(hall)) => hall.name,
            _ => "Unknown Hall".to_string(),
        }
    }

    /// Generate archive file path
    fn archive_path(config: &ArchiveConfig) -> String {
        let date = Local::now().format("%Y-%m-%d");
        match &config.archive_output {
            ArchiveOutput::Chest => format!("archives/ARCHIVE_{}.md", date),
            ArchiveOutput::ChestUser(username) => {
                format!("archives/{}/ARCHIVE_{}.md", username, date)
            }
        }
    }

    /// Run archive for a hall
    fn run_archive(&mut self, hall_id: Uuid) -> Vec<BotAction> {
        let config = match self.get_config(hall_id) {
            Some(c) => c,
            None => return Vec::new(),
        };

        let messages = self.get_messages_in_window(hall_id, &config);
        if messages.is_empty() {
            return vec![BotAction::EmitSystem {
                hall_id,
                content: "Archive skipped: no messages in time window.".to_string(),
            }];
        }

        let hall_name = self.get_hall_name(hall_id);
        let now = Utc::now();
        let start_time = match config.archive_window {
            ArchiveWindow::Hours12 => now - Duration::hours(12),
            ArchiveWindow::Hours24 => now - Duration::hours(24),
            ArchiveWindow::SinceLastRun => config.last_run_at.unwrap_or(now - Duration::hours(24)),
        };

        // Generate markdown
        let markdown = generate_archive_markdown(&hall_name, start_time, now, &messages, 8);

        // Update last run time
        if let Ok(db) = self.db.lock() {
            let _ = db.archive_config().update_last_run(hall_id, now);
        }

        let path = Self::archive_path(&config);

        vec![
            BotAction::WriteFileToChest {
                hall_id,
                path: path.clone(),
                contents: markdown,
            },
            BotAction::EmitSystem {
                hall_id,
                content: format!("Archive saved to {}", path),
            },
        ]
    }

    /// Handle an archive command
    pub fn handle_command(
        &mut self,
        hall_id: Uuid,
        command: &str,
        _user_id: Uuid,
    ) -> Option<BotAction> {
        let parts: Vec<&str> = command.trim().split_whitespace().collect();
        if parts.is_empty() {
            return None;
        }

        match parts[0] {
            "/archive-now" => {
                // Run archive immediately
                let actions = self.run_archive(hall_id);
                if actions.is_empty() {
                    Some(BotAction::EmitSystem {
                        hall_id,
                        content: "Archive failed: no configuration found.".to_string(),
                    })
                } else {
                    // Return first action (the others will be lost, but that's ok for /archive-now)
                    actions.into_iter().next()
                }
            }
            "/archive-status" => {
                let config = self.get_config(hall_id);
                let status = match config {
                    Some(c) => {
                        let enabled = if c.enabled { "enabled" } else { "disabled" };
                        let last_run = c
                            .last_run_at
                            .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                            .unwrap_or_else(|| "never".to_string());
                        format!(
                            "Archivist: {}, time: {:04}, window: {}, last run: {}",
                            enabled,
                            c.archive_time,
                            c.archive_window.as_str(),
                            last_run
                        )
                    }
                    None => "Archivist: not configured".to_string(),
                };
                Some(BotAction::EmitSystem {
                    hall_id,
                    content: status,
                })
            }
            "/archive-enable" => {
                if let Ok(db) = self.db.lock() {
                    let _ = db.archive_config().set_enabled(hall_id, true);
                }
                Some(BotAction::EmitSystem {
                    hall_id,
                    content: "Archivist enabled.".to_string(),
                })
            }
            "/archive-disable" => {
                if let Ok(db) = self.db.lock() {
                    let _ = db.archive_config().set_enabled(hall_id, false);
                }
                Some(BotAction::EmitSystem {
                    hall_id,
                    content: "Archivist disabled.".to_string(),
                })
            }
            "/set-archive-time" => {
                if parts.len() < 2 {
                    return Some(BotAction::EmitSystem {
                        hall_id,
                        content: "Usage: /set-archive-time HHMM (e.g., 2200)".to_string(),
                    });
                }
                match parts[1].parse::<u16>() {
                    Ok(time) if time < 2400 => {
                        if let Ok(db) = self.db.lock() {
                            let _ = db.archive_config().set_time(hall_id, time);
                        }
                        Some(BotAction::EmitSystem {
                            hall_id,
                            content: format!("Archive time set to {:04}.", time),
                        })
                    }
                    _ => Some(BotAction::EmitSystem {
                        hall_id,
                        content: "Invalid time. Use HHMM format (0000-2359).".to_string(),
                    }),
                }
            }
            "/set-archive-window" => {
                if parts.len() < 2 {
                    return Some(BotAction::EmitSystem {
                        hall_id,
                        content: "Usage: /set-archive-window 12h|24h".to_string(),
                    });
                }
                match ArchiveWindow::from_str(parts[1]) {
                    Some(window) => {
                        if let Ok(db) = self.db.lock() {
                            let _ = db.archive_config().set_window(hall_id, window);
                        }
                        Some(BotAction::EmitSystem {
                            hall_id,
                            content: format!("Archive window set to {}.", window.as_str()),
                        })
                    }
                    None => Some(BotAction::EmitSystem {
                        hall_id,
                        content: "Invalid window. Use 12h or 24h.".to_string(),
                    }),
                }
            }
            "/set-archive-output" => {
                if parts.len() < 2 {
                    return Some(BotAction::EmitSystem {
                        hall_id,
                        content: "Usage: /set-archive-output chest|chest:username".to_string(),
                    });
                }
                match ArchiveOutput::from_str(parts[1]) {
                    Some(output) => {
                        if let Ok(db) = self.db.lock() {
                            let _ = db.archive_config().set_output(hall_id, &output);
                        }
                        Some(BotAction::EmitSystem {
                            hall_id,
                            content: format!("Archive output set to {}.", output.as_str()),
                        })
                    }
                    None => Some(BotAction::EmitSystem {
                        hall_id,
                        content: "Invalid output. Use chest or chest:username.".to_string(),
                    }),
                }
            }
            _ => None,
        }
    }
}

impl Bot for Archivist {
    fn manifest(&self) -> &BotManifest {
        &self.manifest
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn on_event(&mut self, event: &BotEvent) -> Vec<BotAction> {
        match event {
            BotEvent::ScheduledTick {
                hall_id,
                current_time_hhmm,
            } => {
                if self.should_run(*hall_id, *current_time_hhmm) {
                    // Mark that we ran at this time
                    self.last_tick.insert(*hall_id, *current_time_hhmm);
                    self.run_archive(*hall_id)
                } else {
                    Vec::new()
                }
            }
            // Archivist doesn't handle presence events
            _ => Vec::new(),
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use exom_core::HallRole;

    /// Helper to create a test MessageDisplay
    fn test_msg(username: &str, content: &str) -> MessageDisplay {
        MessageDisplay {
            id: Uuid::new_v4(),
            sender_id: Uuid::new_v4(),
            sender_username: username.to_string(),
            sender_role: HallRole::HallAgent,
            content: content.to_string(),
            timestamp: Utc::now(),
            is_edited: false,
        }
    }

    #[test]
    fn test_tokenize() {
        let tokens = tokenize("Hello, World! This is a test.");
        assert!(tokens.contains(&"hello".to_string()));
        assert!(tokens.contains(&"world".to_string()));
        assert!(tokens.contains(&"this".to_string()));
        assert!(tokens.contains(&"test".to_string()));
        // Short words should be filtered
        assert!(!tokens.contains(&"is".to_string()));
        assert!(!tokens.contains(&"a".to_string()));
    }

    #[test]
    fn test_split_sentences() {
        // Sentences must be >10 chars to be included
        let sents = split_sentences("Hello there everyone. How are you doing today? I am doing fine thanks!");
        assert_eq!(sents.len(), 3);
        assert!(sents[0].contains("Hello"));
        assert!(sents[1].contains("How"));
        assert!(sents[2].contains("fine"));
    }

    #[test]
    fn test_extract_questions() {
        let messages = vec![
            test_msg("alice", "What do you think about this feature?"),
            test_msg("bob", "I think it's great!"),
        ];

        let questions = extract_questions(&messages, 10);
        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].0, "alice");
        assert!(questions[0].1.contains("feature"));
    }

    #[test]
    fn test_extract_decisions() {
        let messages = vec![
            test_msg("alice", "Let's ship this feature tomorrow."),
            test_msg("bob", "Sounds good to me."),
        ];

        let decisions = extract_decisions(&messages, 10);
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].0, "alice");
        assert!(decisions[0].1.contains("ship"));
    }

    #[test]
    fn test_score_sentences_returns_results() {
        let messages = vec![
            test_msg("alice", "This is an important message about the project direction."),
            test_msg("bob", "I agree with the general approach."),
        ];

        let highlights = score_sentences(&messages, 5);
        assert!(!highlights.is_empty());
    }

    #[test]
    fn test_archive_path_generation() {
        let config = ArchiveConfig {
            hall_id: Uuid::new_v4(),
            enabled: true,
            archive_time: 2200,
            archive_window: ArchiveWindow::Hours24,
            archive_output: ArchiveOutput::Chest,
            last_run_at: None,
        };

        let path = Archivist::archive_path(&config);
        assert!(path.starts_with("archives/ARCHIVE_"));
        assert!(path.ends_with(".md"));

        let config_user = ArchiveConfig {
            archive_output: ArchiveOutput::ChestUser("alice".to_string()),
            ..config
        };
        let path_user = Archivist::archive_path(&config_user);
        assert!(path_user.contains("alice"));
    }

    #[test]
    fn test_calculate_stats() {
        let messages = vec![
            test_msg("alice", "Message 1"),
            test_msg("alice", "Message 2"),
            test_msg("bob", "Message 3"),
        ];

        let (total, top) = calculate_stats(&messages);
        assert_eq!(total, 3);
        assert_eq!(top[0].0, "alice");
        assert_eq!(top[0].1, 2);
    }
}

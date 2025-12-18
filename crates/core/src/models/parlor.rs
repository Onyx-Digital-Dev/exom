//! Parlor module placeholder (future plugin system)
//!
//! Hall Parlors are plugin modules that extend Hall functionality.
//! Examples: education, watch-together, code-together
//!
//! This module defines the interface but does NOT implement any parlors yet.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identifier for a parlor module type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ParlorId(pub Uuid);

impl ParlorId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ParlorId {
    fn default() -> Self {
        Self::new()
    }
}

/// Parlor module trait - placeholder for future implementation
///
/// Parlors will be able to:
/// - Add custom UI panels
/// - Handle custom message types
/// - Sync custom state between members
/// - Define custom permissions
pub trait ParlorModule: Send + Sync {
    /// Unique identifier for this parlor type
    fn parlor_type_id(&self) -> &'static str;

    /// Human-readable name
    fn display_name(&self) -> &'static str;

    /// Called when parlor is activated in a Hall
    fn on_activate(&mut self, hall_id: Uuid);

    /// Called when parlor is deactivated
    fn on_deactivate(&mut self, hall_id: Uuid);
}

/// Registry for available parlor modules (future use)
pub struct ParlorRegistry {
    // Will hold registered parlor factories
    _placeholder: (),
}

impl ParlorRegistry {
    pub fn new() -> Self {
        Self { _placeholder: () }
    }
}

impl Default for ParlorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

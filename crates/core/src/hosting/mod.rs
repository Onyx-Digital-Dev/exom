//! Hosting state management for Halls
//!
//! Hosting determines which member is responsible for coordinating
//! Hall activities. This is state-only for now (no real networking).

use uuid::Uuid;

use crate::error::{Error, Result};
use crate::models::{HallRole, MemberInfo};

/// Hosting state for a Hall
#[derive(Debug, Clone)]
pub struct HostingState {
    /// Current host user ID
    pub host_id: Option<Uuid>,
    /// Election epoch to prevent split-host
    pub election_epoch: u64,
    /// Pending host transfer (if any)
    pub pending_transfer: Option<HostTransfer>,
}

/// A pending host transfer request
#[derive(Debug, Clone)]
pub struct HostTransfer {
    pub from_user_id: Uuid,
    pub to_user_id: Uuid,
    pub epoch: u64,
}

/// Result of a host election
#[derive(Debug, Clone)]
pub enum HostElectionResult {
    /// A new host was elected
    Elected(Uuid),
    /// A higher-role user should be prompted to take over
    PromptTakeover(Uuid),
    /// No eligible host found
    NoHost,
}

impl HostingState {
    pub fn new() -> Self {
        Self {
            host_id: None,
            election_epoch: 0,
            pending_transfer: None,
        }
    }

    /// Check if a user is currently the host
    pub fn is_host(&self, user_id: Uuid) -> bool {
        self.host_id == Some(user_id)
    }

    /// Attempt to become host when entering an empty Hall
    pub fn try_become_initial_host(&mut self, user_id: Uuid, role: HallRole) -> Result<bool> {
        if !role.can_host() {
            return Ok(false);
        }

        if self.host_id.is_none() {
            self.host_id = Some(user_id);
            self.election_epoch += 1;
            return Ok(true);
        }

        Ok(false)
    }

    /// Handle a user joining the Hall
    /// Returns a prompt if they should be offered host takeover
    pub fn on_user_join(
        &self,
        joining_user: Uuid,
        joining_role: HallRole,
        current_host_role: Option<HallRole>,
    ) -> Option<HostElectionResult> {
        if !joining_role.can_host() {
            return None;
        }

        // If no current host, they become host
        if self.host_id.is_none() {
            return Some(HostElectionResult::Elected(joining_user));
        }

        // If joining user has higher priority than current host, prompt takeover
        if let Some(host_role) = current_host_role {
            if joining_role.hosting_priority() > host_role.hosting_priority() {
                return Some(HostElectionResult::PromptTakeover(joining_user));
            }
        }

        None
    }

    /// Handle current host leaving
    /// Returns the next host candidate from remaining members
    pub fn on_host_leave(&self, members: &[MemberInfo]) -> HostElectionResult {
        // Find highest-priority member who can host
        let mut candidates: Vec<_> = members
            .iter()
            .filter(|m| m.role.can_host() && m.is_online)
            .collect();

        // Sort by role priority (highest first)
        candidates.sort_by(|a, b| b.role.hosting_priority().cmp(&a.role.hosting_priority()));

        if let Some(candidate) = candidates.first() {
            HostElectionResult::PromptTakeover(candidate.user_id)
        } else {
            HostElectionResult::NoHost
        }
    }

    /// Transfer host to another user
    pub fn transfer_host(&mut self, to_user_id: Uuid, epoch: u64) -> Result<()> {
        if epoch != self.election_epoch {
            return Err(Error::Hosting("Stale election epoch".into()));
        }

        self.host_id = Some(to_user_id);
        self.election_epoch += 1;
        self.pending_transfer = None;

        Ok(())
    }

    /// Set host directly (for initialization)
    pub fn set_host(&mut self, user_id: Option<Uuid>) {
        self.host_id = user_id;
        self.election_epoch += 1;
    }
}

impl Default for HostingState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_host() {
        let mut state = HostingState::new();
        let user_id = Uuid::new_v4();

        assert!(state
            .try_become_initial_host(user_id, HallRole::HallAgent)
            .unwrap());
        assert!(state.is_host(user_id));
    }

    #[test]
    fn test_fellow_cannot_host() {
        let mut state = HostingState::new();
        let user_id = Uuid::new_v4();

        assert!(!state
            .try_become_initial_host(user_id, HallRole::HallFellow)
            .unwrap());
        assert!(!state.is_host(user_id));
    }

    #[test]
    fn test_higher_role_prompt() {
        let mut state = HostingState::new();
        let existing_host = Uuid::new_v4();
        state.set_host(Some(existing_host));

        let builder_id = Uuid::new_v4();
        let result =
            state.on_user_join(builder_id, HallRole::HallBuilder, Some(HallRole::HallAgent));

        assert!(matches!(
            result,
            Some(HostElectionResult::PromptTakeover(_))
        ));
    }
}

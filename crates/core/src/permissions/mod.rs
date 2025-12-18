//! Permission system for Hall operations
//!
//! Provides a comprehensive permission matrix and enforcement utilities
//! for role-based access control in Halls.

use crate::error::{Error, Result};
use crate::models::HallRole;

/// Actions that can be performed in a Hall
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HallAction {
    // Hall management
    DeleteHall,
    EditHallSettings,
    TransferOwnership,

    // Member management
    ViewMembers,
    InviteMembers,
    InviteWithRole(HallRole),
    KickMembers,
    BanMembers,
    PromoteMembers,
    DemoteMembers,

    // Chat
    ViewMessages,
    SendMessages,
    DeleteOwnMessages,
    DeleteOtherMessages,
    EditOwnMessages,
    PinMessages,

    // Hosting
    BecomeHost,
    TransferHost,
    ForceHostTransfer,

    // Hall Chest
    ViewChest,
    WriteChest,
    DeleteFromChest,
    ManageChest,

    // Parlors (future)
    ViewParlors,
    ActivateParlor,
    ConfigureParlor,
}

/// Permission matrix for Hall roles
pub struct PermissionMatrix;

impl PermissionMatrix {
    /// Check if a role has permission to perform an action
    pub fn can_perform(role: HallRole, action: HallAction) -> bool {
        match action {
            // Hall management - Builder only
            HallAction::DeleteHall => role == HallRole::HallBuilder,
            HallAction::TransferOwnership => role == HallRole::HallBuilder,

            // Hall settings - Builder and Prefect
            HallAction::EditHallSettings => role >= HallRole::HallPrefect,

            // Member viewing - everyone can see members
            HallAction::ViewMembers => true,

            // Member management
            HallAction::InviteMembers => role >= HallRole::HallModerator,
            HallAction::InviteWithRole(invite_role) => {
                // Can only invite with roles lower than your own
                role >= HallRole::HallModerator && invite_role < role
            }
            HallAction::KickMembers => role >= HallRole::HallModerator,
            HallAction::BanMembers => role >= HallRole::HallPrefect,
            HallAction::PromoteMembers => role >= HallRole::HallPrefect,
            HallAction::DemoteMembers => role >= HallRole::HallPrefect,

            // Chat - viewing and sending for all, moderation for higher roles
            HallAction::ViewMessages => true,
            HallAction::SendMessages => role >= HallRole::HallFellow,
            HallAction::DeleteOwnMessages => role >= HallRole::HallFellow,
            HallAction::EditOwnMessages => role >= HallRole::HallFellow,
            HallAction::DeleteOtherMessages => role >= HallRole::HallModerator,
            HallAction::PinMessages => role >= HallRole::HallModerator,

            // Hosting - Agent and above can host
            HallAction::BecomeHost => role >= HallRole::HallAgent,
            HallAction::TransferHost => role >= HallRole::HallAgent,
            HallAction::ForceHostTransfer => role >= HallRole::HallPrefect,

            // Hall Chest - Agent+ can access, Fellows cannot
            HallAction::ViewChest => role >= HallRole::HallAgent,
            HallAction::WriteChest => role >= HallRole::HallAgent,
            HallAction::DeleteFromChest => role >= HallRole::HallAgent,
            HallAction::ManageChest => role >= HallRole::HallPrefect,

            // Parlors - viewing for Agent+, management for Prefect+
            HallAction::ViewParlors => role >= HallRole::HallAgent,
            HallAction::ActivateParlor => role >= HallRole::HallPrefect,
            HallAction::ConfigureParlor => role >= HallRole::HallPrefect,
        }
    }

    /// Check if a role can promote/demote to a target role
    pub fn can_change_role(
        actor_role: HallRole,
        target_current: HallRole,
        target_new: HallRole,
    ) -> bool {
        // Cannot change your own role
        // Cannot change role of someone equal or higher
        if target_current >= actor_role {
            return false;
        }

        // Can only assign roles lower than your own
        if target_new >= actor_role {
            return false;
        }

        // Must be Prefect or higher to change roles
        actor_role >= HallRole::HallPrefect
    }

    /// Check if a role can kick another role
    pub fn can_kick(actor_role: HallRole, target_role: HallRole) -> bool {
        // Can only kick roles lower than your own
        if target_role >= actor_role {
            return false;
        }

        // Must be Moderator or higher to kick
        actor_role >= HallRole::HallModerator
    }

    /// Check if a role can ban another role
    pub fn can_ban(actor_role: HallRole, target_role: HallRole) -> bool {
        // Can only ban roles lower than your own
        if target_role >= actor_role {
            return false;
        }

        // Must be Prefect or higher to ban
        actor_role >= HallRole::HallPrefect
    }

    /// Check if a role can delete a message from a given role
    pub fn can_delete_message(actor_role: HallRole, message_author_role: HallRole) -> bool {
        // Moderators can delete from equal or lower roles
        if actor_role >= HallRole::HallModerator {
            return message_author_role <= actor_role;
        }
        false
    }
}

/// Require a permission, returning an error if not allowed
pub fn require_permission(role: HallRole, action: HallAction) -> Result<()> {
    if PermissionMatrix::can_perform(role, action) {
        Ok(())
    } else {
        Err(Error::PermissionDenied(format!(
            "Role {:?} cannot perform {:?}",
            role, action
        )))
    }
}

/// Require ability to change role, returning an error if not allowed
pub fn require_can_change_role(
    actor_role: HallRole,
    target_current: HallRole,
    target_new: HallRole,
) -> Result<()> {
    if PermissionMatrix::can_change_role(actor_role, target_current, target_new) {
        Ok(())
    } else {
        Err(Error::PermissionDenied(format!(
            "Cannot change role from {:?} to {:?}",
            target_current, target_new
        )))
    }
}

/// Require ability to kick a member, returning an error if not allowed
pub fn require_can_kick(actor_role: HallRole, target_role: HallRole) -> Result<()> {
    if PermissionMatrix::can_kick(actor_role, target_role) {
        Ok(())
    } else {
        Err(Error::PermissionDenied(format!(
            "Cannot kick member with role {:?}",
            target_role
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_permissions() {
        // Builder can do everything
        assert!(PermissionMatrix::can_perform(
            HallRole::HallBuilder,
            HallAction::DeleteHall
        ));
        assert!(PermissionMatrix::can_perform(
            HallRole::HallBuilder,
            HallAction::TransferOwnership
        ));
        assert!(PermissionMatrix::can_perform(
            HallRole::HallBuilder,
            HallAction::InviteMembers
        ));
        assert!(PermissionMatrix::can_perform(
            HallRole::HallBuilder,
            HallAction::BecomeHost
        ));
        assert!(PermissionMatrix::can_perform(
            HallRole::HallBuilder,
            HallAction::ViewChest
        ));
    }

    #[test]
    fn test_prefect_permissions() {
        // Prefect can manage but not delete hall
        assert!(!PermissionMatrix::can_perform(
            HallRole::HallPrefect,
            HallAction::DeleteHall
        ));
        assert!(!PermissionMatrix::can_perform(
            HallRole::HallPrefect,
            HallAction::TransferOwnership
        ));
        assert!(PermissionMatrix::can_perform(
            HallRole::HallPrefect,
            HallAction::EditHallSettings
        ));
        assert!(PermissionMatrix::can_perform(
            HallRole::HallPrefect,
            HallAction::BanMembers
        ));
    }

    #[test]
    fn test_moderator_permissions() {
        // Moderator can invite and kick but not ban
        assert!(PermissionMatrix::can_perform(
            HallRole::HallModerator,
            HallAction::InviteMembers
        ));
        assert!(PermissionMatrix::can_perform(
            HallRole::HallModerator,
            HallAction::KickMembers
        ));
        assert!(!PermissionMatrix::can_perform(
            HallRole::HallModerator,
            HallAction::BanMembers
        ));
        assert!(PermissionMatrix::can_perform(
            HallRole::HallModerator,
            HallAction::DeleteOtherMessages
        ));
    }

    #[test]
    fn test_agent_permissions() {
        // Agent can host and access chest
        assert!(PermissionMatrix::can_perform(
            HallRole::HallAgent,
            HallAction::BecomeHost
        ));
        assert!(PermissionMatrix::can_perform(
            HallRole::HallAgent,
            HallAction::ViewChest
        ));
        assert!(PermissionMatrix::can_perform(
            HallRole::HallAgent,
            HallAction::WriteChest
        ));
        // But not moderate
        assert!(!PermissionMatrix::can_perform(
            HallRole::HallAgent,
            HallAction::KickMembers
        ));
    }

    #[test]
    fn test_fellow_permissions() {
        // Fellow can chat but little else
        assert!(PermissionMatrix::can_perform(
            HallRole::HallFellow,
            HallAction::ViewMessages
        ));
        assert!(PermissionMatrix::can_perform(
            HallRole::HallFellow,
            HallAction::SendMessages
        ));
        assert!(PermissionMatrix::can_perform(
            HallRole::HallFellow,
            HallAction::ViewMembers
        ));
        // Cannot host
        assert!(!PermissionMatrix::can_perform(
            HallRole::HallFellow,
            HallAction::BecomeHost
        ));
        // Cannot access chest
        assert!(!PermissionMatrix::can_perform(
            HallRole::HallFellow,
            HallAction::ViewChest
        ));
    }

    #[test]
    fn test_invite_with_role() {
        // Moderator can invite as Fellow or Agent
        assert!(PermissionMatrix::can_perform(
            HallRole::HallModerator,
            HallAction::InviteWithRole(HallRole::HallFellow)
        ));
        assert!(PermissionMatrix::can_perform(
            HallRole::HallModerator,
            HallAction::InviteWithRole(HallRole::HallAgent)
        ));
        // But not as Moderator or higher
        assert!(!PermissionMatrix::can_perform(
            HallRole::HallModerator,
            HallAction::InviteWithRole(HallRole::HallModerator)
        ));

        // Prefect can invite as Moderator
        assert!(PermissionMatrix::can_perform(
            HallRole::HallPrefect,
            HallAction::InviteWithRole(HallRole::HallModerator)
        ));
    }

    #[test]
    fn test_role_changes() {
        // Prefect can demote Agent to Fellow
        assert!(PermissionMatrix::can_change_role(
            HallRole::HallPrefect,
            HallRole::HallAgent,
            HallRole::HallFellow
        ));

        // Prefect can promote Fellow to Moderator
        assert!(PermissionMatrix::can_change_role(
            HallRole::HallPrefect,
            HallRole::HallFellow,
            HallRole::HallModerator
        ));

        // Prefect cannot promote to Prefect (equal to self)
        assert!(!PermissionMatrix::can_change_role(
            HallRole::HallPrefect,
            HallRole::HallAgent,
            HallRole::HallPrefect
        ));

        // Agent cannot change roles
        assert!(!PermissionMatrix::can_change_role(
            HallRole::HallAgent,
            HallRole::HallFellow,
            HallRole::HallAgent
        ));

        // Cannot change role of equal or higher
        assert!(!PermissionMatrix::can_change_role(
            HallRole::HallPrefect,
            HallRole::HallPrefect,
            HallRole::HallFellow
        ));
    }

    #[test]
    fn test_kick_permissions() {
        // Moderator can kick Agent and Fellow
        assert!(PermissionMatrix::can_kick(
            HallRole::HallModerator,
            HallRole::HallAgent
        ));
        assert!(PermissionMatrix::can_kick(
            HallRole::HallModerator,
            HallRole::HallFellow
        ));

        // Moderator cannot kick Moderator
        assert!(!PermissionMatrix::can_kick(
            HallRole::HallModerator,
            HallRole::HallModerator
        ));

        // Agent cannot kick anyone
        assert!(!PermissionMatrix::can_kick(
            HallRole::HallAgent,
            HallRole::HallFellow
        ));
    }

    #[test]
    fn test_ban_permissions() {
        // Prefect can ban lower roles
        assert!(PermissionMatrix::can_ban(
            HallRole::HallPrefect,
            HallRole::HallModerator
        ));
        assert!(PermissionMatrix::can_ban(
            HallRole::HallPrefect,
            HallRole::HallFellow
        ));

        // Prefect cannot ban Prefect
        assert!(!PermissionMatrix::can_ban(
            HallRole::HallPrefect,
            HallRole::HallPrefect
        ));

        // Moderator cannot ban anyone
        assert!(!PermissionMatrix::can_ban(
            HallRole::HallModerator,
            HallRole::HallFellow
        ));
    }

    #[test]
    fn test_delete_message_permissions() {
        // Moderator can delete messages from lower/equal roles
        assert!(PermissionMatrix::can_delete_message(
            HallRole::HallModerator,
            HallRole::HallFellow
        ));
        assert!(PermissionMatrix::can_delete_message(
            HallRole::HallModerator,
            HallRole::HallModerator
        ));

        // Moderator cannot delete Prefect messages
        assert!(!PermissionMatrix::can_delete_message(
            HallRole::HallModerator,
            HallRole::HallPrefect
        ));

        // Agent cannot delete other messages
        assert!(!PermissionMatrix::can_delete_message(
            HallRole::HallAgent,
            HallRole::HallFellow
        ));
    }

    #[test]
    fn test_require_permission() {
        // Success case
        assert!(require_permission(HallRole::HallBuilder, HallAction::DeleteHall).is_ok());

        // Failure case
        assert!(require_permission(HallRole::HallFellow, HallAction::DeleteHall).is_err());
    }
}

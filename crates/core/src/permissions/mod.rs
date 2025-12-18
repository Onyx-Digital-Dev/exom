//! Permission system for Hall operations

use crate::models::HallRole;

/// Actions that can be performed in a Hall
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HallAction {
    // Hall management
    DeleteHall,
    EditHallSettings,
    TransferOwnership,

    // Member management
    InviteMembers,
    KickMembers,
    BanMembers,
    PromoteMembers,
    DemoteMembers,

    // Chat
    SendMessages,
    DeleteOwnMessages,
    DeleteOtherMessages,
    EditOwnMessages,

    // Hosting
    BecomeHost,
    TransferHost,
    ForceHostTransfer,

    // Hall Chest
    ViewChest,
    WriteChest,
    ManageChest,

    // Parlors (future)
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

            // Member management
            HallAction::InviteMembers => role >= HallRole::HallModerator,
            HallAction::KickMembers => role >= HallRole::HallModerator,
            HallAction::BanMembers => role >= HallRole::HallPrefect,
            HallAction::PromoteMembers => role >= HallRole::HallPrefect,
            HallAction::DemoteMembers => role >= HallRole::HallPrefect,

            // Chat - most can send, some can moderate
            HallAction::SendMessages => role >= HallRole::HallFellow,
            HallAction::DeleteOwnMessages => role >= HallRole::HallFellow,
            HallAction::EditOwnMessages => role >= HallRole::HallFellow,
            HallAction::DeleteOtherMessages => role >= HallRole::HallModerator,

            // Hosting - Agent and above can host
            HallAction::BecomeHost => role >= HallRole::HallAgent,
            HallAction::TransferHost => role >= HallRole::HallAgent,
            HallAction::ForceHostTransfer => role >= HallRole::HallPrefect,

            // Hall Chest - Agent+ can read/write, Fellows cannot
            HallAction::ViewChest => role >= HallRole::HallAgent,
            HallAction::WriteChest => role >= HallRole::HallAgent,
            HallAction::ManageChest => role >= HallRole::HallPrefect,

            // Parlors - Prefect and above
            HallAction::ActivateParlor => role >= HallRole::HallPrefect,
            HallAction::ConfigureParlor => role >= HallRole::HallPrefect,
        }
    }

    /// Check if a role can promote/demote to a target role
    pub fn can_change_role(actor_role: HallRole, _target_current: HallRole, target_new: HallRole) -> bool {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_permissions() {
        assert!(PermissionMatrix::can_perform(HallRole::HallBuilder, HallAction::DeleteHall));
        assert!(PermissionMatrix::can_perform(HallRole::HallBuilder, HallAction::TransferOwnership));
        assert!(PermissionMatrix::can_perform(HallRole::HallBuilder, HallAction::InviteMembers));
    }

    #[test]
    fn test_fellow_permissions() {
        assert!(PermissionMatrix::can_perform(HallRole::HallFellow, HallAction::SendMessages));
        assert!(!PermissionMatrix::can_perform(HallRole::HallFellow, HallAction::BecomeHost));
        assert!(!PermissionMatrix::can_perform(HallRole::HallFellow, HallAction::ViewChest));
    }

    #[test]
    fn test_role_changes() {
        // Prefect can demote Agent to Fellow
        assert!(PermissionMatrix::can_change_role(
            HallRole::HallPrefect,
            HallRole::HallAgent,
            HallRole::HallFellow
        ));

        // Agent cannot change roles
        assert!(!PermissionMatrix::can_change_role(
            HallRole::HallAgent,
            HallRole::HallFellow,
            HallRole::HallAgent
        ));
    }
}

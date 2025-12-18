//! Developer guardrails and invariants
//!
//! Debug assertions for detecting impossible states during development.
//! These checks are compiled out in release builds.

use uuid::Uuid;

use crate::models::{Hall, HallRole, MemberInfo, Membership};

/// Validate that a Hall's state is internally consistent
pub fn assert_hall_invariants(hall: &Hall) {
    // Epoch should be 0 only if there has never been a host
    debug_assert!(
        !(hall.election_epoch == 0 && hall.current_host_id.is_some()),
        "Hall {} has host {:?} but epoch is 0",
        hall.id,
        hall.current_host_id
    );

    // Name must not be empty
    debug_assert!(
        !hall.name.trim().is_empty(),
        "Hall {} has empty name",
        hall.id
    );
}

/// Validate that a membership is valid
pub fn assert_membership_invariants(membership: &Membership) {
    // User and hall IDs must not be nil
    debug_assert!(
        membership.user_id != Uuid::nil(),
        "Membership {} has nil user_id",
        membership.id
    );

    debug_assert!(
        membership.hall_id != Uuid::nil(),
        "Membership {} has nil hall_id",
        membership.id
    );
}

/// Validate that a member list is consistent
pub fn assert_member_list_invariants(members: &[MemberInfo], hall: &Hall) {
    // There should be at most one host
    let host_count = members.iter().filter(|m| m.is_host).count();
    debug_assert!(
        host_count <= 1,
        "Hall {} has {} hosts, expected 0 or 1",
        hall.id,
        host_count
    );

    // If hall has current_host_id, exactly one member should be marked as host
    if let Some(host_id) = hall.current_host_id {
        let host_in_list = members.iter().any(|m| m.user_id == host_id && m.is_host);
        let host_online = members.iter().any(|m| m.user_id == host_id && m.is_online);

        // Host should be in list and marked correctly (if online)
        debug_assert!(
            !host_online || host_in_list,
            "Hall {} host {:?} is online but not marked as host in member list",
            hall.id,
            host_id
        );
    }

    // Owners (Builders) should exist
    let has_builder = members.iter().any(|m| m.role == HallRole::HallBuilder);
    debug_assert!(
        has_builder || members.is_empty(),
        "Hall {} has members but no Builder",
        hall.id
    );
}

/// Validate that host assignment is valid for a given role
pub fn assert_host_role_valid(host_id: Uuid, role: HallRole) {
    debug_assert!(
        role.can_host(),
        "User {:?} assigned as host but has role {:?} which cannot host",
        host_id,
        role
    );
}

/// Validate role promotion is valid (not promoting above own level)
pub fn assert_valid_promotion(actor_role: HallRole, target_role: HallRole, new_role: HallRole) {
    debug_assert!(
        new_role < actor_role,
        "Promotion would set role {:?} equal to or above actor role {:?}",
        new_role,
        actor_role
    );

    debug_assert!(
        target_role < actor_role,
        "Cannot change role of member with equal or higher role: {:?} vs {:?}",
        target_role,
        actor_role
    );
}

/// Validate that a user ID is not nil
pub fn assert_user_id_valid(user_id: Uuid, context: &str) {
    debug_assert!(
        user_id != Uuid::nil(),
        "Nil user_id in context: {}",
        context
    );
}

/// Validate that a hall ID is not nil
pub fn assert_hall_id_valid(hall_id: Uuid, context: &str) {
    debug_assert!(
        hall_id != Uuid::nil(),
        "Nil hall_id in context: {}",
        context
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_hall() -> Hall {
        Hall {
            id: Uuid::new_v4(),
            name: "Test Hall".to_string(),
            description: None,
            owner_id: Uuid::new_v4(),
            created_at: Utc::now(),
            active_parlor: None,
            current_host_id: None,
            election_epoch: 0,
        }
    }

    #[test]
    fn test_valid_hall() {
        let hall = make_hall();
        assert_hall_invariants(&hall);
    }

    #[test]
    fn test_hall_with_host() {
        let mut hall = make_hall();
        hall.current_host_id = Some(Uuid::new_v4());
        hall.election_epoch = 1;
        assert_hall_invariants(&hall);
    }

    #[test]
    fn test_valid_membership() {
        let membership = Membership::new(Uuid::new_v4(), Uuid::new_v4(), HallRole::HallAgent);
        assert_membership_invariants(&membership);
    }

    #[test]
    fn test_host_role_valid() {
        assert_host_role_valid(Uuid::new_v4(), HallRole::HallAgent);
        assert_host_role_valid(Uuid::new_v4(), HallRole::HallBuilder);
    }

    #[test]
    #[should_panic(expected = "cannot host")]
    fn test_fellow_cannot_host() {
        assert_host_role_valid(Uuid::new_v4(), HallRole::HallFellow);
    }
}

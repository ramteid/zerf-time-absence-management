/// Canonical role constants used across the application.
pub const ROLE_ADMIN: &str = "admin";
pub const ROLE_ASSISTANT: &str = "assistant";
pub const ROLE_EMPLOYEE: &str = "employee";
pub const ROLE_TEAM_LEAD: &str = "team_lead";

/// Normalize a stored or client-provided role value (trim whitespace, lowercase).
/// All role comparisons must go through this to handle legacy/padded values.
#[inline]
pub fn normalize_role(role: &str) -> String {
    role.trim().to_ascii_lowercase()
}

/// Returns true when the role matches the assistant role.
/// Assistant policy is the canonical switch for fixed-target and flextime behavior.
/// We intentionally do not infer this from weekly_hours to avoid changing behavior
/// for non-assistant users that temporarily have zero hours.
#[inline]
pub fn is_assistant_role(role: &str) -> bool {
    normalize_role(role) == ROLE_ASSISTANT
}

/// Returns true when the role matches the admin role.
#[inline]
pub fn is_admin_role(role: &str) -> bool {
    normalize_role(role) == ROLE_ADMIN
}

/// Returns true when the role matches the team_lead role.
#[inline]
pub fn is_team_lead_role(role: &str) -> bool {
    normalize_role(role) == ROLE_TEAM_LEAD
}

/// Returns true for any leadership role (team_lead or admin) that can
/// review submissions and manage team members.
#[inline]
pub fn is_lead_role(role: &str) -> bool {
    matches!(normalize_role(role).as_str(), ROLE_TEAM_LEAD | ROLE_ADMIN)
}

/// Admin subjects can only be approved by other active admins.
#[inline]
pub fn can_approve_admin_subjects(role: &str, active: bool) -> bool {
    active && is_admin_role(role)
}

/// Non-admin subjects can be approved by any active lead (team_lead or admin).
#[inline]
pub fn can_approve_non_admin_subjects(role: &str, active: bool) -> bool {
    active && is_lead_role(role)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `normalize_role` must strip surrounding whitespace and lowercase the value.
    #[test]
    fn normalize_role_trims_and_lowercases() {
        assert_eq!(normalize_role("  Admin  "), "admin");
        assert_eq!(normalize_role("TEAM_LEAD"), "team_lead");
        assert_eq!(normalize_role("employee"), "employee");
        assert_eq!(normalize_role(""), "");
    }

    /// Each `is_*_role` predicate must match exactly its own role after
    /// normalization and reject all other roles.
    #[test]
    fn role_predicates_identify_correct_roles() {
        assert!(is_assistant_role("assistant"));
        assert!(is_assistant_role(" ASSISTANT "));
        assert!(!is_assistant_role("admin"));
        assert!(!is_assistant_role("employee"));

        assert!(is_admin_role("admin"));
        assert!(is_admin_role("  Admin "));
        assert!(!is_admin_role("team_lead"));

        assert!(is_team_lead_role("team_lead"));
        assert!(is_team_lead_role("TEAM_LEAD"));
        assert!(!is_team_lead_role("admin"));
        assert!(!is_team_lead_role("employee"));
    }

    /// `is_lead_role` must return true for both team_lead and admin.
    #[test]
    fn is_lead_role_accepts_team_lead_and_admin() {
        assert!(is_lead_role("team_lead"));
        assert!(is_lead_role("admin"));
        assert!(is_lead_role(" Admin "));
        assert!(!is_lead_role("employee"));
        assert!(!is_lead_role("assistant"));
    }

    /// `can_approve_admin_subjects` requires the approver to be an active admin.
    #[test]
    fn can_approve_admin_subjects_requires_active_admin() {
        assert!(can_approve_admin_subjects("admin", true));
        // Inactive admin must not approve.
        assert!(!can_approve_admin_subjects("admin", false));
        // Team lead can never approve admin subjects regardless of active flag.
        assert!(!can_approve_admin_subjects("team_lead", true));
        assert!(!can_approve_admin_subjects("employee", true));
    }

    /// `can_approve_non_admin_subjects` accepts any active team_lead or admin.
    #[test]
    fn can_approve_non_admin_subjects_accepts_any_active_lead() {
        assert!(can_approve_non_admin_subjects("team_lead", true));
        assert!(can_approve_non_admin_subjects("admin", true));
        // Inactive leads must not approve.
        assert!(!can_approve_non_admin_subjects("team_lead", false));
        assert!(!can_approve_non_admin_subjects("admin", false));
        // Employees and assistants are never eligible.
        assert!(!can_approve_non_admin_subjects("employee", true));
        assert!(!can_approve_non_admin_subjects("assistant", true));
    }
}

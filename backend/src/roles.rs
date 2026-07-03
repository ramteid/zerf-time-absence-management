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

/// Returns true when a user is expected to submit weekly timesheets at all.
/// Assistants have no fixed target schedule and no mandatory submission
/// workflow; users with `weekly_hours <= 0` are non-booking users by the same
/// policy the submission-reminder scheduler already uses (see
/// `UserDb::get_active_non_assistant_users`). Week-completeness checks
/// (Submissions tile, team report, monthly PDF upload gating) must use the
/// same exemption the reminder uses — otherwise a zero-hour user is nagged by
/// "weeks missing" indicators for a reminder that will never actually fire.
#[inline]
pub fn has_submission_obligation(role: &str, weekly_hours: f64) -> bool {
    !is_assistant_role(role) && weekly_hours > 0.0
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

    /// `has_submission_obligation` requires a non-assistant role AND positive
    /// weekly hours; either condition failing means no obligation.
    #[test]
    fn has_submission_obligation_requires_non_assistant_and_positive_hours() {
        assert!(has_submission_obligation("employee", 40.0));
        assert!(has_submission_obligation("team_lead", 20.0));
        // Assistants never have a submission obligation, regardless of hours.
        assert!(!has_submission_obligation("assistant", 40.0));
        assert!(!has_submission_obligation("assistant", 0.0));
        // Non-assistants with zero (or negative, defensively) weekly hours
        // are non-booking users and have no obligation either.
        assert!(!has_submission_obligation("employee", 0.0));
        assert!(!has_submission_obligation("team_lead", -1.0));
    }
}

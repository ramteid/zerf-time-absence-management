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

pub const ROLE_ADMIN: &str = "admin";
pub const ROLE_ASSISTANT: &str = "assistant";
pub const ROLE_EMPLOYEE: &str = "employee";
pub const ROLE_TEAM_LEAD: &str = "team_lead";

/// Normalize a stored or client-provided role value before any policy checks.
#[inline]
pub fn normalize_role(role: &str) -> String {
    role.trim().to_ascii_lowercase()
}

/// Assistant role policy is the canonical switch for fixed-target and flextime behavior.
/// We intentionally do not infer this from weekly_hours to avoid changing behavior
/// for non-assistant users that temporarily have zero hours.
#[inline]
pub fn is_assistant_role(role: &str) -> bool {
    let normalized_role = normalize_role(role);
    let is_assistant = normalized_role == ROLE_ASSISTANT;
    tracing::debug!(
        target: "zerf::assistant_role",
        raw_role = %role,
        normalized_role = %normalized_role,
        is_assistant,
        "evaluated assistant role"
    );
    is_assistant
}

#[inline]
pub fn is_admin_role(role: &str) -> bool {
    normalize_role(role) == ROLE_ADMIN
}

#[inline]
pub fn is_team_lead_role(role: &str) -> bool {
    normalize_role(role) == ROLE_TEAM_LEAD
}

#[inline]
pub fn is_lead_role(role: &str) -> bool {
    matches!(normalize_role(role).as_str(), ROLE_TEAM_LEAD | ROLE_ADMIN)
}

#[inline]
pub fn can_approve_admin_subjects(role: &str, active: bool) -> bool {
    active && is_admin_role(role)
}

#[inline]
pub fn can_approve_non_admin_subjects(role: &str, active: bool) -> bool {
    active && is_lead_role(role)
}

pub const ROLE_ASSISTANT: &str = "assistant";

/// Assistant role policy is the canonical switch for fixed-target and flextime behavior.
/// We intentionally do not infer this from weekly_hours to avoid changing behavior
/// for non-assistant users that temporarily have zero hours.
#[inline]
pub fn is_assistant_role(role: &str) -> bool {
    role == ROLE_ASSISTANT
}

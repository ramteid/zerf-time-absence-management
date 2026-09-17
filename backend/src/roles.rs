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

/// Display order for role-grouped user listings (team lead, employee,
/// assistant, admin) — mirrors the frontend's identical grouping in
/// `frontend/src/lib/domain/users.js` (`sortUsersByRoleThenName`), used
/// wherever a roster of users is rendered or exported (e.g. the combined
/// team timesheet PDF).
#[inline]
pub fn role_sort_rank(role: &str) -> u8 {
    match normalize_role(role).as_str() {
        ROLE_TEAM_LEAD => 0,
        ROLE_EMPLOYEE => 1,
        ROLE_ASSISTANT => 2,
        ROLE_ADMIN => 3,
        _ => 4,
    }
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

/// Returns true when the app may *demand* that a user's weeks be accounted
/// for — nothing more.
///
/// This does **not** say who submits. Everybody records, submits and has their
/// weeks approved through the same week-level workflow, assistants included:
/// entries are submitted a week at a time and approved a week at a time, and
/// until that has happened the hours count nowhere — which is exactly what the
/// payroll report's approval gate relies on.
///
/// What assistants and zero-hour users lack is a target schedule. There is no
/// amount of booked time anybody can require of them, so nothing about their
/// week can be called "missing": no submission reminder, and no "weeks missing"
/// verdict on the Submissions tile, in the team report, or in the monthly PDF
/// and payroll gates. Those checks must use the same exemption the reminder
/// uses (see `UserDb::get_active_non_assistant_users`) — otherwise a zero-hour
/// user is nagged by an indicator for a reminder that will never fire.
#[inline]
pub fn has_submission_obligation(role: &str, weekly_hours: f64) -> bool {
    !is_assistant_role(role) && weekly_hours > 0.0
}

/// True when the app places a work target on this contract.
///
/// A work target is an amount of time the app expects somebody to work. Two
/// kinds of contract have none. Assistants are paid for the hours they are
/// present, so there is nothing to fall short of and no flextime account to
/// fall short into. Accounts that do not track time record nothing at all.
///
/// This is the single answer to that question. It decides who gets a pattern of
/// working weekdays written for them, and — just as importantly — whose
/// recorded pattern is read back. Somebody who moves from employee to assistant
/// keeps their old pattern rows, because a report covering the months they were
/// an employee still has to be able to read them; what changes is that no
/// calculation consults those rows while the contract has no work target.
#[inline]
pub fn has_work_target(role: &str, tracks_time: bool) -> bool {
    tracks_time && !is_assistant_role(role)
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

    /// `has_work_target` requires time tracking AND a non-assistant role.
    /// Both halves matter: an assistant who tracks time still has no target,
    /// and an employee who does not track time has nothing to measure.
    #[test]
    fn has_work_target_requires_tracking_and_a_non_assistant_role() {
        assert!(has_work_target("employee", true));
        assert!(has_work_target("team_lead", true));
        assert!(has_work_target("admin", true));
        assert!(has_work_target(" EMPLOYEE ", true));
        // Assistants never have one, however their time is recorded.
        assert!(!has_work_target("assistant", true));
        assert!(!has_work_target(" Assistant ", true));
        // Neither does anybody whose time is not tracked at all.
        assert!(!has_work_target("employee", false));
        assert!(!has_work_target("admin", false));
    }

    /// `role_sort_rank` must order team_lead, employee, assistant, admin,
    /// with any unrecognised role sorting last.
    #[test]
    fn role_sort_rank_orders_team_lead_employee_assistant_admin() {
        assert!(role_sort_rank("team_lead") < role_sort_rank("employee"));
        assert!(role_sort_rank("employee") < role_sort_rank("assistant"));
        assert!(role_sort_rank("assistant") < role_sort_rank("admin"));
        assert!(role_sort_rank("admin") < role_sort_rank("bogus"));
        // Normalization (trim/lowercase) applies here too.
        assert_eq!(role_sort_rank(" ADMIN "), role_sort_rank("admin"));
    }
}

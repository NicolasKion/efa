//! Actions on the fit's settings.

use super::{Action, Outcome};
use crate::fit::Fit;

/// Set one of the fit's settings: mode, security, damage and target profiles.
pub(super) fn apply(fit: &mut Fit, action: Action) -> Outcome {
    match action {
        Action::SetMode(mode) => fit.mode = Some(mode),
        Action::SetSecStatus(sec) => fit.sec_status = sec,
        Action::SetSystemSecurity(security) => fit.system_security = security,
        Action::SetDamageProfile(profile) => {
            fit.damage_profile = profile;
        }
        Action::SetTargetProfile(profile) => fit.target_profile = profile,
        _ => unreachable!("not a settings action"),
    }
    Outcome::default()
}

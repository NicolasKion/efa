//! Eve Fleet Simulator's JSON (service/port/efs.py), export only. The numbers come from the
//! engine; see [`crate::engine::Engine::efs`].

use crate::{
    engine::{EfsContext, Engine},
    fit::Fit,
};

/// The fit as EFS JSON.
pub fn export(engine: &Engine, fit: &Fit, cx: &EfsContext<'_>) -> Result<String, String> {
    let data = engine.efs(fit, cx).ok_or("the fit has no ship")?;
    serde_json::to_string(&data).map_err(|e| e.to_string())
}

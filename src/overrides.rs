//! pyfa's attribute overrides (eos/saveddata/override.py, service/attribute.py): a value per type
//! and attribute that stands in for the type's own base value while overrides are on, for every
//! item of that type in every fit.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::data::TypeId;

/// Overridden base values by type, then by attribute ID.
pub type Values = BTreeMap<TypeId, BTreeMap<i32, f64>>;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Overrides {
    /// pyfa's Global > Turn Overrides On/Off. Off by default, as in pyfa.
    pub enabled: bool,
    pub values: Values,
}

impl Overrides {
    /// The overridden value of one attribute of a type, whether or not overrides are on.
    pub fn get(&self, type_id: TypeId, attr: i32) -> Option<f64> {
        self.values.get(&type_id)?.get(&attr).copied()
    }

    /// The overridden attributes of a type, whether or not overrides are on.
    pub fn of(&self, type_id: TypeId) -> Option<&BTreeMap<i32, f64>> {
        self.values.get(&type_id)
    }

    /// The attributes overridden for a type right now: none while overrides are off.
    pub fn in_effect(&self, type_id: TypeId) -> Option<&BTreeMap<i32, f64>> {
        self.enabled.then(|| self.of(type_id)).flatten()
    }

    /// The values the engine should use: none while overrides are off.
    pub fn active(&self) -> Values {
        if self.enabled {
            self.values.clone()
        } else {
            Values::new()
        }
    }

    pub fn set(&mut self, type_id: TypeId, attr: i32, value: f64) {
        self.values.entry(type_id).or_default().insert(attr, value);
    }

    /// Drop one override. Returns whether there was one.
    pub fn remove(&mut self, type_id: TypeId, attr: i32) -> bool {
        let Some(attrs) = self.values.get_mut(&type_id) else {
            return false;
        };
        let removed = attrs.remove(&attr).is_some();
        if attrs.is_empty() {
            self.values.remove(&type_id);
        }
        removed
    }

    /// Drop every override of a type, pyfa's "Remove Overrides for Item".
    pub fn clear_type(&mut self, type_id: TypeId) {
        self.values.remove(&type_id);
    }

    /// pyfa's override file: one `typeID,attributeID,value` line per override.
    pub fn to_csv(&self) -> String {
        self.values
            .iter()
            .flat_map(|(type_id, attrs)| {
                attrs
                    .iter()
                    .map(move |(attr, value)| format!("{type_id},{attr},{value}\n"))
            })
            .collect()
    }

    /// Add the overrides of a pyfa override file. Returns how many were read; blank lines are
    /// skipped, and anything else that does not parse is an error naming the line.
    pub fn import_csv(&mut self, text: &str) -> Result<usize, String> {
        let mut read = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parsed = (|| {
                let mut parts = line.split(',').map(str::trim);
                let type_id = parts.next()?.parse().ok()?;
                let attr = parts.next()?.parse().ok()?;
                let value: f64 = parts.next()?.parse().ok()?;
                (parts.next().is_none() && value.is_finite()).then_some((type_id, attr, value))
            })();
            read.push(parsed.ok_or_else(|| format!("line {}: {line}", n + 1))?);
        }
        let count = read.len();
        for (type_id, attr, value) in read {
            self.set(type_id, attr, value);
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removing_the_last_override_drops_the_type() {
        let mut o = Overrides::default();
        o.set(587, 263, 1000.0);
        o.set(587, 265, 500.0);
        assert!(o.remove(587, 263));
        assert!(!o.remove(587, 263));
        assert_eq!(o.get(587, 265), Some(500.0));
        assert!(o.remove(587, 265));
        assert!(o.values.is_empty());
    }

    #[test]
    fn nothing_is_in_effect_while_off() {
        let mut o = Overrides::default();
        o.set(587, 263, 1000.0);
        assert!(o.in_effect(587).is_none());
        assert!(o.active().is_empty());
        o.enabled = true;
        assert_eq!(o.in_effect(587).and_then(|a| a.get(&263)), Some(&1000.0));
        assert_eq!(o.active().len(), 1);
    }

    #[test]
    fn csv_round_trips_in_pyfas_format() {
        let mut o = Overrides::default();
        o.set(587, 263, 1000.5);
        o.set(3170, 64, 2.0);
        let text = o.to_csv();
        assert_eq!(text, "587,263,1000.5\n3170,64,2\n");
        let mut back = Overrides::default();
        assert_eq!(back.import_csv(&format!("{text}\n")), Ok(2));
        assert_eq!(back.values, o.values);
        assert!(back.import_csv("587,x,1").is_err());
    }

    #[test]
    fn old_saves_without_overrides_load() {
        let saved: crate::store::Saved = serde_json::from_str("{}").expect("empty save loads");
        assert_eq!(saved.settings.overrides, Overrides::default());
        let mut o = Overrides {
            enabled: true,
            ..Overrides::default()
        };
        o.set(587, 263, 1.5);
        let json = serde_json::to_string(&o).expect("serializes");
        let back: Overrides = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back, o);
    }
}

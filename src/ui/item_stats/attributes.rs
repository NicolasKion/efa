//! The Attributes tab: pyfa's grouping of attributes into Fitting, Structure, Armor and so on,
//! and its CSV export (gui/builtinItemStatsViews/itemAttributes.py, attributeGrouping.py).

use std::{collections::HashSet, fmt::Write as _};

use crate::{
    data::{Catalog, category},
    icons::Icon,
    item_data::{AttrInfo, ItemData, REQUIRED_SKILL_ATTRS, TypeInfo},
};

use super::{format, tree::Node};

/// Icon pyfa shows for attributes without one of their own.
const UNKNOWN_ICON: i32 = 0;
/// Structures, which pyfa groups like ships.
const CATEGORY_STRUCTURE: i32 = 65;

/// An attribute's value now and before bonuses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Value {
    pub id: i32,
    pub current: f64,
    pub base: f64,
}

/// What every attribute line needs to be described.
pub struct Describe<'a> {
    pub data: &'a ItemData,
    pub catalog: &'a Catalog,
    /// The type's own attributes; others are only listed in the raw view, as in pyfa.
    pub own: Option<&'a TypeInfo>,
    /// Fitted items have a base value column.
    pub fitted: bool,
    /// Attributes whose base value comes from an attribute override.
    pub overridden: &'a [i32],
    pub raw: bool,
}

/// Attributes shown together under a label of their own, such as the four damage types.
const SUBGROUPS: [(&[&str], &str); 5] = [
    (
        &[
            "emDamage",
            "thermalDamage",
            "kineticDamage",
            "explosiveDamage",
        ],
        "Damage",
    ),
    (
        &[
            "emDamageResonance",
            "thermalDamageResonance",
            "kineticDamageResonance",
            "explosiveDamageResonance",
        ],
        "Resistances",
    ),
    (
        &[
            "armorEmDamageResonance",
            "armorThermalDamageResonance",
            "armorKineticDamageResonance",
            "armorExplosiveDamageResonance",
        ],
        "Resistances",
    ),
    (
        &[
            "shieldEmDamageResonance",
            "shieldThermalDamageResonance",
            "shieldKineticDamageResonance",
            "shieldExplosiveDamageResonance",
        ],
        "Resistances",
    ),
    (
        &[
            "scanRadarStrength",
            "scanMagnetometricStrength",
            "scanGravimetricStrength",
            "scanLadarStrength",
        ],
        "Sensor Strengths",
    ),
];

fn subgroup(name: &str) -> Option<(&'static [&'static str], &'static str)> {
    SUBGROUPS
        .iter()
        .find(|(members, _)| members.contains(&name))
        .copied()
}

/// One of pyfa's attribute groups: its heading and its attributes, in order.
struct Group {
    label: &'static str,
    attrs: Vec<(String, Option<&'static str>)>,
}

fn group(label: &'static str, attrs: &[&str]) -> Group {
    Group {
        label,
        attrs: attrs.iter().map(|a| ((*a).to_owned(), None)).collect(),
    }
}

const FITTING: &[&str] = &[
    "cpuOutput",
    "powerOutput",
    "upgradeCapacity",
    "hiSlots",
    "medSlots",
    "lowSlots",
    "serviceSlots",
    "turretSlotsLeft",
    "launcherSlotsLeft",
    "upgradeSlotsLeft",
    "cpu",
    "power",
    "rigSize",
    "upgradeCost",
];
const STRUCTURE: &[&str] = &[
    "hp",
    "capacity",
    "mass",
    "volume",
    "agility",
    "droneCapacity",
    "droneBandwidth",
    "generalMiningHoldCapacity",
    "specialIceHoldCapacity",
    "specialGasHoldCapacity",
    "specialMineralHoldCapacity",
    "specialSalvageHoldCapacity",
    "specialShipHoldCapacity",
    "specialSmallShipHoldCapacity",
    "specialMediumShipHoldCapacity",
    "specialLargeShipHoldCapacity",
    "specialIndustrialShipHoldCapacity",
    "specialAmmoHoldCapacity",
    "specialCommandCenterHoldCapacity",
    "specialPlanetaryCommoditiesHoldCapacity",
    "specialColonyResourcesHoldCapacity",
    "structureDamageLimit",
    "specialSubsystemHoldCapacity",
    "emDamageResonance",
    "thermalDamageResonance",
    "kineticDamageResonance",
    "explosiveDamageResonance",
];
const ARMOR: &[&str] = &[
    "armorHP",
    "armorDamageLimit",
    "armorEmDamageResonance",
    "armorThermalDamageResonance",
    "armorKineticDamageResonance",
    "armorExplosiveDamageResonance",
];
const SHIELD: &[&str] = &[
    "shieldCapacity",
    "shieldRechargeRate",
    "shieldDamageLimit",
    "shieldEmDamageResonance",
    "shieldExplosiveDamageResonance",
    "shieldKineticDamageResonance",
    "shieldThermalDamageResonance",
];
const EWAR_RESISTS: &[&str] = &[
    "ECMResistance",
    "remoteAssistanceImpedance",
    "remoteRepairImpedance",
    "energyWarfareResistance",
    "sensorDampenerResistance",
    "stasisWebifierResistance",
    "targetPainterResistance",
    "weaponDisruptionResistance",
];
const CAPACITOR: &[&str] = &["capacitorCapacity", "rechargeRate"];
const TARGETING: &[&str] = &[
    "maxTargetRange",
    "maxRange",
    "maxLockedTargets",
    "signatureRadius",
    "optimalSigRadius",
    "scanResolution",
    "proximityRange",
    "falloff",
    "trackingSpeed",
    "scanRadarStrength",
    "scanMagnetometricStrength",
    "scanGravimetricStrength",
    "scanLadarStrength",
];
const SHARED_FACILITIES: &[&str] = &[
    "fleetHangarCapacity",
    "shipMaintenanceBayCapacity",
    "maxJumpClones",
];
const FIGHTER_FACILITIES: &[&str] = &[
    "fighterCapacity",
    "fighterTubes",
    "fighterLightSlots",
    "fighterSupportSlots",
    "fighterHeavySlots",
    "fighterStandupLightSlots",
    "fighterStandupSupportSlots",
    "fighterStandupHeavySlots",
];
const ON_DEATH: &[&str] = &[
    "onDeathDamageEM",
    "onDeathDamageTherm",
    "onDeathDamageKin",
    "onDeathDamageExp",
    "onDeathAOERadius",
    "onDeathSignatureRadius",
];
const JUMP_SYSTEMS: &[&str] = &[
    "jumpDriveCapacitorNeed",
    "jumpDriveRange",
    "jumpDriveConsumptionType",
    "jumpDriveConsumptionAmount",
    "jumpPortalCapacitorNeed",
    "jumpDriveDuration",
    "specialFuelBayCapacity",
    "jumpPortalConsumptionMassFactor",
    "jumpPortalDuration",
];
const PROPULSION: &[&str] = &["maxVelocity"];
const FIGHTERS: &[&str] = &[
    "mass",
    "maxVelocity",
    "agility",
    "volume",
    "signatureRadius",
    "fighterSquadronMaxSize",
    "fighterRefuelingTime",
    "fighterSquadronOrbitRange",
];

/// "Can Fit To": the groups and ships a module is limited to, labelled Group and Ship.
fn can_fit_to() -> Group {
    let groups = (1..=20).map(|i| (format!("canFitShipGroup{i:02}"), Some("Group")));
    let ships = (1..=20).map(|i| (format!("canFitShipType{i}"), Some("Ship")));
    Group {
        label: "Can Fit To",
        attrs: groups.chain(ships).collect(),
    }
}

/// The groups pyfa shows for an item category, in order.
fn groups_for(category_id: i32) -> Vec<Group> {
    match category_id {
        category::FIGHTER => vec![
            group("Fighter", FIGHTERS),
            group("Shield", SHIELD),
            group("Targeting", TARGETING),
        ],
        category::SHIP | category::DRONE | CATEGORY_STRUCTURE => vec![
            group("Fitting", FITTING),
            group("Structure", STRUCTURE),
            group("Armor", ARMOR),
            group("Shield", SHIELD),
            group("Electronic Warfare", EWAR_RESISTS),
            group("Capacitor", CAPACITOR),
            group("Targeting", TARGETING),
            group("Shared Facilities", SHARED_FACILITIES),
            group("Fighter Squadron Facilities", FIGHTER_FACILITIES),
            group("On Death", ON_DEATH),
            group("Jump Drive Systems", JUMP_SYSTEMS),
            group("Propulsion", PROPULSION),
            can_fit_to(),
        ],
        _ => vec![group("Fitting", FITTING), can_fit_to()],
    }
}

/// Builds the tree the way pyfa's `PopulateList` does, attribute by attribute.
struct Builder<'a> {
    describe: &'a Describe<'a>,
    values: Vec<(String, Value)>,
    processed: HashSet<String>,
    required_skill_attrs: HashSet<i32>,
}

impl Builder<'_> {
    fn value(&self, name: &str) -> Option<Value> {
        self.values.iter().find(|(n, _)| n == name).map(|&(_, v)| v)
    }

    /// The type's own information on an attribute; pyfa lists nothing else outside the raw view.
    fn info(&self, id: i32) -> Option<&AttrInfo> {
        let own = self.describe.own?;
        own.attr(id)?;
        self.describe.data.attr(id)
    }

    fn line(&self, name: &str, value: Value, label: Option<&str>) -> Option<Node> {
        let d = self.describe;
        let info = self.info(value.id);
        if !d.raw {
            let grouped = subgroup(name).is_some();
            let blank = value.current == 0.0 && value.base == 0.0;
            let published = info.is_some_and(|i| i.published);
            if (!grouped && blank) || !published || self.required_skill_attrs.contains(&value.id) {
                return None;
            }
        }
        let mut shown_name = match (d.raw, info.and_then(|i| i.display_name.as_deref())) {
            (false, Some(display)) => label.unwrap_or(display).to_owned(),
            _ => name.to_owned(),
        };
        if d.overridden.contains(&value.id) {
            shown_name.push_str(" (overridden)");
        }
        let icon = match info {
            Some(info) => info.icon.map(Icon::Id),
            None => Some(Icon::Id(UNKNOWN_ICON)),
        };
        let shown = |v: f64| {
            if d.raw {
                format::raw(v)
            } else {
                format::value(d.data, d.catalog, d.data.attr(value.id), v)
            }
        };
        let mut cells = vec![shown(value.current)];
        if d.fitted {
            cells.push(shown(value.base));
        }
        Some(Node {
            key: format!("a:{name}"),
            icon,
            label: shown_name,
            cells,
            children: Vec::new(),
            link: None,
        })
    }

    fn add(&mut self, parent: &mut Vec<Node>, name: &str, label: Option<&str>) {
        if self.processed.contains(name) {
            return;
        }
        let Some(value) = self.value(name) else {
            return;
        };
        if let Some(node) = self.line(name, value, label) {
            parent.push(node);
            self.processed.insert(name.to_owned());
        }
    }

    /// A labelled node of grouped attributes, if any of them are shown.
    fn subgroup_node(&mut self, key: &str, members: &[&str], label: &str) -> Option<Node> {
        let mut node = Node::new(key, label);
        for member in members {
            self.add(&mut node.children, member, None);
        }
        (!node.children.is_empty()).then_some(node)
    }
}

/// The Attributes tab's tree for an item of `category_id` with these values.
pub fn tree(describe: &Describe<'_>, category_id: i32, values: &[Value]) -> Vec<Node> {
    let names: Vec<(String, Value)> = values
        .iter()
        .map(|v| {
            let name = describe
                .data
                .attr(v.id)
                .map_or_else(|| v.id.to_string(), |a| a.name.clone());
            (name, *v)
        })
        .collect();
    let mut b = Builder {
        describe,
        values: names,
        processed: HashSet::new(),
        required_skill_attrs: REQUIRED_SKILL_ATTRS
            .iter()
            .flat_map(|&(skill, level)| [skill, level])
            .collect(),
    };
    let mut root = Vec::new();
    for group in groups_for(category_id) {
        let mut header = Node::new(format!("g:{}", group.label), group.label);
        for (attr, label) in &group.attrs {
            if let Some((members, sub_label)) = subgroup(attr) {
                let key = format!("g:{}/{attr}", group.label);
                if let Some(node) = b.subgroup_node(&key, members, sub_label) {
                    header.children.push(node);
                }
                continue;
            }
            b.add(&mut header.children, attr, *label);
        }
        if !header.children.is_empty() {
            root.push(header);
        }
    }

    // Everything the groups did not take, by internal name.
    let mut rest: Vec<String> = b.values.iter().map(|(n, _)| n.clone()).collect();
    rest.sort();
    for name in rest {
        if let Some((members, label)) = subgroup(&name) {
            let key = format!("g:/{name}");
            if let Some(node) = b.subgroup_node(&key, members, label) {
                root.push(node);
            }
            continue;
        }
        b.add(&mut root, &name, None);
    }
    root
}

/// pyfa's "Export Item Stats" CSV: every attribute, with internal and display names.
pub fn csv(data: &ItemData, own: Option<&TypeInfo>, values: &[Value]) -> String {
    let mut out = String::from("ID,Internal Name,Friendly Name,Modified Value,Base Value\r\n");
    for v in values {
        let info = data.attr(v.id);
        let on_type = own.and_then(|o| o.attr(v.id));
        let fields = [
            info.map_or_else(String::new, |i| i.id.to_string()),
            info.map_or_else(|| v.id.to_string(), |i| i.name.clone()),
            info.and_then(|i| i.display_name.clone())
                .unwrap_or_default(),
            format::raw(v.current),
            on_type.map_or_default(format::raw),
        ];
        let line: Vec<String> = fields.iter().map(|f| csv_field(f)).collect();
        let _ = write!(out, "{}\r\n", line.join(","));
    }
    out
}

/// Quote a field the way Python's csv module does: only when it has to.
fn csv_field(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_quotes_only_when_needed() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a, b"), "\"a, b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn grouped_attributes_find_their_label() {
        assert_eq!(subgroup("kineticDamage").map(|(_, l)| l), Some("Damage"));
        assert_eq!(
            subgroup("armorEmDamageResonance").map(|(_, l)| l),
            Some("Resistances")
        );
        assert!(subgroup("cpu").is_none());
    }
}

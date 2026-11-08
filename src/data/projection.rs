//! What the Projected, Command and Fighters tabs need from the data: environmental effects in
//! pyfa's menu tree, which items can be projected at all, and fighter classes and abilities.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::{Localized, PhbTypeDogma, SdeType, TypeId, category, effect_category, read};
use std::path::Path;

mod group {
    pub const EFFECT_BEACON: i32 = 920;
    pub const MASSIVE_ENVIRONMENTS: i32 = 1882;
    pub const NON_INTERACTABLE: i32 = 1975;
    pub const ABYSSAL_HAZARDS: i32 = 1971;
    pub const ABYSSAL_ENVIRONMENT: i32 = 1983;
    pub const COMMAND_BURST: i32 = 1770;
}

/// Effects that make an item project onto ships around it, rather than act on a whole system:
/// abyssal weather and the localized clouds.
pub const PROJECTED_ENV_EFFECTS: [i32; 9] = [7050, 7051, 7053, 7058, 7059, 7060, 7061, 7062, 7063];

/// pyfa's `abyssal_mapping`: which environment names a weather item, by the weather's name
/// without its tier.
const ABYSSAL_WEATHER: [(&str, TypeId); 5] = [
    ("caustic_toxin_weather", 47862),
    ("darkness_weather", 47863),
    ("infernal_weather", 47864),
    ("electric_storm_weather", 47865),
    ("xenon_gas_weather", 47866),
];

/// Single items pyfa lists by ID under its own headings.
const DRIFTER_CRISIS: TypeId = 87294;
const TRIGLAVIAN: [TypeId; 3] = [87177, 87164, 74002];
const FW_INSURGENCY: [TypeId; 3] = [79839, 92609, 95625];
const IHUB: [(TypeId, &str); 4] = [
    (87950, "Electric"),
    (87949, "Plasma"),
    (87951, "Exotic"),
    (87815, "Gamma"),
];

mod attr {
    pub const SQUADRON_IS: [(i32, super::FighterClass); 6] = [
        (2212, super::FighterClass::Light),
        (2213, super::FighterClass::Support),
        (2214, super::FighterClass::Heavy),
        (2740, super::FighterClass::Light),
        (2741, super::FighterClass::Support),
        (2742, super::FighterClass::Heavy),
    ];
    pub const SQUADRON_MAX_SIZE: i32 = 2215;
}

/// One leaf of the environmental effects menu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvEntry {
    pub type_id: TypeId,
    pub label: String,
}

/// A level of pyfa's "Add Environmental Effect" menu: submenus first, then effects.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvMenu {
    pub groups: Vec<(String, EnvMenu)>,
    pub items: Vec<EnvEntry>,
}

impl EnvMenu {
    fn group(&mut self, name: &str) -> &mut EnvMenu {
        let at = self
            .groups
            .iter()
            .position(|(n, _)| n == name)
            .unwrap_or_else(|| {
                self.groups.push((name.to_owned(), EnvMenu::default()));
                self.groups.len() - 1
            });
        &mut self.groups[at].1
    }

    /// pyfa's `Group.sort`: submenus by name, effects by label, all the way down.
    fn sort(&mut self) {
        self.groups.sort_by(|a, b| a.0.cmp(&b.0));
        for (_, group) in &mut self.groups {
            group.sort();
        }
        self.items.sort_by(|a, b| a.label.cmp(&b.label));
    }

    fn is_empty(&self) -> bool {
        self.groups.is_empty() && self.items.is_empty()
    }
}

/// An environmental effect item, which the catalog's types leave out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvType {
    /// What the Projected tab calls it, as pyfa names it there.
    pub name: String,
    pub icon_id: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FighterClass {
    Light,
    Heavy,
    Support,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FighterType {
    pub class: Option<FighterClass>,
    /// Squadron size before skills and ship bonuses.
    pub squadron_size: Option<u32>,
}

/// Data behind projection and fighters, built with the rest of the catalog.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Projection {
    pub env_menu: EnvMenu,
    #[serde(serialize_with = "crate::sorted::map")]
    pub env_types: HashMap<TypeId, EnvType>,
    /// Modules, drones and fighters with an effect that reaches other ships, which pyfa calls
    /// projected items.
    #[serde(serialize_with = "crate::sorted::set")]
    pub projectable: HashSet<TypeId>,
    /// Command bursts, which make a fit worth offering as a fleet booster.
    #[serde(serialize_with = "crate::sorted::set")]
    pub command_bursts: HashSet<TypeId>,
    #[serde(serialize_with = "crate::sorted::map")]
    pub fighters: HashMap<TypeId, FighterType>,
    /// Fighter ability names, by ability ID.
    #[serde(serialize_with = "crate::sorted::map")]
    pub ability_names: HashMap<i32, String>,
}

impl Projection {
    pub fn env_name(&self, type_id: TypeId) -> Option<&str> {
        self.env_types.get(&type_id).map(|t| t.name.as_str())
    }

    pub fn fighter_class(&self, type_id: TypeId) -> Option<FighterClass> {
        self.fighters.get(&type_id).and_then(|f| f.class)
    }

    pub fn squadron_size(&self, type_id: TypeId) -> Option<u32> {
        self.fighters.get(&type_id).and_then(|f| f.squadron_size)
    }

    pub fn ability_name(&self, ability: i32) -> String {
        self.ability_names
            .get(&ability)
            .cloned()
            .unwrap_or_else(|| format!("Ability {ability}"))
    }
}

/// An item that may end up in the environmental effects menu.
struct EnvCandidate {
    id: TypeId,
    name: String,
    group_id: i32,
    published: bool,
    icon_id: Option<i32>,
    projected: bool,
}

/// Collects what [`Projection`] needs while the catalog walks the types once.
pub(super) struct Builder<'a> {
    effect_category: &'a HashMap<i32, i32>,
    env: Vec<EnvCandidate>,
    projection: Projection,
}

impl<'a> Builder<'a> {
    pub(super) fn new(effect_category: &'a HashMap<i32, i32>) -> Self {
        Self {
            effect_category,
            env: Vec::new(),
            projection: Projection::default(),
        }
    }

    /// Look at one type of the dump, whatever its category.
    pub(super) fn consider(
        &mut self,
        id: TypeId,
        t: &SdeType,
        category_id: i32,
        dogma: Option<&PhbTypeDogma>,
    ) {
        let effects = || {
            dogma
                .into_iter()
                .flat_map(|d| d.effects.iter().map(|e| e.effect_id))
        };
        let attr = |wanted: i32| {
            dogma.and_then(|d| {
                d.attributes
                    .iter()
                    .find(|a| a.attribute_id == wanted)
                    .map(|a| a.value)
            })
        };
        let env_group = matches!(
            t.group_id,
            group::EFFECT_BEACON
                | group::MASSIVE_ENVIRONMENTS
                | group::NON_INTERACTABLE
                | group::ABYSSAL_HAZARDS
                | group::ABYSSAL_ENVIRONMENT
        ) || t.group_id == IHUB_GROUP;
        let listed = id == DRIFTER_CRISIS
            || TRIGLAVIAN.contains(&id)
            || FW_INSURGENCY.contains(&id)
            || IHUB.iter().any(|&(i, _)| i == id);
        if env_group || listed {
            let projected = effects().any(|e| PROJECTED_ENV_EFFECTS.contains(&e));
            // Most non-interactable objects are scenery; only the weathers matter.
            if t.group_id != group::NON_INTERACTABLE || projected {
                self.env.push(EnvCandidate {
                    id,
                    name: t.name.en.clone(),
                    group_id: t.group_id,
                    published: t.published,
                    icon_id: t.icon_id,
                    projected,
                });
            }
        }
        if matches!(
            category_id,
            category::MODULE | category::DRONE | category::FIGHTER
        ) && effects()
            .any(|e| self.effect_category.get(&e).copied() == Some(effect_category::TARGET))
        {
            self.projection.projectable.insert(id);
        }
        if t.group_id == group::COMMAND_BURST {
            self.projection.command_bursts.insert(id);
        }
        if category_id == category::FIGHTER {
            let class = attr::SQUADRON_IS
                .iter()
                .find(|&&(a, _)| attr(a).is_some_and(|v| v > 0.0))
                .map(|&(_, class)| class);
            self.projection.fighters.insert(
                id,
                FighterType {
                    class,
                    squadron_size: attr(attr::SQUADRON_MAX_SIZE).map(|v| v as u32),
                },
            );
        }
    }

    pub(super) fn finish(mut self, dir: &Path) -> Result<Projection, String> {
        let abilities: HashMap<String, SdeAbility> = read(dir, "sde/fighterabilities.json")?;
        self.projection.ability_names = abilities
            .into_iter()
            .filter_map(|(id, a)| Some((id.parse().ok()?, a.display_name?.en)))
            .collect();
        let (menu, types) = env_menu(&self.env);
        self.projection.env_menu = menu;
        self.projection.env_types = types;
        Ok(self.projection)
    }
}

/// The group of sovereignty hub upgrades that change a whole system.
const IHUB_GROUP: i32 = 4839;

#[derive(Deserialize)]
struct SdeAbility {
    #[serde(rename = "displayName")]
    display_name: Option<Localized>,
}

/// pyfa's `customName`: what it calls Caustic clouds.
fn custom_name(name: &str) -> String {
    name.replace("Caustic", "Tachyon")
}

fn squeeze(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// pyfa's `getEffectBeacons`: published effect beacons whose names contain one of `groups`,
/// named without the group and the `garbage` words. With a single group the effects go straight
/// into the menu, otherwise into a submenu per group.
fn effect_beacons(candidates: &[EnvCandidate], groups: &[&str], garbage: &[&str]) -> EnvMenu {
    let compacted = groups.len() <= 1;
    let mut garbage_words = vec!["System Effects", "Effects"];
    garbage_words.extend_from_slice(garbage);
    let strip = |text: &str| {
        let mut text = text.to_owned();
        for word in &garbage_words {
            text = text.replace(word, "");
        }
        squeeze(&text)
    };
    let mut menu = EnvMenu::default();
    let beacons = candidates
        .iter()
        .filter(|c| c.group_id == group::EFFECT_BEACON && c.published);
    for beacon in beacons {
        let Some(group) = groups.iter().find(|g| beacon.name.contains(*g)) else {
            continue;
        };
        let entry = EnvEntry {
            type_id: beacon.id,
            label: strip(&beacon.name.replace(group, "")),
        };
        if compacted {
            menu.items.push(entry);
        } else {
            menu.group(&strip(group)).items.push(entry);
        }
    }
    // pyfa lists these in whatever order its set gives; by name reads better and stays put.
    menu.sort();
    menu
}

/// pyfa's `getAbyssalWeather`: weathers by environment and tier, then the localized clouds.
fn abyssal_weather(candidates: &[EnvCandidate], names: &mut HashMap<TypeId, String>) -> EnvMenu {
    let environment = |id: TypeId| {
        candidates
            .iter()
            .find(|c| c.id == id && c.group_id == group::ABYSSAL_ENVIRONMENT)
            .map(|c| c.name.clone())
    };
    let mut menu = EnvMenu::default();
    let weathers = candidates.iter().filter(|c| {
        c.projected
            && matches!(
                c.group_id,
                group::MASSIVE_ENVIRONMENTS | group::NON_INTERACTABLE
            )
    });
    for weather in weathers {
        let Some((base, tier)) = weather
            .name
            .char_indices()
            .nth_back(1)
            .map(|(at, _)| (&weather.name[..at], &weather.name[weather.name.len() - 1..]))
        else {
            continue;
        };
        let Some(env_name) = ABYSSAL_WEATHER
            .iter()
            .find(|&&(prefix, _)| prefix == base)
            .and_then(|&(_, env)| environment(env))
        else {
            continue;
        };
        let label = format!("{env_name} {tier}");
        names.insert(weather.id, label.clone());
        menu.group(&env_name).items.push(EnvEntry {
            type_id: weather.id,
            label,
        });
    }
    menu.sort();

    let mut localized = EnvMenu::default();
    let clouds = candidates
        .iter()
        .filter(|c| c.projected && c.group_id == group::ABYSSAL_HAZARDS);
    for cloud in clouds {
        let name = custom_name(&cloud.name);
        let Some(kind) = ["Bioluminescence", "Tachyon", "Filament"]
            .into_iter()
            .find(|k| name.contains(k))
        else {
            continue;
        };
        localized.group(kind).items.push(EnvEntry {
            type_id: cloud.id,
            label: name,
        });
    }
    if !localized.is_empty() {
        localized.sort();
        menu.groups.push(("Localized".to_owned(), localized));
    }
    menu
}

/// Items pyfa adds by ID, labelled with their name or a given short label.
fn listed(candidates: &[EnvCandidate], items: &[(TypeId, Option<&str>)]) -> Vec<EnvEntry> {
    items
        .iter()
        .filter_map(|&(id, label)| {
            let c = candidates.iter().find(|c| c.id == id)?;
            Some(EnvEntry {
                type_id: id,
                label: label.map_or_else(|| c.name.clone(), str::to_owned),
            })
        })
        .collect()
}

/// pyfa's `AddEnvironmentEffect.getData`, and the names the Projected tab shows.
fn env_menu(candidates: &[EnvCandidate]) -> (EnvMenu, HashMap<TypeId, EnvType>) {
    let mut names: HashMap<TypeId, String> = HashMap::new();
    let mut menu = EnvMenu::default();
    menu.groups.push((
        "Metaliminal Storm".to_owned(),
        effect_beacons(
            candidates,
            &["Electrical", "Exotic", "Gamma", "Plasma"],
            &["Metaliminal", "Storm", "Matter", "Ray", "Firestorm"],
        ),
    ));
    menu.groups.push((
        "Wormhole".to_owned(),
        effect_beacons(
            candidates,
            &[
                "Black Hole",
                "Cataclysmic Variable",
                "Magnetar",
                "Pulsar",
                "Red Giant",
                "Wolf Rayet",
            ],
            &[],
        ),
    ));
    menu.groups.push((
        "Abyssal Weather".to_owned(),
        abyssal_weather(candidates, &mut names),
    ));
    menu.groups.push((
        "Sansha Incursion".to_owned(),
        effect_beacons(candidates, &["Sansha Incursion"], &[]),
    ));
    let mut drifter = effect_beacons(candidates, &["Drifter Incursion"], &[]);
    drifter
        .items
        .extend(listed(candidates, &[(DRIFTER_CRISIS, None)]));
    menu.groups.push(("Drifter Incursion".to_owned(), drifter));
    let as_listed = |ids: &[TypeId]| ids.iter().map(|&id| (id, None)).collect::<Vec<_>>();
    menu.groups.push((
        "Triglavian Invasion".to_owned(),
        EnvMenu {
            groups: Vec::new(),
            items: listed(candidates, &as_listed(&TRIGLAVIAN)),
        },
    ));
    menu.groups.push((
        "FW/Insurgency".to_owned(),
        EnvMenu {
            groups: Vec::new(),
            items: listed(candidates, &as_listed(&FW_INSURGENCY)),
        },
    ));
    let ihub: Vec<(TypeId, Option<&str>)> =
        IHUB.iter().map(|&(id, label)| (id, Some(label))).collect();
    menu.groups.push((
        "IHub Upgrades".to_owned(),
        EnvMenu {
            groups: Vec::new(),
            items: listed(candidates, &ihub),
        },
    ));

    let types = candidates
        .iter()
        .map(|c| {
            let name = names.remove(&c.id).unwrap_or_else(|| custom_name(&c.name));
            (
                c.id,
                EnvType {
                    name,
                    icon_id: c.icon_id,
                },
            )
        })
        .collect();
    (menu, types)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: TypeId, name: &str, group_id: i32, projected: bool) -> EnvCandidate {
        EnvCandidate {
            id,
            name: name.to_owned(),
            group_id,
            published: true,
            icon_id: None,
            projected,
        }
    }

    #[test]
    fn beacons_are_named_like_pyfa() {
        let candidates = [
            candidate(1, "Class 1 Black Hole Effects", group::EFFECT_BEACON, false),
            candidate(
                2,
                "Strong Metaliminal Plasma Firestorm",
                group::EFFECT_BEACON,
                false,
            ),
            candidate(
                3,
                "Weak Metaliminal Exotic Matter Storm",
                group::EFFECT_BEACON,
                false,
            ),
        ];
        let wormholes = effect_beacons(&candidates, &["Black Hole", "Pulsar"], &[]);
        assert_eq!(wormholes.groups[0].0, "Black Hole");
        assert_eq!(wormholes.groups[0].1.items[0].label, "Class 1");
        let storms = effect_beacons(
            &candidates,
            &["Electrical", "Exotic", "Gamma", "Plasma"],
            &["Metaliminal", "Storm", "Matter", "Ray", "Firestorm"],
        );
        let labels: Vec<(&str, &str)> = storms
            .groups
            .iter()
            .map(|(g, m)| (g.as_str(), m.items[0].label.as_str()))
            .collect();
        assert_eq!(labels, [("Exotic", "Weak"), ("Plasma", "Strong")]);
    }

    #[test]
    fn weather_is_named_after_its_environment() {
        let candidates = [
            candidate(
                47862,
                "Exotic Particle Storm",
                group::ABYSSAL_ENVIRONMENT,
                false,
            ),
            candidate(
                47384,
                "caustic_toxin_weather_1",
                group::NON_INTERACTABLE,
                true,
            ),
            candidate(47436, "Small Caustic Cloud", group::ABYSSAL_HAZARDS, true),
        ];
        let mut names = HashMap::new();
        let menu = abyssal_weather(&candidates, &mut names);
        assert_eq!(menu.groups[0].0, "Exotic Particle Storm");
        assert_eq!(menu.groups[0].1.items[0].label, "Exotic Particle Storm 1");
        assert_eq!(names[&47384], "Exotic Particle Storm 1");
        let (localized, clouds) = &menu.groups[1];
        assert_eq!(localized, "Localized");
        assert_eq!(clouds.groups[0].0, "Tachyon");
        assert_eq!(clouds.groups[0].1.items[0].label, "Small Tachyon Cloud");
    }
}

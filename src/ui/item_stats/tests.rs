//! The stats window's trees against the real data. Skipped when no dump is present.

use std::{collections::HashSet, path::Path};

use super::*;
use crate::{
    data::{Catalog, Slot, category},
    engine::Engine,
};

const DATA: &str = "data/eve";
const DAMAGE_MULTIPLIER: i32 = 64;

fn setup() -> Option<(Catalog, ItemData)> {
    if !Path::new(DATA).join("phobos/metadata.json").exists() {
        eprintln!("no EVE data in {DATA}, skipping");
        return None;
    }
    let catalog =
        Catalog::load_cached(Path::new(DATA), Path::new("data/cache")).expect("the dump loads");
    let wanted: HashSet<TypeId> = catalog.types.keys().copied().collect();
    let key = crate::data::cache_key(Path::new(DATA));
    let data = ItemData::load_cached(
        Path::new(DATA),
        Path::new("data/cache"),
        key.as_deref(),
        &wanted,
    )
    .expect("the item data loads");
    Some((catalog, data))
}

fn labels(nodes: &[Node]) -> Vec<&str> {
    nodes.iter().map(|n| n.label.as_str()).collect()
}

fn id(catalog: &Catalog, name: &str) -> TypeId {
    catalog
        .find(name)
        .unwrap_or_else(|| panic!("{name} exists"))
        .id
}

#[test]
fn ship_attributes_are_grouped_like_pyfa() {
    let Some((catalog, data)) = setup() else {
        return;
    };
    let rifter = id(&catalog, "Rifter");
    let info = data.type_info(rifter);
    let values: Vec<attributes::Value> = info
        .expect("the Rifter has attributes")
        .attrs
        .iter()
        .map(|&(id, v)| attributes::Value {
            id,
            current: v,
            base: v,
        })
        .collect();
    let describe = attributes::Describe {
        data: &data,
        catalog: &catalog,
        own: info,
        fitted: false,
        overridden: &[],
        raw: false,
    };
    let tree = attributes::tree(&describe, category::SHIP, &values);
    let top = labels(&tree);
    assert_eq!(&top[..4], ["Fitting", "Structure", "Armor", "Shield"]);
    let fitting = labels(&tree[0].children);
    assert!(fitting.contains(&"Powergrid Output"), "{fitting:?}");
    let resists = tree[2]
        .children
        .iter()
        .find(|n| n.label == "Resistances")
        .expect("armor resistances are grouped");
    assert_eq!(resists.children.len(), 4);
    assert!(
        resists.children[0].cells[0].ends_with('%'),
        "{:?}",
        resists.children[0].cells
    );

    // The raw view shows internal names and the skill attributes the normal view hides.
    let raw = attributes::tree(
        &attributes::Describe {
            raw: true,
            ..describe
        },
        category::SHIP,
        &values,
    );
    assert!(
        raw.iter()
            .flat_map(|n| std::iter::once(n).chain(&n.children))
            .any(|n| n.label == "requiredSkill1")
    );
}

#[test]
fn affected_by_groups_copies_of_a_module() {
    let Some((catalog, data)) = setup() else {
        return;
    };
    let engine = Engine::load(
        Path::new(DATA),
        Path::new("data/cache"),
        catalog.skills.clone(),
    )
    .expect("the engine loads");
    let eft = "[Rifter, T]\nGyrostabilizer II\nGyrostabilizer II\n\n\n200mm AutoCannon I\n";
    let (fit, _) = Fit::from_eft(&catalog, eft).expect("the fit parses");
    let inspection = engine
        .inspect(&fit, engine.all5(), FitItem::Module(Slot::High, 0))
        .expect("the turret is there");
    let names = affected::Names {
        catalog: &catalog,
        data: &data,
    };
    let by_item = affected::tree(&names, &inspection, affected::View::default(), Some(0));
    let gyros = by_item
        .iter()
        .find(|n| n.label == "Gyrostabilizer II x 2")
        .expect("both gyrostabilizers on one line");
    assert!(
        gyros.children.iter().any(|c| {
            c.label.starts_with("Damage Modifier * ") && c.label.ends_with("(penalized)")
        }),
        "{:?}",
        labels(&gyros.children)
    );
    assert!(matches!(
        gyros.link,
        Some((ref kind, Target::Fitted { fit: 0, .. })) if kind == "Module"
    ));
    let skill = by_item
        .iter()
        .find(|n| n.link.as_ref().is_some_and(|(kind, _)| kind == "Skill"))
        .expect("skills affect the turret");
    assert!(matches!(skill.link, Some((_, Target::Type(_)))));

    let view = affected::View {
        by_attribute: true,
        real_names: true,
    };
    let by_attr = affected::tree(&names, &inspection, view, Some(0));
    assert!(labels(&by_attr).contains(&"damageMultiplier"));
}

#[test]
fn requirements_and_dependents() {
    let Some((catalog, data)) = setup() else {
        return;
    };
    let reqs = requirements(&catalog, &data, id(&catalog, "Rifter"));
    assert_eq!(labels(&reqs), ["Minmatar Frigate  I"]);
    assert!(!reqs[0].children.is_empty(), "the frigate skill needs more");
    let deps = dependents(&catalog, &data, id(&catalog, "Minmatar Frigate"));
    let level1 = deps
        .iter()
        .find(|n| n.label == "Level I")
        .expect("frigates need level I");
    assert!(labels(&level1.children).contains(&"Rifter"));
}

#[test]
fn variations_compare_on_differing_attributes() {
    let Some((catalog, data)) = setup() else {
        return;
    };
    let items = catalog
        .variations_of(id(&catalog, "Gyrostabilizer II"))
        .to_vec();
    assert!(items.len() > 2);
    assert!(compare::columns(&data, &items).contains(&DAMAGE_MULTIPLIER));
}

#[test]
fn traits_read_like_pyfa() {
    let Some((catalog, data)) = setup() else {
        return;
    };
    let traits = data
        .type_info(id(&catalog, "Rifter"))
        .and_then(|i| i.traits.as_ref())
        .expect("the Rifter has bonuses");
    let html = text::traits_html(&catalog, traits);
    assert!(
        html.starts_with("<b>Minmatar Frigate bonuses (per skill level):</b><br />\n7.5% "),
        "{html}"
    );
    assert!(!html.contains("showinfo"), "{html}");
}

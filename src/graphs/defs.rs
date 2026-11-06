//! Every graph pyfa offers, with its axes, inputs and options, as pyfa defines them in
//! graphs/data/*/graph.py.

/// A quantity a graph can sweep, hold fixed, or plot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Param {
    Distance,
    Time,
    TgtSpeed,
    TgtSigRad,
    AtkSpeed,
    AtkAngle,
    TgtAngle,
    Resist,
    CapAmount,
    CapAmountT0,
    ShieldAmount,
    ShieldAmountT0,
    TgtMass,
    TgtInertia,
    // Outputs.
    Dps,
    Volley,
    Damage,
    NeutStr,
    WebStr,
    EcmStrMax,
    DampStrLockRange,
    TdStrOptimal,
    GdStrRange,
    TpStr,
    Rps,
    Total,
    CapRegen,
    ShieldRegen,
    Speed,
    Momentum,
    BumpSpeed,
    BumpDistance,
    WarpTime,
    LockTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unit {
    None,
    Km,
    M,
    S,
    Percent,
    MPerS,
    Au,
    Gj,
    GjPerS,
    Hp,
    HpPerS,
    Ehp,
    EhpPerS,
    Mkg,
    GkgMPerS,
    Degrees,
}

impl Unit {
    /// How pyfa writes the unit after a label, or None for unitless values.
    pub fn label(self) -> Option<&'static str> {
        Some(match self {
            Self::None => return None,
            Self::Km => "km",
            Self::M => "m",
            Self::S => "s",
            Self::Percent => "%",
            Self::MPerS => "m/s",
            Self::Au => "AU",
            Self::Gj => "GJ",
            Self::GjPerS => "GJ/s",
            Self::Hp => "HP",
            Self::HpPerS => "HP/s",
            Self::Ehp => "EHP",
            Self::EhpPerS => "EHP/s",
            Self::Mkg => "Mkg",
            Self::GkgMPerS => "Gkg⋅m/s",
            Self::Degrees => "°",
        })
    }
}

/// A quantity in a unit: what an axis or input is.
pub type Spec = (Param, Unit);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct XDef {
    pub param: Param,
    pub unit: Unit,
    pub label: &'static str,
    /// The input that gives this axis its range.
    pub main_input: Spec,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct YDef {
    pub param: Param,
    pub unit: Unit,
    pub label: &'static str,
    /// What the Y selector calls it, when different from the axis label.
    pub selector: Option<&'static str>,
}

impl YDef {
    pub fn selector_label(&self) -> &'static str {
        self.selector.unwrap_or(self.label)
    }
}

/// When an input or checkbox shows: for (x axis, y axis) pairs; None matches anything.
pub type Condition = (Option<Spec>, Option<Spec>);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputDef {
    pub param: Param,
    pub unit: Unit,
    pub label: &'static str,
    /// The game icon pyfa shows next to the field.
    pub icon: i32,
    /// Empty means "not set", which several graphs treat specially.
    pub default: Option<f64>,
    pub default_range: (f64, f64),
    pub main_tooltip: Option<&'static str>,
    pub tooltip: Option<&'static str>,
    pub conditions: &'static [Condition],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CheckboxDef {
    pub key: Checkbox,
    pub label: &'static str,
    pub default: bool,
    pub conditions: &'static [Condition],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Checkbox {
    AncReload,
    UseCapsim,
}

/// Speed and heading inputs drawn as a vector, as pyfa's damage graph does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VectorDef {
    pub length: Param,
    pub angle: Param,
    pub label: &'static str,
}

/// Extra columns the fit and target lists show for a graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Dps,
    Volley,
    Speed,
    Radius,
    ShieldRr,
    ArmorRr,
    HullRr,
    CapAmount,
    CapTime,
    Agility,
    WarpSpeed,
    WarpDistance,
    ScanResolution,
    ShieldAmount,
    ShieldTime,
    TargetResists,
    SigRadius,
    FullHp,
}

impl Column {
    pub fn header(self) -> &'static str {
        match self {
            Self::Dps => "DPS",
            Self::Volley => "Volley",
            Self::Speed => "Speed",
            Self::Radius => "Radius",
            Self::ShieldRr => "Shield RR",
            Self::ArmorRr => "Armor RR",
            Self::HullRr => "Hull RR",
            Self::CapAmount => "Cap",
            Self::CapTime => "Cap time",
            Self::Agility => "Align",
            Self::WarpSpeed => "Warp",
            Self::WarpDistance => "Warp range",
            Self::ScanResolution => "Scan res",
            Self::ShieldAmount => "Shield",
            Self::ShieldTime => "Shield regen",
            Self::TargetResists => "Resists",
            Self::SigRadius => "Sig",
            Self::FullHp => "HP",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Graph {
    DamageStats,
    EwarStats,
    RemoteReps,
    ShieldRegen,
    Capacitor,
    Mobility,
    WarpTime,
    LockTime,
}

const DISTANCE_ICON: i32 = 1391;
const TIME_ICON: i32 = 1392;
const SPEED_ICON: i32 = 1389;
const SIG_ICON: i32 = 1390;

const ALWAYS: &[Condition] = &[];
const DISTANCE_TIP: &str =
    "Distance between the attacker and the target, as seen in overview (surface-to-surface)";

impl Graph {
    /// In pyfa's menu order.
    pub const ALL: [Self; 8] = [
        Self::DamageStats,
        Self::EwarStats,
        Self::RemoteReps,
        Self::ShieldRegen,
        Self::Capacitor,
        Self::Mobility,
        Self::WarpTime,
        Self::LockTime,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::DamageStats => "Damage Stats",
            Self::EwarStats => "Electronic Warfare Stats",
            Self::RemoteReps => "Remote Repairs",
            Self::ShieldRegen => "Shield Regeneration",
            Self::Capacitor => "Capacitor",
            Self::Mobility => "Mobility",
            Self::WarpTime => "Warp Time",
            Self::LockTime => "Lock Time",
        }
    }

    /// The damage graph names its outputs "effective" unless resists are ignored, and the shield
    /// graph counts EHP or HP; both follow options.
    pub fn y_defs(self, ignore_resists: bool, effective_hp: bool) -> Vec<YDef> {
        let y = |param, unit, label| YDef {
            param,
            unit,
            label,
            selector: None,
        };
        let sel = |param, unit, label, selector| YDef {
            param,
            unit,
            label,
            selector: Some(selector),
        };
        match self {
            Self::DamageStats => {
                let (dps, volley, damage) = if ignore_resists {
                    ("DPS", "Volley", "Damage inflicted")
                } else {
                    (
                        "Effective DPS",
                        "Effective volley",
                        "Effective damage inflicted",
                    )
                };
                vec![
                    y(Param::Dps, Unit::None, dps),
                    y(Param::Volley, Unit::None, volley),
                    y(Param::Damage, Unit::None, damage),
                ]
            }
            Self::EwarStats => vec![
                sel(
                    Param::NeutStr,
                    Unit::None,
                    "Cap neutralized per second",
                    "Neuts: cap per second",
                ),
                sel(
                    Param::WebStr,
                    Unit::Percent,
                    "Speed reduction",
                    "Webs: speed reduction",
                ),
                sel(
                    Param::EcmStrMax,
                    Unit::None,
                    "Combined ECM strength",
                    "ECM: combined strength",
                ),
                sel(
                    Param::DampStrLockRange,
                    Unit::Percent,
                    "Lock range reduction",
                    "Damps: lock range reduction",
                ),
                sel(
                    Param::TdStrOptimal,
                    Unit::Percent,
                    "Turret optimal range reduction",
                    "TDs: turret optimal range reduction",
                ),
                sel(
                    Param::GdStrRange,
                    Unit::Percent,
                    "Missile flight range reduction",
                    "GDs: missile flight range reduction",
                ),
                sel(
                    Param::TpStr,
                    Unit::Percent,
                    "Signature radius increase",
                    "TPs: signature radius increase",
                ),
            ],
            Self::RemoteReps => vec![
                y(Param::Rps, Unit::HpPerS, "Repair speed"),
                y(Param::Total, Unit::Hp, "Total repaired"),
            ],
            Self::ShieldRegen => {
                let (amount, regen) = if effective_hp {
                    (Unit::Ehp, Unit::EhpPerS)
                } else {
                    (Unit::Hp, Unit::HpPerS)
                };
                vec![
                    y(Param::ShieldAmount, amount, "Shield amount"),
                    y(Param::ShieldRegen, regen, "Shield regen"),
                ]
            }
            Self::Capacitor => vec![
                y(Param::CapAmount, Unit::Gj, "Cap amount"),
                y(Param::CapRegen, Unit::GjPerS, "Cap regen"),
            ],
            Self::Mobility => vec![
                y(Param::Speed, Unit::MPerS, "Speed"),
                y(Param::Distance, Unit::Km, "Distance"),
                y(Param::Momentum, Unit::GkgMPerS, "Momentum"),
                sel(Param::BumpSpeed, Unit::MPerS, "Target speed", "Bump speed"),
                sel(
                    Param::BumpDistance,
                    Unit::Km,
                    "Target distance traveled",
                    "Bump distance",
                ),
            ],
            Self::WarpTime => vec![y(Param::WarpTime, Unit::S, "Warp time")],
            Self::LockTime => vec![y(Param::LockTime, Unit::S, "Lock time")],
        }
    }

    pub fn x_defs(self, effective_hp: bool) -> Vec<XDef> {
        let x = |param, unit, label, main_input| XDef {
            param,
            unit,
            label,
            main_input,
        };
        match self {
            Self::DamageStats => vec![
                x(
                    Param::Distance,
                    Unit::Km,
                    "Distance",
                    (Param::Distance, Unit::Km),
                ),
                x(Param::Time, Unit::S, "Time", (Param::Time, Unit::S)),
                x(
                    Param::TgtSpeed,
                    Unit::MPerS,
                    "Target speed",
                    (Param::TgtSpeed, Unit::Percent),
                ),
                x(
                    Param::TgtSpeed,
                    Unit::Percent,
                    "Target speed",
                    (Param::TgtSpeed, Unit::Percent),
                ),
                x(
                    Param::TgtSigRad,
                    Unit::M,
                    "Target signature radius",
                    (Param::TgtSigRad, Unit::Percent),
                ),
                x(
                    Param::TgtSigRad,
                    Unit::Percent,
                    "Target signature radius",
                    (Param::TgtSigRad, Unit::Percent),
                ),
            ],
            Self::EwarStats => vec![x(
                Param::Distance,
                Unit::Km,
                "Distance",
                (Param::Distance, Unit::Km),
            )],
            Self::RemoteReps => vec![
                x(
                    Param::Distance,
                    Unit::Km,
                    "Distance",
                    (Param::Distance, Unit::Km),
                ),
                x(Param::Time, Unit::S, "Time", (Param::Time, Unit::S)),
            ],
            Self::ShieldRegen => vec![
                x(Param::Time, Unit::S, "Time", (Param::Time, Unit::S)),
                x(
                    Param::ShieldAmount,
                    if effective_hp { Unit::Ehp } else { Unit::Hp },
                    "Shield amount",
                    (Param::ShieldAmount, Unit::Percent),
                ),
                x(
                    Param::ShieldAmount,
                    Unit::Percent,
                    "Shield amount",
                    (Param::ShieldAmount, Unit::Percent),
                ),
            ],
            Self::Capacitor => vec![
                x(Param::Time, Unit::S, "Time", (Param::Time, Unit::S)),
                x(
                    Param::CapAmount,
                    Unit::Gj,
                    "Cap amount",
                    (Param::CapAmount, Unit::Percent),
                ),
                x(
                    Param::CapAmount,
                    Unit::Percent,
                    "Cap amount",
                    (Param::CapAmount, Unit::Percent),
                ),
            ],
            Self::Mobility => vec![x(Param::Time, Unit::S, "Time", (Param::Time, Unit::S))],
            Self::WarpTime => vec![
                x(
                    Param::Distance,
                    Unit::Au,
                    "Distance",
                    (Param::Distance, Unit::Au),
                ),
                x(
                    Param::Distance,
                    Unit::Km,
                    "Distance",
                    (Param::Distance, Unit::Km),
                ),
            ],
            Self::LockTime => vec![x(
                Param::TgtSigRad,
                Unit::M,
                "Target signature radius",
                (Param::TgtSigRad, Unit::M),
            )],
        }
    }

    pub fn inputs(self) -> Vec<InputDef> {
        let input = |param, unit, label, icon, default, default_range| InputDef {
            param,
            unit,
            label,
            icon,
            default,
            default_range,
            main_tooltip: None,
            tooltip: None,
            conditions: ALWAYS,
        };
        match self {
            Self::DamageStats => vec![
                InputDef {
                    main_tooltip: Some(DISTANCE_TIP),
                    tooltip: Some(
                        "Distance between the attacker and the target, as seen in overview (surface-to-surface)\nWhen set, places the target that far away from the attacker\nWhen not set, attacker's weapons always hit the target",
                    ),
                    ..input(
                        Param::Distance,
                        Unit::Km,
                        "Distance",
                        DISTANCE_ICON,
                        None,
                        (0.0, 100.0),
                    )
                },
                InputDef {
                    tooltip: Some(
                        "When set, uses attacker's exact damage stats at a given time\nWhen not set, uses attacker's damage stats as shown in stats panel of main window",
                    ),
                    ..input(Param::Time, Unit::S, "Time", TIME_ICON, None, (0.0, 80.0))
                },
                input(
                    Param::TgtSpeed,
                    Unit::Percent,
                    "Target speed",
                    SPEED_ICON,
                    Some(100.0),
                    (0.0, 100.0),
                ),
                InputDef {
                    conditions: &[
                        (Some((Param::TgtSigRad, Unit::M)), None),
                        (Some((Param::TgtSigRad, Unit::Percent)), None),
                    ],
                    ..input(
                        Param::TgtSigRad,
                        Unit::Percent,
                        "Target signature",
                        SIG_ICON,
                        Some(100.0),
                        (100.0, 200.0),
                    )
                },
            ],
            Self::EwarStats => vec![
                input(
                    Param::Distance,
                    Unit::Km,
                    "Distance",
                    DISTANCE_ICON,
                    None,
                    (0.0, 100.0),
                ),
                input(
                    Param::Resist,
                    Unit::Percent,
                    "Target resistance",
                    1393,
                    Some(0.0),
                    (0.0, 100.0),
                ),
            ],
            Self::RemoteReps => vec![
                InputDef {
                    tooltip: Some(
                        "When set, uses repairing ship's exact RR stats at a given time\nWhen not set, uses repairing ship's RR stats as shown in stats panel of main window",
                    ),
                    ..input(Param::Time, Unit::S, "Time", TIME_ICON, None, (0.0, 80.0))
                },
                InputDef {
                    main_tooltip: Some(
                        "Distance between the repairing ship and the target, as seen in overview (surface-to-surface)",
                    ),
                    tooltip: Some(
                        "Distance between the repairing ship and the target, as seen in overview (surface-to-surface)",
                    ),
                    ..input(
                        Param::Distance,
                        Unit::Km,
                        "Distance",
                        DISTANCE_ICON,
                        None,
                        (0.0, 100.0),
                    )
                },
            ],
            Self::ShieldRegen => vec![
                InputDef {
                    conditions: &[(Some((Param::Time, Unit::S)), None)],
                    ..input(
                        Param::Time,
                        Unit::S,
                        "Time",
                        TIME_ICON,
                        Some(120.0),
                        (0.0, 300.0),
                    )
                },
                InputDef {
                    conditions: &[
                        (Some((Param::ShieldAmount, Unit::Ehp)), None),
                        (Some((Param::ShieldAmount, Unit::Hp)), None),
                        (Some((Param::ShieldAmount, Unit::Percent)), None),
                    ],
                    ..input(
                        Param::ShieldAmount,
                        Unit::Percent,
                        "Shield amount",
                        1384,
                        Some(25.0),
                        (0.0, 100.0),
                    )
                },
                InputDef {
                    conditions: &[(Some((Param::Time, Unit::S)), None)],
                    ..input(
                        Param::ShieldAmountT0,
                        Unit::Percent,
                        "Starting shield amount",
                        1384,
                        Some(0.0),
                        (0.0, 100.0),
                    )
                },
            ],
            Self::Capacitor => vec![
                InputDef {
                    conditions: &[(Some((Param::Time, Unit::S)), None)],
                    ..input(
                        Param::Time,
                        Unit::S,
                        "Time",
                        TIME_ICON,
                        Some(120.0),
                        (0.0, 300.0),
                    )
                },
                InputDef {
                    conditions: &[
                        (Some((Param::CapAmount, Unit::Gj)), None),
                        (Some((Param::CapAmount, Unit::Percent)), None),
                    ],
                    ..input(
                        Param::CapAmount,
                        Unit::Percent,
                        "Cap amount",
                        1668,
                        Some(25.0),
                        (0.0, 100.0),
                    )
                },
                InputDef {
                    conditions: &[(Some((Param::Time, Unit::S)), None)],
                    ..input(
                        Param::CapAmountT0,
                        Unit::Percent,
                        "Starting cap amount",
                        1668,
                        Some(100.0),
                        (0.0, 100.0),
                    )
                },
            ],
            Self::Mobility => vec![
                input(
                    Param::Time,
                    Unit::S,
                    "Time",
                    TIME_ICON,
                    Some(10.0),
                    (0.0, 30.0),
                ),
                // The target defaults are a random carrier or FAX, as in pyfa.
                InputDef {
                    tooltip: Some("Defined in millions of kilograms"),
                    conditions: &[
                        (None, Some((Param::BumpSpeed, Unit::MPerS))),
                        (None, Some((Param::BumpDistance, Unit::Km))),
                    ],
                    ..input(
                        Param::TgtMass,
                        Unit::Mkg,
                        "Target mass",
                        76,
                        Some(1300.0),
                        (100.0, 2500.0),
                    )
                },
                InputDef {
                    tooltip: Some("Inertia Modifier attribute value of the target ship"),
                    conditions: &[(None, Some((Param::BumpDistance, Unit::Km)))],
                    ..input(
                        Param::TgtInertia,
                        Unit::None,
                        "Target inertia factor",
                        1401,
                        Some(0.015),
                        (0.03, 0.1),
                    )
                },
            ],
            Self::WarpTime => vec![
                input(
                    Param::Distance,
                    Unit::Au,
                    "Distance",
                    DISTANCE_ICON,
                    Some(20.0),
                    (0.0, 50.0),
                ),
                input(
                    Param::Distance,
                    Unit::Km,
                    "Distance",
                    DISTANCE_ICON,
                    Some(1000.0),
                    (150.0, 5000.0),
                ),
            ],
            Self::LockTime => vec![input(
                Param::TgtSigRad,
                Unit::M,
                "Target signature",
                SIG_ICON,
                None,
                (25.0, 500.0),
            )],
        }
    }

    pub fn checkboxes(self) -> &'static [CheckboxDef] {
        match self {
            Self::RemoteReps => &[CheckboxDef {
                key: Checkbox::AncReload,
                label: "Reload ancillary RRs",
                default: true,
                conditions: ALWAYS,
            }],
            Self::Capacitor => &[CheckboxDef {
                key: Checkbox::UseCapsim,
                label: "Use capacitor simulator",
                default: true,
                conditions: &[(
                    Some((Param::Time, Unit::S)),
                    Some((Param::CapAmount, Unit::Gj)),
                )],
            }],
            _ => &[],
        }
    }

    pub fn has_targets(self) -> bool {
        self == Self::DamageStats
    }

    pub fn src_vector(self) -> Option<VectorDef> {
        (self == Self::DamageStats).then_some(VectorDef {
            length: Param::AtkSpeed,
            angle: Param::AtkAngle,
            label: "Attacker",
        })
    }

    pub fn tgt_vector(self) -> Option<VectorDef> {
        (self == Self::DamageStats).then_some(VectorDef {
            length: Param::TgtSpeed,
            angle: Param::TgtAngle,
            label: "Target",
        })
    }

    pub fn src_columns(self) -> &'static [Column] {
        match self {
            Self::DamageStats => &[Column::Dps, Column::Volley, Column::Speed, Column::Radius],
            Self::EwarStats => &[],
            Self::RemoteReps => &[Column::ShieldRr, Column::ArmorRr, Column::HullRr],
            Self::ShieldRegen => &[Column::ShieldAmount, Column::ShieldTime],
            Self::Capacitor => &[Column::CapAmount, Column::CapTime],
            Self::Mobility => &[Column::Speed, Column::Agility],
            Self::WarpTime => &[Column::WarpSpeed, Column::WarpDistance],
            Self::LockTime => &[Column::ScanResolution],
        }
    }

    pub fn tgt_columns(self, ignore_resists: bool) -> Vec<Column> {
        let mut cols = Vec::new();
        if !ignore_resists {
            cols.push(Column::TargetResists);
        }
        cols.extend([
            Column::Speed,
            Column::SigRadius,
            Column::Radius,
            Column::FullHp,
        ]);
        cols
    }

    /// The graph options menu (pyfa's ☰ icon) has entries for these graphs.
    pub fn has_options(self) -> bool {
        matches!(self, Self::DamageStats | Self::RemoteReps | Self::EwarStats)
    }
}

/// Whether an input or checkbox applies to the chosen axes.
pub fn conditions_met(conditions: &[Condition], x: Spec, y: Spec) -> bool {
    conditions.is_empty()
        || conditions
            .iter()
            .any(|(cx, cy)| cx.is_none_or(|c| c == x) && cy.is_none_or(|c| c == y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_x_axis_has_its_main_input() {
        for graph in Graph::ALL {
            let inputs = graph.inputs();
            for x in graph.x_defs(true) {
                assert!(
                    inputs.iter().any(|i| (i.param, i.unit) == x.main_input),
                    "{} {:?}",
                    graph.name(),
                    x.main_input
                );
            }
        }
    }

    #[test]
    fn capsim_checkbox_only_for_time_against_cap() {
        let cb = Graph::Capacitor.checkboxes()[0];
        let time = (Param::Time, Unit::S);
        assert!(conditions_met(
            cb.conditions,
            time,
            (Param::CapAmount, Unit::Gj)
        ));
        assert!(!conditions_met(
            cb.conditions,
            time,
            (Param::CapRegen, Unit::GjPerS)
        ));
    }
}

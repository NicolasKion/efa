# pyfa context menus

152 menu entries in 35 right-click contexts (pyfa ContextMenu registry plus 4 hand-built menus).
gyfa progress is tracked per line below; fitting rows have Shift/Ctrl selection and Ctrl/Alt "all similar" now.
Biggest gaps: projected pane (29 incl. env effects), market item actions (9), module extras (variations, mutaplasmids, spool, RAH, fill, skills), drone/fighter quantity and abilities, command fits and generic links, ship menu (tactical mode, sec status), additions copy/paste, stats-pane and graph options.

Legend: `[x]` in gyfa, `[~]` partial, `[ ]` missing. Line format: label | when it shows | what it does | pyfa file (in gui/builtinContextMenus/ unless a path is given).

## How pyfa builds menus

- gui/contextMenu.py: `getMenu(callingWindow, mainItem, selection, *fullContexts)`. For each fullContext `(srcContext, itemContext)` in order, every registered menu class is asked `display()`; matching entries are appended in registration order (gui/builtinContextMenus/__init__.py). A separator goes between consecutive source contexts that produced entries. No separators inside one context.
- itemContext is the clicked item's market category display name ("Module", "Charge", "Drone", "Implant", ...) and fills `{}` in labels.
- Holding Ctrl while opening shows entries hidden by preference (`visibilitySetting`: ammoPattern, changeAffectingSkills, metaSwap, project, moduleFill, spoolup, additionsCopyPaste; all on by default).
- Ctrl/Alt while clicking an action applies it to all "similar" items (same type) instead of the selection, where noted. "multi" = acts on the whole selection.
- Registration order (= order inside a context): fitOpenNewTab, fitAddBrowse, fitAddCurrentlyOpen, envEffectAdd, commandFitAdd, commandLinkAdd, targetProfile/adder, graphFitAmmoPicker | shipModeChange, moduleAmmoChange, moduleSpool, moduleRahPattern, boosterSideEffects, fighterAbilities, resistMode, targetProfile/editor | itemStats, itemMarketJump, fitSystemSecurity, fitPilotSecurity, shipJump | itemRemove, itemAmountChange, itemProjectionRange, droneSplitStack, itemVariationChange, itemMutations, moduleFill, moduleMutatedExport, skillAffectors | itemFill, droneAddStack, cargoAdd, cargoFill, cargoAddAmmo, itemProject, ammoToDmgPattern, implantSetApply, implantSetSave | priceOptions | damagePatternChange | factorReload, targetProfile/switcher | graph options | additionsExport*/Import. (The `|` groups are code comments only, not visible separators.)

## Fitting view (gui/builtinViews/fittingView.py)

Contexts in order: fittingModule (non-empty module, incl. rigs/subsystems), fittingCharge (if module has a charge loaded), fittingMode (T3D tactical mode row), then always fittingShip. Separator between each. Right-click on an empty slot gives only fittingShip.

### fittingModule
- [x] Charge | module has loadable charges (fitting or projected module) | submenu by module kind: turrets "Long Range" list with per-family submenus ("Less Damage"/"More Damage"), missiles by damage type (EM/Thermal/Explosive/Kinetic/Mixed), mining crystals by ore class, else flat list; last item "Empty" unloads. Applies to all similar modules by default (pref ammoChangeAll), Ctrl/Alt inverts to selection only. GuiChangeLocalModuleChargesCommand | moduleAmmoChange.py
- [x] Spoolup Cycles | module group "Precursor Weapon" or "Mutadaptive Remote Armor Repairer" (pref spoolup) | checkable list of cycle counts (default marked "(default)", min..max, current) + "Reset". GuiChangeLocalModuleSpoolCommand | moduleSpool.py
- [ ] RAH Damage Pattern | module group "Armor Resistance Shift Hardener" | checkable: "Fit Pattern", "Do Not Adapt", separator, damage profiles tree (builtin + user, categories as submenus). sFit.setRahPattern | moduleRahPattern.py, shared/patterns.py
- [ ] {Module} Stats | always | opens item stats window (Shift reuses last window) | itemStats.py
- [x] {Module} Market Group | item has a market group (mutated: base item's) | jumps market browser to the item | itemMarketJump.py
- [x] Remove {Module} | always | removes selection (multi); Ctrl/Alt removes all similar. GuiRemoveLocalModuleCommand | itemRemove.py
- [x] Variations | item has 2+ variations, not hidden (pref metaSwap) | submenu of meta variants grouped by meta group (disabled header rows), sorted by meta level; unfittable ones disabled. Swaps selection modules with identical variation set; Ctrl/Alt all similar. GuiChangeLocalModuleMetasCommand | itemVariationChange.py
- [ ] Apply Mutaplasmid / Revert to {base} | item has mutaplasmids or is mutated | not mutated: submenu of mutaplasmids (Glorified sorted after base name); mutated: plain entry reverting to base item. GuiConvertMutatedLocalModuleCommand / GuiRevertMutatedLocalModuleCommand | itemMutations.py
- [x] Fill With {Module} | always (pref moduleFill) | clones the module into all free slots of that rack. GuiFillWithClonedLocalModulesCommand | moduleFill.py
- [ ] Copy Module to Clipboard | module is mutated | copies EFT mutant block (item, mutaplasmid, attrs) | moduleMutatedExport.py
- [ ] Change {Module} Skills | some skill modifies a non-zero attr of the item (pref changeAffectingSkills) | submenu per affecting skill, each with "Not Learned", "Level 0".."Level 5" (checked current); changes character skill level and refreshes fit | skillAffectors.py

### fittingCharge
- [ ] {Charge} Stats | charge loaded | stats window for the charge | itemStats.py
- [x] {Charge} Market Group | charge has market group | jump to charge in market | itemMarketJump.py
- [ ] Change {Charge} Skills | skills affect charge attrs | as above, on chargeModifiedAttributes | skillAffectors.py

### fittingMode
- [ ] Tactical Mode Stats | right-click the mode row | stats window for the mode item | itemStats.py

### fittingShip
- [x] Tactical Mode | ship has modes (T3 destroyers) | radio submenu Defense/Propulsion/Sharpshooter (or Primary/Secondary/Tertiary). GuiChangeShipModeCommand | shipModeChange.py
- [ ] Ship Stats / Citadel Stats | always | stats window for the ship | itemStats.py
- [ ] Citadel System Security | fit is a structure | radio: High Security, Low Security, Null Security, W-Space. GuiChangeFitSystemSecurityCommand | fitSystemSecurity.py
- [x] Pilot Security Status | ship is Pacifier, Enforcer, Marshal, Sidewinder, Cobra or Python | radio: "Character (x.xx)", "Custom (x)" (dialog, -10..5), separator, presets -10,-8,-6,-4,-2,0,1,2,3,4,5. GuiChangeFitPilotSecurityCommand | fitPilotSecurity.py
- [x] Open in Fitting Browser | unless fitting browser already shows this ship's fit list | switches to fittings tab, stage 3 for the ship | shipJump.py
- [ ] Change Ship Skills / Change Citadel Skills | skills affect ship attrs | skill level submenu as above | skillAffectors.py

## Drones pane (gui/builtinAdditionPanes/droneView.py)

Contexts: droneItem (row clicked), separator, droneItemMisc (always).

### droneItem
- [ ] Drone Stats | row clicked | stats window | itemStats.py. Not yet: no item stats window
- [x] Drone Market Group | has market group | market jump | itemMarketJump.py
- [x] Remove Drone Stack | row clicked | removes whole stacks of selection (multi). GuiRemoveLocalDronesCommand | itemRemove.py
- [x] Change Drone Quantity | row clicked | dialog "New Amount", sets stack size. GuiChangeLocalDroneAmountCommand | itemAmountChange.py
- [x] Split Drone Stack | stack amount > 1 | dialog for amount, splits into a new stack. GuiSplitLocalDroneStackCommand | droneSplitStack.py
- [x] Variations | 2+ variations | variation submenu, swaps selected drones with same variation set. GuiChangeLocalDroneMetasCommand | itemVariationChange.py
- [ ] Apply Mutaplasmid / Revert to {base} | drone has mutaplasmids or is mutated | as modules. GuiConvertMutatedLocalDroneCommand / GuiRevertMutatedLocalDroneCommand | itemMutations.py. Not yet: needs mutaplasmid data in the catalog, mutated items in the fit model and EFT, and the stats window to edit rolls; best done with modules
- [ ] Change Drone Skills | skills affect drone | skill level submenu | skillAffectors.py. Not yet: no characters, every skill is at V

### droneItemMisc
- [x] Copy Selected Drones | selection non-empty (pref additionsCopyPaste) | EFT-style export of selection to clipboard | additionsExportSelection.py
- [x] Copy All Drones | fit has drones | export all drones | additionsExportAll.py
- [x] Paste Drones | clipboard non-empty | parses clipboard, imports drone lines. GuiImportLocalDronesCommand | additionsImport.py

## Fighters pane (gui/builtinAdditionPanes/fighterView.py)

### fighterItem
- [ ] Abilities | row clicked | checkable list of implemented abilities; toggles on selection, Ctrl/Alt on all similar. GuiToggleLocalFighterAbilityStateCommand | fighterAbilities.py
- [ ] Fighter Stats | row clicked | stats window | itemStats.py
- [ ] Fighter Market Group | has market group | market jump | itemMarketJump.py
- [ ] Remove Fighter | row clicked | removes selection, Ctrl/Alt all similar. GuiRemoveLocalFightersCommand | itemRemove.py
- [ ] Change Fighter Quantity | row clicked | dialog, sets squadron size. GuiChangeLocalFighterAmountCommand | itemAmountChange.py
- [ ] Variations | 2+ variations | variation swap, Ctrl/Alt similar. GuiChangeLocalFighterMetasCommand | itemVariationChange.py
- [ ] Change Fighter Skills | skills affect fighter | skill level submenu | skillAffectors.py

### fighterItemMisc
- [ ] Copy Selected Fighters | selection | clipboard export | additionsExportSelection.py
- [ ] Copy All Fighters | fit has fighters | clipboard export | additionsExportAll.py
- [ ] Paste Fighters | clipboard non-empty | import. GuiImportLocalFightersCommand | additionsImport.py

## Cargo pane (gui/builtinAdditionPanes/cargoView.py)

### cargoItem
- [ ] {Category} Stats | row clicked | stats window (needs the item stats window) | itemStats.py
- [x] {Category} Market Group | has market group | market jump | itemMarketJump.py
- [x] Remove {Category} Stack | row clicked | removes selected cargo stacks (multi). GuiRemoveCargosCommand | itemRemove.py
- [x] Change Selection Quantity | row clicked | dialog, sets amount on all selected stacks; 0 removes them. GuiChangeCargosAmountCommand | itemAmountChange.py
- [x] Variations | 2+ variations | variation swap for selected cargo with same variation set, merging into an existing stack. GuiChangeCargoMetasCommand | itemVariationChange.py
- [x] Fill Cargo With {Category} | row clicked | adds as many units as fit in remaining cargo capacity. GuiAddCargoCommand | cargoFill.py

### cargoItemMisc
- [x] Copy Selected Cargo Items | selection | clipboard export | additionsExportSelection.py
- [x] Copy All Cargo Items | cargo non-empty | clipboard export | additionsExportAll.py
- [x] Paste Cargo Items | clipboard non-empty | import (non-abyssal items). GuiImportCargosCommand | additionsImport.py

## Implants pane (gui/builtinAdditionPanes/implantView.py)

Context depends on implant source: fit implants use implantItem/implantItemMisc, character implants use implantItemChar/implantItemMiscChar.

### implantItem
- [ ] Implant Stats | row clicked | stats window | itemStats.py
- [x] Implant Market Group | has market group | market jump | itemMarketJump.py
- [x] Remove Implant | row clicked | removes selection (multi). GuiRemoveImplantsCommand | itemRemove.py
- [x] Variations | 2+ variations | variation submenu (unsorted by meta), swaps clicked implant. GuiChangeImplantMetaCommand | itemVariationChange.py

### implantItemChar
- [ ] Implant Stats | row clicked | stats window | itemStats.py
- [x] Implant Market Group | has market group | market jump | itemMarketJump.py

### implantItemMisc / implantItemMiscChar
- [x] Apply Implant Set | implantItemMisc only, any user or precalculated set exists | submenu: precalculated sets (pirate sets etc., grade submenus Low/Mid/High-grade), separator, "Saved Sets" submenu of user sets; adds all implants of the set (gyfa adds an Implant Set Editor entry at the end) | implantSetApply.py
- [x] Save as New Implant Set | fit has applied implants | name dialog, opens implant set editor with them | implantSetSave.py
- [x] Copy Selected Implants | selection | clipboard export | additionsExportSelection.py
- [x] Copy All Implants | implants non-empty | clipboard export | additionsExportAll.py
- [x] Paste Implants | clipboard non-empty | import. GuiImportImplantsCommand | additionsImport.py

## Boosters pane (gui/builtinAdditionPanes/boosterView.py)

### boosterItem
- [x] Side Effects | booster has an implemented side effect | checkable list of side effects; toggles one. GuiToggleBoosterSideEffectStateCommand | boosterSideEffects.py
- [ ] Booster Stats | row clicked | stats window | itemStats.py
- [x] Booster Market Group | has market group | market jump | itemMarketJump.py
- [x] Remove Booster | row clicked | removes selection (multi). GuiRemoveBoostersCommand | itemRemove.py
- [x] Variations | 2+ variations | boosters of the same slot in the same market group (or none), sorted by side effect chance (or name if > 7); swaps the clicked booster only, keeping state and side effects. GuiChangeBoosterMetaCommand | itemVariationChange.py

### boosterItemMisc
- [x] Copy Selected Boosters | selection | clipboard export | additionsExportSelection.py
- [x] Copy All Boosters | boosters non-empty | clipboard export | additionsExportAll.py
- [x] Paste Boosters | clipboard non-empty | import. GuiImportBoostersCommand | additionsImport.py

## Projected pane (gui/builtinAdditionPanes/projectedView.py)

Contexts: one of projectedModule (+ projectedCharge if loaded) / projectedDrone / projectedFighter / projectedFit, then always projected. Right-click on the state column opens no menu. itemContext is "Projected Item".

### projectedModule (incl. environment effects, which are projected modules)
- [ ] Charge | module has loadable charges | same submenu as fittingModule. GuiChangeProjectedModuleChargesCommand | moduleAmmoChange.py
- [ ] Spoolup Cycles | Precursor Weapon / Mutadaptive RR | as fittingModule. GuiChangeProjectedModuleSpoolCommand | moduleSpool.py
- [ ] Projected Item Stats | row clicked | stats window | itemStats.py
- [ ] Projected Item Market Group | has market group | market jump | itemMarketJump.py
- [ ] Remove Projected Item | row clicked | removes selection, Ctrl/Alt all similar. GuiRemoveProjectedItemsCommand | itemRemove.py
- [ ] Change Projected Item Range | not an exclusive system effect | dialog "New Range, km" (empty = no range) for selection, Ctrl/Alt similar. GuiChangeProjectedItemsProjectionRangeCommand | itemProjectionRange.py
- [ ] Variations | 2+ variations | variation swap (no fit check). GuiChangeProjectedModuleMetasCommand | itemVariationChange.py

### projectedCharge
- [ ] {Charge} Stats | charge loaded | stats window | itemStats.py
- [ ] {Charge} Market Group | has market group | market jump | itemMarketJump.py

### projectedDrone
- [ ] Projected Item Stats | row clicked | stats window | itemStats.py
- [ ] Projected Item Market Group | has market group | market jump | itemMarketJump.py
- [ ] Remove Projected Item Stack | row clicked | removes selection. GuiRemoveProjectedItemsCommand | itemRemove.py
- [ ] Change Projected Item Quantity | row clicked | dialog, amount. GuiChangeProjectedDroneAmountCommand | itemAmountChange.py
- [ ] Change Projected Item Range | row clicked | range dialog. GuiChangeProjectedItemsProjectionRangeCommand | itemProjectionRange.py
- [ ] Variations | 2+ variations | variation swap. GuiChangeProjectedDroneMetasCommand | itemVariationChange.py

### projectedFighter
- [ ] Abilities | row clicked | ability toggles. GuiToggleProjectedFighterAbilityStateCommand | fighterAbilities.py
- [ ] Projected Item Stats | row clicked | stats window | itemStats.py
- [ ] Projected Item Market Group | has market group | market jump | itemMarketJump.py
- [ ] Remove Projected Item | row clicked | removes selection, Ctrl/Alt similar. GuiRemoveProjectedItemsCommand | itemRemove.py
- [ ] Change Projected Item Quantity | row clicked | dialog, squadron size. GuiChangeProjectedFighterAmountCommand | itemAmountChange.py
- [ ] Change Projected Item Range | row clicked | range dialog. GuiChangeProjectedItemsProjectionRangeCommand | itemProjectionRange.py
- [ ] Variations | 2+ variations | variation swap, Ctrl/Alt similar. GuiChangeProjectedFighterMetasCommand | itemVariationChange.py

### projectedFit
- [ ] Open Fit in New Tab | projected fit is not the active fit | opens it in a new tab | fitOpenNewTab.py
- [ ] Remove Projected Item Stack | row clicked | removes selected projected fits. GuiRemoveProjectedItemsCommand | itemRemove.py
- [ ] Change Projected Item Quantity | row clicked | dialog (0-20), number of copies projected. GuiChangeProjectedFitAmountCommand | itemAmountChange.py
- [ ] Change Projected Item Range | row clicked | range dialog. GuiChangeProjectedItemsProjectionRangeCommand | itemProjectionRange.py

### projected (pane background, always last)
- [ ] Add Fit... | always | fit browser dialog (excludes already added), projects chosen fits | fitAddBrowse.py
- [ ] Add Currently Open Fit | active fit exists | submenu "Ship: Fit" for each open tab; projects it | fitAddCurrentlyOpen.py
- [ ] Add Environmental Effect | always | submenu: Metaliminal Storm (Electrical/Exotic/Gamma/Plasma, by strength), Wormhole (Black Hole, Cataclysmic Variable, Magnetar, Pulsar, Red Giant, Wolf Rayet, by class), Abyssal Weather (5 weathers by tier, plus Localized: Bioluminescence/Tachyon/Filament clouds), Sansha Incursion, Drifter Incursion, Triglavian Invasion, FW/Insurgency, IHub Upgrades (Electric/Plasma/Exotic/Gamma). GuiAddProjectedModuleCommand | envEffectAdd.py

## Command pane (gui/builtinAdditionPanes/commandView.py)

Contexts: commandFit (row is a fit, not a generic link), separator, commandView (always).

### commandFit
- [ ] Open Fit in New Tab | not the active fit | opens it in a new tab | fitOpenNewTab.py
- [ ] Remove Command Fit | row clicked | removes selected command fits. GuiRemoveCommandFitsCommand | itemRemove.py

### commandView
- [ ] Add Fit... | always | fit browser dialog, adds as command fits | fitAddBrowse.py
- [ ] Add Currently Open Fit | active fit exists | submenu of open tabs | fitAddCurrentlyOpen.py
- [ ] Command Fits | active fit and any saved fit carries a command burst | submenu of those fits ("(Ship) Fit"; grouped per ship if 15+). GuiAddCommandFitsCommand | commandFitAdd.py
- [ ] Generic Links | active fit | submenu: "Max Links", separator, "All Links" (strength x mindlink grid), separator, Shield/Armor/Skirmish/Information/Expedition/Mining category submenus, each link type with leaves "5%/lvl + Mindlink".."0%/lvl + Mindlink", separator, "5%/lvl".."0%/lvl". Injects warfare buffs without a booster fit. GuiAddCommandLinkCommand | commandLinkAdd.py, eos/saveddata/commandLink.py

## Market browser item list (gui/builtinMarketBrowser/itemView.py)

Context marketItemGroup (browsing a group) or marketItemMisc (search / recent).
- [ ] {Category} Stats | always | stats window | itemStats.py
- [x] {Category} Market Group | marketItemMisc only | jumps to the item's group | itemMarketJump.py
- [x] Fill With Module | active fit, category Module (pref moduleFill) | adds the module to every free slot. GuiFillWithNewLocalModulesCommand | itemFill.py
- [x] Add {Drone} to Drone Bay (xN) | active fit, category Drone, room for >= 1 | adds stack sized to bay/bandwidth limit. GuiAddLocalDroneCommand | droneAddStack.py
- [x] Add {Category} to Cargo | active fit (structures: charges only) | adds 1 to cargo. GuiAddCargoCommand | cargoAdd.py
- [x] Fill Cargo With {Category} | active fit, item is charge or commodity (gyfa: any cargo-only item) | adds max units that fit. GuiAddCargoCommand | cargoFill.py
- [x] Add {Charge} to Cargo (x1000) / (x8) for Scan Probes | active fit, category Charge | adds 1000 (or 8). GuiAddCargoCommand | cargoAddAmmo.py
- [ ] Project {Category} onto Fit | active non-structure fit, item has a projected effect (pref project) | adds as projected module/drone/fighter. GuiAddProjectedModule/Drone/FighterCommand | itemProject.py
- [ ] Set {Category} as Damage Pattern | active fit, item has em/therm/kin/exp damage attrs (pref ammoPattern) | sets fit damage pattern to the item's damage ratio | ammoToDmgPattern.py

## Fitting browser (gui/builtinShipBrowser)

### baseShip (ship row, shipItem.py)
- [ ] Ship Basic Stats | always | stats window for the ship type | itemStats.py

### fit row (fitItem.py, hand-built wx.Menu, only when a fit is active)
- [x] Open in new tab | active fit exists | opens the fit in a new tab | gui/builtinShipBrowser/fitItem.py
- [ ] Project onto Active Fit | active fit exists | projects this fit onto the active fit | gui/builtinShipBrowser/fitItem.py
- [ ] Add Command Booster | active fit exists | adds this fit as command fit | gui/builtinShipBrowser/fitItem.py

## Character selection, missing-skills icon (gui/characterSelection.py, hand-built)
- [ ] Grant Missing Skills | fit needs skills | raises character skills to fit requirements | gui/characterSelection.py
- [ ] Copy Missing Skills | same | clipboard, one skill per line | gui/characterSelection.py
- [ ] Copy Missing Skills (condensed) | same | clipboard, condensed | gui/characterSelection.py
- [ ] Copy Missing Skills (EVEMon) | same | clipboard, EVEMon plan XML | gui/characterSelection.py

## Character editor (gui/characterEditor.py)

### skillItem (skill tree leaf)
- [ ] Skill Stats | always | stats window | itemStats.py
- [ ] Unlearn, Level 0 .. Level 5 | character is not "All 0"/"All 5" (appended after separator) | sets skill level | gui/characterEditor.py

### implantEditor (character implants list)
- [ ] Apply Implant Set | any set exists | set submenu as above, adds to character | implantSetApply.py

## Item stats window (gui/builtinItemStatsViews)

### itemStats (Affected By tree node)
- [ ] {Type} Stats | node is an item/container | opens stats window for that afflictor | itemStats.py, itemAffectedBy.py
### description / traits text (hand-built)
- [ ] Copy | always | copies text | itemDescription.py, itemTraits.py

## Stats pane panels (gui/statsPane.py, menu icon on panel header)

### priceViewFull / priceViewMinimal
- [x] Drones | always | checkbox: include drones in price total | priceOptions.py
- [x] Cargo | always | checkbox: include cargo | priceOptions.py
- [x] Implants && Boosters | always | checkbox: include implants/boosters | priceOptions.py

### resistancesViewFull
- [~] {damage pattern names} | always (disabled with no fit) | top-level checkable entries for root patterns (Uniform, Selected Ammo, ...) plus category submenus; sets fit damage pattern (EHP) | damagePatternChange.py; builtin patterns only, no user patterns yet

### firepowerViewFull
- [x] Factor in Reload Time | always (disabled with no fit) | checkbox, global setting, reload included in DPS | factorReload.py
- [ ] Target Resists | active fit | checkable "No Profile", separator, target profile tree; sets fit target profile for applied DPS | targetProfile/switcher.py; left out until refine can apply damage to a profile instead of a projectee item

## Graphs (graphs/gui)

### graphFitList (fits list) then graphFitListMisc
- [ ] Add Fit... | always | fit browser, adds fits to graph | fitAddBrowse.py
- [ ] Add Currently Open Fit | always | submenu of open tabs | fitAddCurrentlyOpen.py
- [ ] Plot with Different Ammo... | fit row, damage graph | dialog: per weapon group pick charges, drones, fighters to plot | graphFitAmmoPicker.py
- [ ] Remove Fit | row clicked | removes selected from graph | itemRemove.py
- [ ] Open Fit in New Tab | fit row, not active fit (graphFitListMisc) | open tab | fitOpenNewTab.py

### graphTgtList (targets list) then graphTgtListMisc
- [ ] Add Fit... | always | fit browser, adds targets | fitAddBrowse.py
- [ ] Add Currently Open Fit | always | open tabs submenu | fitAddCurrentlyOpen.py
- [ ] Add Target Profile | always | submenu: Ideal profile, builtin + user profile tree | targetProfile/adder.py
- [ ] Resist Mode | fit row and ignoreResists off | radio Auto/Shield/Armor/Hull/Weighted Average for selected target fits | resistMode.py
- [ ] Edit Target Profile | user (non-builtin) profile row | opens target profile editor | targetProfile/editor.py
- [ ] Remove Target | row clicked | removes selected | itemRemove.py
- [ ] Open Fit in New Tab | fit row, not active fit (graphTgtListMisc) | open tab | fitOpenNewTab.py

### graph control panel menu icon (ctrlPanel.py, context = graph internalName)
- [ ] Apply Projected Items | dmgStatsGraph | checkbox | graphDmgApplyProjected.py
- [ ] Ignore Target Resists | dmgStatsGraph | checkbox | graphDmgIgnoreResists.py
- [ ] Ignore Target Resists | ammoOptimalDpsGraph | checkbox | graphAmmoOptimalIgnoreResists.py
- [ ] Apply Projected Effects | ammoOptimalDpsGraph | checkbox | graphAmmoOptimalApplyProjected.py
- [ ] Ignore Lock Range | dmgStatsGraph, remoteRepsGraph, ewarStatsGraph | checkbox | graphLockRange.py
- [ ] Ignore Drone Control Range | dmgStatsGraph, remoteRepsGraph, ewarStatsGraph | checkbox | graphDroneControlRange.py
- [ ] Drone Mode | dmgStatsGraph | radio Auto / Stick to Target / Stick to Attacker | graphDmgDroneMode.py

## gyfa-only today
- Module row State submenu (offline/online/active/overheated): pyfa has no menu for this, it uses clicks on the state column.
- Fittings browser Delete: pyfa does this with a button on the fit row, not a menu.
- Rig/subsystem Remove: same entry as pyfa's fittingModule "Remove Module".
- No multi-select anywhere in gyfa; pyfa's multi/Ctrl-similar variants are missing even where marked [x].

# Engine support

refine API paths are under refine-core/src/api. "ui only" = no engine involvement.

## fittingModule
- Charge: yes, Module::set_charge_type_id; charge removal via Charge::remove.
- Spoolup Cycles: yes, Module::set_spool_override(Spool::Cycles), Module::get_spool_cycle_count for the range.
- RAH Damage Pattern: partial, Fit::set_rah_incoming_dps is per fit not per module; "Do Not Adapt" maybe via ItemMut::set_effect_mode on the RAH effect.
- Module Stats: yes, ItemMut::iter_attrs / get_attr / iter_modifiers (affected by), ItemMut::stats.
- Market Group: ui only.
- Remove: yes, Module::remove.
- Variations: yes, Module::set_type_id (keeps state/charge).
- Apply Mutaplasmid / Revert: yes, Module::mutate, Mutation::change_attr (roll), Mutation::remove.
- Fill With Module: yes, Fit::add_module repeated (validate with Fit::validate_fast / try_items).
- Copy Module to Clipboard: ui only (reads Mutation attrs).
- Change Skills: yes, Skill::set_level / Fit::add_skill, affecting skills found with ItemMut::iter_modifiers.

## fittingCharge / fittingMode
- Charge Stats: yes, Charge item attrs (ItemMut::iter_attrs).
- Charge Market Group: ui only.
- Change Charge Skills: yes, Skill::set_level.
- Tactical Mode Stats: yes, Stance item attrs.

## fittingShip
- Tactical Mode: yes, Fit::set_stance.
- Ship Stats: yes, Ship attrs + FitStats.
- Citadel System Security: yes, Sol::set_sec_zone (SecZone is per sol, not per fit).
- Pilot Security Status: yes, Fit::set_sec_status (FitSecStatus -10..5).
- Open in Fitting Browser: ui only.
- Change Ship Skills: yes, Skill::set_level.

## droneItem
- Stats: yes, drone attrs.
- Market Group: ui only.
- Remove Stack: yes, Drone::remove per drone.
- Change Quantity: yes, but refine drones are single items: add/remove N via Fit::add_drone / Drone::remove (stack is a gyfa concept).
- Split Stack: ui only on top of the above (stacks are gyfa grouping; per-drone state via Drone::set_state).
- Variations: yes, Drone::set_type_id.
- Mutaplasmid: yes, Drone::mutate, Mutation::remove.
- Change Skills: yes, Skill::set_level.
- Copy Selected/All, Paste: ui only (paste then Fit::add_drone).

## fighterItem
- Abilities: yes, Fighter::iter_abilities, Ability::set_state.
- Stats: yes.
- Market Group: ui only.
- Remove: yes, Fighter::remove.
- Change Quantity: yes, Fighter::set_count_override.
- Variations: yes, Fighter::set_type_id.
- Change Skills: yes, Skill::set_level.
- Copy/Paste: ui only (Fit::add_fighter).

## cargoItem
- Stats: ui only (static type attrs; cargo not modeled in refine).
- Market Group: ui only.
- Remove Stack, Change Quantity, Variations, Fill Cargo: no engine (cargo not an engine item); capacity from ship attr via Ship get_attr.
- Copy/Paste: ui only.

## implantItem / implantItemChar / implantItemMisc
- Stats: yes, implant attrs.
- Market Group: ui only.
- Remove: yes, Implant::remove.
- Variations: yes, Implant::set_type_id.
- Apply Implant Set: yes, Fit::add_implant per implant; set storage ui only.
- Save as New Implant Set: ui only.
- Copy/Paste: ui only (Fit::add_implant).

## boosterItem
- Side Effects: yes, Booster::iter_side_effects, SideEffect::set_state (get_chance/get_strength for labels).
- Stats: yes.
- Market Group: ui only.
- Remove: yes, Booster::remove.
- Variations: yes, Booster::set_type_id.
- Copy/Paste: ui only (Fit::add_booster).

## projectedModule / projectedCharge / projectedDrone / projectedFighter
- Charge: yes, Module::set_charge_type_id on the projector module.
- Spoolup: yes, Module::set_spool_override.
- Stats: yes.
- Market Group: ui only.
- Remove: yes, RangedProj::remove or remove the projector item.
- Change Range: yes, ranged projections take range from positions: set_coordinates on projector/projectee (RangedProj::get_range to read).
- Quantity (drone/fighter): drones add/remove items; fighters Fighter::set_count_override.
- Abilities (fighter): yes, Ability::set_state.
- Variations: yes, set_type_id on projector item.
- Note: refine projections need a projector item on some fit; pyfa's free-floating projected modules/drones/fighters need a hidden source fit (Sol::add_fit + Module::add_proj / Drone::add_proj / Fighter::add_proj).

## projectedFit
- Open Fit in New Tab: ui only.
- Remove: yes, remove projections of that fit's items (RangedProj::remove).
- Change Quantity: partial, no multiplier; needs N fit copies.
- Change Range: yes, via coordinates.

## projected (background)
- Add Fit... / Add Currently Open Fit: yes, add the fit to the same Sol and add_proj its projectable items onto the active ship.
- Add Environmental Effect: yes, Sol::add_sw_effect (system-wide beacons/weather), Fit::add_fw_effect (FW/insurgency, IHub), Sol::add_proj_effect + ProjEffect::add_proj for targeted ones.

## commandFit / commandView
- Open Fit in New Tab: ui only.
- Remove Command Fit: yes, Fit::set_fleet(None) / Fleet::remove_fit (note: refine fleets are symmetric, pyfa boosts one way).
- Add Fit... / Add Currently Open Fit / Command Fits: yes, Sol::add_fleet, Fit::set_fleet / Fleet::add_fit.
- Generic Links: no direct API; emulate with a synthetic fleet fit (command ship + bursts + charges + skills), cannot hit exact strength %/lvl.

## Market browser
- Stats: ui only (static type data) or a scratch item for computed values.
- Market Group: ui only.
- Fill With Module: yes, Fit::add_module repeated.
- Add to Drone Bay (xN): yes, Fit::add_drone N times; limit from FitStats::get_stat_drone_bay_volume / get_stat_drone_bandwidth.
- Add to Cargo / Fill Cargo / Add x1000: no engine (cargo).
- Project onto Fit: yes, hidden source fit + add_proj (see projected).
- Set as Damage Pattern: yes, DpsProfile passed to FitStats::get_stat_ehp(incoming_dps) / Sol::set_default_incoming_dps.

## Fitting browser
- Ship Basic Stats: ui only (static attrs).
- Open in new tab: ui only.
- Project onto Active Fit: yes, as projected fit.
- Add Command Booster: yes, as command fit (fleet).

## Character selection / editor
- Grant Missing Skills: yes, Skill::set_level (requirements from Fit::validate_verbose skill reqs).
- Copy Missing Skills (3 variants): ui only.
- Skill Stats: ui only.
- Unlearn / Level 0..5: yes, Skill::set_level / Skill::remove.
- Apply Implant Set (character): yes, Fit::add_implant.

## Item stats window
- {Type} Stats from Affected By: yes, ItemMut::iter_modifiers for the tree.
- Copy description/traits: ui only.

## Stats pane
- Price toggles (Drones, Cargo, Implants && Boosters): ui only.
- Damage pattern (resistances panel): yes, FitStats::get_stat_ehp(Some(DpsProfile)).
- Factor in Reload Time: yes, StatTimeOptions::Sim with optional_reloads (Sol::set_default_optional_reloads, Module::set_optional_reload_override); burst DPS ignores reload.
- Target Resists: partial, get_stat_dmg_applied needs a projectee item; no abstract resist/sig profile, would need a target ship item or own resist math on StatDmg.

## Graphs
- Add Fit... / Add Currently Open Fit / Remove / Open in New Tab: ui only.
- Plot with Different Ammo...: yes, Module::set_charge_type_id per sample (or scratch fit copies).
- Add Target Profile / Edit Target Profile: partial, same projectee limitation as Target Resists.
- Resist Mode: no engine (graph math choosing which layer's resists to apply).
- Apply Projected Items / Apply Projected Effects: yes, include or omit projections when computing.
- Ignore Target Resists: yes, use get_stat_dmg instead of get_stat_dmg_applied.
- Ignore Lock Range / Ignore Drone Control Range: ui only (graph clipping; values from get_stat_lock_range / get_stat_drone_control_range).
- Drone Mode: partial, Drone::set_movement / set_coordinates control drone position; mode logic is graph-side.

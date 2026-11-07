# Feature parity with pyfa

Rough coverage: fitting, stats, additions, item stats, graphs, editors, context menus, the file formats and ESI/SSO are mostly there.
Biggest gaps: minimal stats views.
Most of the missing calculation is already in refine (fleets, projections, system effects, stances, fighters, applied dps, cap sim, RAH, spool, validation, attribute modification trees), so the bulk of the remaining work is UI and persistence.
Not in refine: prices, ESI/SSO, file formats, skill plans, graph sampling over distance/time, abstract target profiles for applied damage, lock/warp time formulas (all done graph-side in src/graphs).

Legend: `[x]` done, `[~]` partial, `[ ]` missing. "refine:" names the engine API that likely covers the calculation.

## Fit management and tabs

- [x] Multiple fit tabs (gui/chrome_tabs.py, gui/mainFrame.py)
- [x] Fits persisted between runs (eos/db): fits.json in the XDG data dir, written through a temp file
- [ ] Safe loading and backups of saved data (eos/db/migration.py backs the DB up before migrating): a fits.json that fails to parse loads as empty and the next save overwrites it; no backup copy is kept
- [x] Delete confirmation (builtinShipBrowser/fitItem.py "Do you really want to delete this fit?", skipped with Shift), from the fittings browser and Fit > Delete Fit; Enter says Yes as in pyfa, Escape cancels
- [x] Middle-click a tab to close it (chrome_tabs.py OnMiddleUp)
- [x] New fit (mainMenuBar.py New Tab)
- [x] Delete fit (shipBrowser fitItem.py)
- [x] Close tab without deleting the fit, close all tabs (mainMenuBar.py), Ctrl+W and Ctrl+Alt+W
- [x] Reopen previous fits on startup as a preference (pyfaGeneralPreferences.py)
- [x] Rename fit (fitCommands/gui/fitRename.py): double-click the tab, the tab menu, or Rename in the fittings browser
- [x] Copy/duplicate fit (builtinShipBrowser/fitItem.py): the main menu and Copy in the fittings browser, named "<fit> copy" as in pyfa
- [~] Reorder tabs by drag, tab tooltips (chrome_tabs.py), tooltips with the fit and hull behind a preference
- [x] Open fit in new tab vs same tab (builtinContextMenus/fitOpenNewTab.py): a click in the fittings browser follows the "open in a new page" preference, "Open in new tab" and a middle click always open a tab
- [x] Undo/redo for every fit change (gui/fitCommands/, mainMenuBar.py): one history per fit, every action and market add, Ctrl+Z/Ctrl+Y
- [x] Disable fitting restrictions toggle (fitCommands/gui/fitRestrictionToggle.py), per fit in the Fit menu: with restrictions on, modules the ship cannot take are refused (ship group, rig size, max group or type fitted, hardpoints, capital size, subsystem slot); a full rack refuses either way; turning them back on takes off what does not fit, as one undo step
- [x] Fit validation messages (over CPU/PG/slots/skills/max group fitted/rig size etc.) (eos/saveddata/fit.py), refine: validate_verbose; per item in the row tooltips, overuse of CPU, powergrid and the like in the stats pane
- [x] System security zone per fit (builtinContextMenus/fitSystemSecurity.py), structures only as in pyfa, refine: set_sec_zone
- [x] Pilot security status per fit (builtinContextMenus/fitPilotSecurity.py), refine: set_sec_status
- [x] Per-fit character selection, stored on the fit; old fits fly All 5 and new fits get the character picked last (gui/characterSelection.py)
- [x] Per-fit damage pattern and target profile (gui/builtinStatsViews, contextMenus/damagePatternChange.py, targetProfile/switcher.py)
- [x] Fit notes, carried by duplicates, shown in the fittings browser tooltip cut to 200 characters (builtinAdditionPanes/notesView.py, builtinShipBrowser/fitItem.py); EFT text has no notes in pyfa either
- [x] Mark fit as fleet booster / command fit (builtinAdditionPanes/commandView.py), from the fittings browser's "Add Command Booster", drags, and the Command tab's menus
- [x] Backup all fits to XML (mainMenuBar.py backupFits)
- [x] Fit browser lite dialog for picking fits (gui/fitBrowserLite.py): a searchable list of saved fits for the Projected and Command tabs' "Add Fit..."

## Market browser

- [x] Market group tree (builtinMarketBrowser/marketTree.py), over the results pane with a draggable divider, as in pyfa's splitter; the split is saved. Clicking a group lists its items, or all its subgroups' items when it has none of its own, and the tree stays put
- [x] Search box (builtinMarketBrowser/searchBox.py, pfSearchBox.py): searches the whole market into the results pane; clearing it goes back to the picked group
- [x] Double-click to add to fit (builtinMarketBrowser/itemView.py)
- [x] Item icons (itemView.py), from the client CDN, cached on disk
- [x] Meta group filter buttons (builtinMarketBrowser/metaButton.py), different from pyfa on purpose: six buttons (T1, T2, T3, Faction, Deadspace, Officer) where pyfa has four (Normal, Faction, Complex, Officer); a click toggles one, Ctrl+click shows only that one, none pressed shows everything. They sit in the results pane's filter bar
- [x] Recently used items (itemView.py, service/market.py): pyfa's "Recently Used Items" tree entry, listed in the results pane, the last 20 items added from the market (double-click, menus, drags), saved with the fits
- [x] "Charges For Active Fit" tree entry (itemView.py), listed in the results pane: every charge the fit's modules take, in market order
- [~] Search delay and meta button behaviour prefs (pyfaMarketPreferences.py), search delay done; pyfa's four meta button modes (search and recents, group selection, empty view, market jump) are missing
- [~] Market shortcuts: Alt+1..9 adds the Nth listed item (mainFrame.py ItemSelect); the "Show market shortcuts" preference that numbers them is missing (pyfaMarketPreferences.py)
- [x] Jump to market group from an item (builtinContextMenus/itemMarketJump.py): from anything the results pane lists outside the item's own group, and the fit's, additions' and projected items' menus; the jump picks the group, presses the item's meta button and clears the pane's filters so it shows
- [x] Drag item from market onto fit/slot/additions (itemView.py, fittingView.py)
- [x] Item tooltip with name, Omega, price and the start of the description (gyfa extra: pyfa's item list has none); the description loads with the stats window's data on first hover
- [x] Omega-only marker (builtinViewColumns/omega.py) behind pyfa's "Show Omega icons" preference, off by default; pyfa reads an attribute the current data no longer sets, gyfa marks items needing a skill level beyond the Alpha clone caps from clonegrades.json
- [x] Items listed in pyfa's order (Market.itemSort): category, market group, variation parent, meta button, meta level, name, so variations sit next to their T1 item
- [x] Jargon/abbreviation search, e.g. "lse", "hml" (service/jargon/): pyfa's default jargon file, a user_jargon.yaml next to the saved fits on top, `*` and `?` wildcards, `re:` regex queries, and pyfa's minimum of three characters after expansion
- [x] Results pane filter bar (gyfa extra): a filter box narrowing what is listed (name substring, or a market search with jargon), the meta buttons, a "Fits current ship" toggle and a count; an empty state with "Clear filters" when the filters hide everything. The ship filter checks the catalog only: rack slots and hardpoints after subsystems, rig size, canFitShipGroup/Type limits, structure vs ship modules, charges some fitted module loads, a drone or fighter bay; CPU, powergrid and max-group limits are not checked
- [x] Compatible charges for a module (gyfa extra): "Show Compatible Charges" on fitted and projected modules lists every charge the module loads in the results pane, under headings the way the Charge submenu groups them (turret families, missile damage types)

## Ship browser

- [x] Fittings browser with pyfa's stages (gui/shipBrowser.py): ship groups, a group's ships, a ship's fits; ships are still in the market tree too. Citadels, engineering complexes and refineries are listed with the ships, as in pyfa
- [x] Ship group navigation with back (builtinShipBrowser/navigationPanel.py, categoryItem.py): Ship groups, Back, Recent Fits, hide empty groups, New fit on a ship's fits; pyfa has no forward either
- [x] Race filter (builtinShipBrowser/raceSelector.py): toggles for the races in the group, under its ships; nothing picked shows all, as in pyfa. Race comes from the faction, else the skills' races, else the ship's own, like pyfa's Item.race
- [x] Saved fits listed under each ship with "N fits" on ships and groups (builtinShipBrowser/shipItem.py, fitItem.py); a ship without fits starts one named "<ship> fit"
- [x] Recent fits view (navigationPanel.py): the 50 fits changed last, by the change time gyfa now keeps per fit, shown on fit rows like pyfa's timestamp (EVE time)
- [x] Search ships and saved fits by name (navigationPanel.py)
- [x] Hide empty ship groups (navigationPanel.py), for groups and ships
- [x] Ship and fit tooltips with traits (shipItem.py, fitItem.py) behind pyfa's "Show ship browser tooltip" preference: fits show ship, notes, then traits
- [x] Jump from fit to ship in browser (builtinContextMenus/shipJump.py), hidden while the browser already shows that ship's fits
- [x] Copy, rename and delete from the fit row's menu (pyfa's row buttons); rename opens the fit and edits its tab name, delete does not ask

## Fitting view and columns
- [x] Columns auto-size to content, resize by dragging the header edge, double-click resets (gui/display.py); widths are kept across restarts, pyfa forgets them

- [x] High/mid/low/rig/subsystem racks with empty slot rows (builtinViews/fittingView.py)
- [x] Module state cycling offline/online/active/overload (builtinViewColumns/state.py), refine: set_state
- [x] Remove module (double-click) (fittingView.py)
- [x] Ship name header with icon and fit name editing (fittingView.py)
- [x] Module table with attribute columns (builtinViewColumns/attributeDisplay.py)
- [x] Rack labels and rack separators prefs (pyfaGeneralPreferences.py): "Separate Racks" and "Show Rack Labels", labels as section bands, separators as a gap and border
- [x] Color rows by slot type (pyfaGeneralPreferences.py)
- [x] Highlight modules with unmet skills or invalid fitting in red (fittingView.py), with the reasons on hover; drones, fighters, implants, boosters and the mode too. pyfa colors the row background and leaves skills out; gyfa uses red text for both
- [x] Drag and drop modules between slots, swap/rearrange (fittingView.py, fitCommands/gui/localModule)
- [x] Ctrl drag to copy a module or rig onto an empty slot of its rack, Ctrl+Alt drag to fill the rack with copies (fittingView.py swapItems), drag module to cargo (fitCommands/gui/localModuleCargo), module/charge to and from cargo with Ctrl to copy
- [x] Multi-select and bulk state change (fittingView.py): Shift/Ctrl selection, a state click takes the selection along, Alt every similar module, Ctrl switches them off
- [x] Delete key removes selection, Ctrl+A selects all, Escape clears (fittingView.py)
- [x] Column: ship/item icon and charge icon (baseIcon.py, ammoIcon.py)
- [x] Column: CPU, PG per module (attributeDisplay.py), refine: item get_attr
- [x] Column: capacitor usage per module (capacitorUse.py), refine: item get_attr
- [x] Column: optimal + falloff range (maxRange.py), refine: item get_attr
- [x] Column: misc info per module type (tracking, neut, web, ewar, damage mods, reps, mining, ancillaries, RAH resists, command bursts, charges, subsystem slots) with pyfa's tooltips (misc.py), refine: item attrs and switched item stats; doomsdays show raw volley without pyfa's "over Ns"
- [x] Column: heat, estimated time until an overheated module burns out (heat.py), pyfa's simulation on refine's attributes
- [x] Column: price (price.py)
- [x] Column: charge name with pyfa's "Name (charges, N cycles)" (ammo.py), refine: get_charge_count, get_charged_cycle_count
- [x] Column: drone EHP and drone regen (droneEhp.py, droneRegen.py), refine: item stats get_stat_ehp, get_stat_erps
- [x] Column: fighter abilities (abilities.py), refine: iter_abilities
- [x] Column: booster side effects (sideEffects.py), refine: iter_side_effects
- [x] Column: projection range (projectionRange.py), refine places the projector that far from the target's surface
- [x] Column: target resists, damp scan res (targetResists.py, dampScanRes.py): pyfa shows these in the graphs' target list only, where gyfa has them too
- [x] Strategic cruiser subsystem slot changes reflected live (fittingView.py), refine: get_stat_*_slots handles it
- [x] T3 destroyer stance/mode switching (builtinContextMenus/shipModeChange.py), refine: set_stance
- [x] Ship row context menu (fittingView.py)

## Context menu actions

- [x] Change module state (gyfa right-click State)
- [x] Load/unload charge on module (builtinContextMenus/moduleAmmoChange.py), refine: set_charge_type_id
- [x] Remove item (builtinContextMenus/itemRemove.py)
- [x] Charge picker grouped by damage type/range (moduleAmmoChange.py): turret families Long to Short Range, missiles by damage type, crystals by ore; pyfa shows no ammo stats in it either
- [x] Change charge in all modules of the same type (pyfaGeneralPreferences.py), Ctrl/Alt does the other
- [x] Item variations: swap to T2/faction/deadspace variant (builtinContextMenus/itemVariationChange.py)
- [x] Fill all slots with module (builtinContextMenus/moduleFill.py)
- [x] Fill rack / item fill (builtinContextMenus/itemFill.py)
- [x] Change item amount (builtinContextMenus/itemAmountChange.py): drones, fighters, cargo, projected fits, drones and fighters
- [x] Show item stats (builtinContextMenus/itemStats.py): everywhere pyfa has it, including fighter rows, projected rows and their charges, and skills in the character editor. Projected modules, drones, fighters and environment effects show their type's base attributes (pyfa shows them as the fit's skills modify them); a projected fit opens its ship with that fit's bonuses, which pyfa does not offer
- [x] Jump to market group (itemMarketJump.py)
- [x] Project item onto fit (builtinContextMenus/itemProject.py), refine: add_proj from a hidden capsule fit per projected item
- [x] Projection range (builtinContextMenus/itemProjectionRange.py), refine: projector coordinates
- [x] Mutate item / mutaplasmid apply and revert, modules and drones (builtinContextMenus/itemMutations.py), refine: mutate, FullMAttr::set_value; rolls are edited in the stats window
- [x] Export mutated module (builtinContextMenus/moduleMutatedExport.py)
- [x] Spool-up percentage for spooling modules (builtinContextMenus/moduleSpool.py), refine: set_spool_override, set_default_spool
- [x] Reactive armor hardener pattern (builtinContextMenus/moduleRahPattern.py): Fit Pattern, Do Not Adapt or any damage pattern, kept with the module; refine: set_rah_incoming_dps (one per fit, and a fit holds one hardener)
- [x] Factor in reload time toggle (builtinContextMenus/factorReload.py), refine: StatTimeOptions reload
- [x] Resist mode (builtinContextMenus/resistMode.py): in pyfa this is the graphs' target resist mode, done there
- [x] Set damage pattern from a market charge (builtinContextMenus/ammoToDmgPattern.py)
- [x] Damage pattern switcher with user patterns (builtinContextMenus/damagePatternChange.py)
- [x] Target profile switcher/adder/editor (builtinContextMenus/targetProfile/): switcher, editor, and the graph window's adder and "Edit Target Profile"
- [x] Add charge/ammo to cargo, fill cargo (builtinContextMenus/cargoAdd.py, cargoAddAmmo.py, cargoFill.py)
- [x] Split/merge drone stacks (builtinContextMenus/droneSplitStack.py, droneAddStack.py), merge by dragging a stack onto another
- [x] Fighter ability toggles (builtinContextMenus/fighterAbilities.py), refine: get_ability / set state, with pyfa's defaults (the attack, or everything but propulsion without one)
- [x] Booster side effects toggles (builtinContextMenus/boosterSideEffects.py), refine: get_side_effect, set_state
- [x] Apply/save implant set (builtinContextMenus/implantSetApply.py, implantSetSave.py)
- [x] Add fit from browser / currently open fits as projected or command (builtinContextMenus/fitAddBrowse.py, fitAddCurrentlyOpen.py, commandFitAdd.py)
- [ ] Add command link bursts directly (builtinContextMenus/commandLinkAdd.py); refine cannot add warfare buffs without a burst on a fit, so pyfa's numbers are out of reach
- [x] Add environment effect (wormhole, incursion, abyssal weather, etc.) (builtinContextMenus/envEffectAdd.py), refine: add_fw_effect, add_proj_effect for weather and clouds
- [x] Show skills affecting item, change skill level inline (builtinContextMenus/skillAffectors.py), refine: iter_modifiers, set_level; modules, charges, ship, drones and fighters
- [x] Price total options (builtinContextMenus/priceOptions.py): Drones, Cargo, Implants && Boosters
- [x] Copy/paste/export/import additions lists (builtinContextMenus/additionsExportAll.py, additionsExportSelection.py, additionsImport.py)
- [x] Context menu enable/disable preferences (pyfaContextMenuPreferences.py): every toggle is honoured, and holding Ctrl while opening a menu brings the hidden entries back as in pyfa (gui/contextMenu.py)

## Additions pane

- [x] Drones pane: pyfa's columns (state, name, max range, misc, speed, EHP, regen, price) from per-drone engine stats, sorted by drone class, state click launches/recalls the selection, double-click removes one drone (Alt: stack), Delete/Escape/Ctrl+A, market drops (Alt: full stack), drag a stack onto another to merge, Ctrl to copy (builtinAdditionPanes/droneView.py), refine: add_drone, item stats
- [x] Implants and boosters list (builtinAdditionPanes/implantView.py, boosterView.py), refine: add_implant, add_booster
- [x] Cargo: parsed from EFT and exported, added from the market (double-click with Ctrl/Alt batches, drag, menus), edited, dragged to and from the fit (builtinAdditionPanes/cargoView.py)
- [x] Tabbed additions pane (gui/additionsPane.py): Drones, Fighters, Cargo, Implants, Boosters, Projected, Command, Notes
- [x] Ctrl+1..9 switches additions tab (mainFrame.py AdditionsTabSelect), counted in gyfa's tab order
- [x] Tab names with counts (pyfaGeneralPreferences.py "Extra info in Additions panel tab names"): none, active or all items
- [x] Drone stack amount editing, separate stacks (droneView.py)
- [x] Fighters tab: squadrons with size, state, speed, EHP, regen, active abilities and price, sorted like pyfa, light/heavy/support tube counts, Delete/Escape/Ctrl+A, market drops, copy/paste, EFT in and out (builtinAdditionPanes/fighterView.py), refine: add_fighter, set_count_override, abilities, get_stat_launched_*_fighters
- [x] Cargo tab with amounts, volume and price, sorted like pyfa, Delete/Ctrl+A/Escape keys (cargoView.py)
- [x] Commodities, materials, deployables and containers in the catalog for cargo; market tree shows pyfa's Deployable Structures and Filaments roots, search adds ice products (service/market.py)
- [x] Implants: use character implants vs fit implants switch (implantView.py); the All 5 character has no implants until characters exist
- [x] Implant slot conflict handling: one implant per slot, the new one replaces the old, listed in slot order (implantView.py, eos HandledImplantList)
- [x] Implants pane columns: state checkbox (multi-select toggle), slot, icon, name, price; drag from the market onto the pane, Delete/Escape/Ctrl+A (implantView.py)
- [x] Booster slot conflict and side effects (boosterView.py): one booster per slot, listed by slot, on/off state, side effect, price and duration columns, drop from the market, Delete/Escape/Ctrl+A, refine: set_state, iter_side_effects
- [x] Projected tab: fits (N copies each), modules with charges and spool, drones, fighters, environment effects, pyfa's columns and order, drops from the market, the fitting view and the fittings browser, Delete/Escape/Ctrl+A (builtinAdditionPanes/projectedView.py), refine: add_proj, add_fw_effect, add_proj_effect
- [x] Projected item states and amounts (fitCommands/gui/projectedChangeStates.py), left click toggles, right click overheats modules, Alt takes similar items, double-click takes one copy or drone (Alt: all)
- [x] Command tab: fleet booster fits (builtinAdditionPanes/commandView.py), refine: add_fleet, set_fleet. Fits are referenced by a stable id that survives deletes and moves
- [x] Notes tab: edits kept with the fit right away and written out shortly after, character count in the tab title, Ctrl+Backspace and undo from the text field (builtinAdditionPanes/notesView.py)

## Stats panes

- [x] Resources: hardpoints, launched drones or fighter squadrons, calibration, CPU and PG gauges, drone bay or fighter bay (cargo bay while the cargo tab is open), bandwidth, with pyfa's tooltips (builtinStatsViews/resourcesViewFull.py), refine: get_stat_launched_fighters, get_stat_fighter_bay_volume
- [x] Resistances: gauges per layer, EHP or raw HP per layer and in the header (EHP/HP button), resist multiplier tooltips, incoming damage pattern row (builtinStatsViews/resistancesViewFull.py), refine: get_stat_ehp(Some(DpsProfile)), set_rah_incoming_dps
- [x] Damage pattern per fit: Uniform, Selected Ammo and pyfa's builtin and the user's patterns from the header menu (damagePatternChange.py)
- [x] Recharge rates: passive shield, reinforced and sustained shield/armor/hull in EHP/s or HP/s, armor spool range tooltip (builtinStatsViews/rechargeViewFull.py), refine: get_stat_rps
- [x] Firepower: weapon and drone DPS (sentries count as drones), volley, total, spool and damage type tooltips, damage type bar, swap to mining yield (builtinStatsViews/firepowerViewFull.py), refine: get_stat_dmg
- [x] Factor in Reload Time from the firepower menu, also used by the capacitor simulation (factorReload.py), refine: StatTimeOptions::Sim
- [x] Applied DPS vs target resists (firepowerViewFull.py "Effective" and targetProfile/switcher.py): like pyfa, only the profile's resists are applied, per damage type on refine's get_stat_dmg; breacher pods count as pure damage, capped by the profile's HP, and show as "Pure" in the tooltips
- [x] Mining yield: miner, drone, total with yield, drain and efficiency tooltips (builtinStatsViews/miningyieldViewFull.py), refine: get_stat_mps
- [x] Remote reps: capacitor, shield, armor, hull with spool tooltips, plus neutralization when the fit neuts (builtinStatsViews/outgoingViewFull.py), refine: get_stat_outgoing_rps, get_stat_outgoing_cps, get_stat_outgoing_nps
- [x] Capacitor: capacity, stable or lasts, delta with recharge/use tooltip, neutralizer resistance with effective capacity (builtinStatsViews/capacitorViewFull.py), refine: get_stat_cap_sim, get_stat_cap_balance, get_stat_neut_resist
- [x] Targeting and misc: targets, range, scan res with lock times, sensor strength with type and jam chance, drone range, speed, align with mass and agility, signature with probe size, warp speed with max warp distance and core strength, cargo with special holds (builtinStatsViews/targetingMiscViewMinimal.py), refine: get_stat_* sensors, mobility, drone control range, incoming jam
- [x] Bombing: bombs to kill per damage type and Covert Ops level, collapsed by default as pyfa hides it (builtinStatsViews/bombingViewFull.py); wormhole bomb damage effects not applied
- [x] Price: ship, fittings, character, drones, cargo, total from ESI prices, with the Drones/Cargo/Implants && Boosters total toggles (builtinStatsViews/priceViewFull.py, priceOptions.py)
- [x] Collapsible stat sections with header menus, also on right-click (gui/toggle_panel.py, statsPane.py)
- [~] Per-view full/minimal/hidden preference (pyfaStatViewPreferences.py), shown or hidden; gyfa has no minimal views (each section would need a second, compact layout)
- [ ] Capability flags: can warp/jump/dock/tether (no pyfa view), refine: get_stat_can_* (gyfa-only extra)
- [~] Neut resist, breach resist, d-scan range (no pyfa view), refine: get_stat_neut_resist, get_stat_breach_resist, get_stat_dscan_range; neut resist is in the capacitor section, the other two are not shown

## Characters and skills

- [x] User characters with per-skill levels, calculated by the engine (service/character.py), kept in characters.json
- [x] All 0 / All 5 built-in characters, read-only (service/character.py)
- [x] Character editor in its own window: skill tree by group with search, levels from the menu or keys 0-5, unlearn, pending levels marked (gui/characterEditor.py); setting a level trains its prerequisites and unlearns skills that need it higher (strict skill levels)
- [x] Create, rename, copy, delete, save, save as, revert characters (characterEditor.py, mainMenuBar.py)
- [x] Alpha clone skill caps (characterEditor.py clone choice, eos/saveddata/character.py alphaClone): Omega or one of the four Alpha clones per character, from sde/clonegrades.json. The trained levels stay; the pilot flies capped ones, skills the clone lacks at 0, and requirements check the capped levels. The editor shows a capped skill as "Level 3 (of 5)"
- [x] Character implants tab, with Apply Implant Set (characterEditor.py)
- [x] Character security status, used where the fit does not set its own (characterEditor.py), refine: set_sec_status
- [x] Import skills from ESI (characterEditor.py "Get Skills" and the EVE SSO tab linking a character), see ESI and SSO
- [x] Import/export skills to clipboard, levels in digits or Roman numerals (characterEditor.py)
- [x] Import character file (EVEMon XML) (mainMenuBar.py importCharacter)
- [x] Export skills needed for fit as EVEMon .emp/.xml or text, by file extension (mainMenuBar.py exportSkillsNeeded)
- [x] Missing skills icon with tooltip and menu (gui/characterSelection.py); the condensed tooltip, pyfa's default
- [x] Global character vs per-fit character pref (pyfaGeneralPreferences.py "Use global character"): the current fit takes the character picked last, as pyfa's switchFit does
- [x] Compact vs full skills-needed tooltip pref (pyfaGeneralPreferences.py "Compact skills needed tooltip")
- [x] Skill Stats in the character editor's skill menu (itemStats.py skillItem), on the built-ins too
- [x] Enforce strict skill requirements pref (pyfaEnginePreferences.py): lowering a skill unlearns the skills that need it higher, in the character editor and the Change Skills menus
- [x] Implant set editor: new, rename, copy, delete, add from search, remove, clipboard import/export (gui/setEditor.py, builtinViews/implantEditor.py, service/implantSet.py)
- [x] Precalculated implant sets (service/precalcImplantSet.py, db_update.py processImplantSets)

## Damage and target profiles

- [x] Damage pattern editor with built-in NPC presets (gui/patternEditor.py, service/damagePattern.py), refine: set_default_incoming_dps
- [x] Damage pattern import/export (patternEditor.py)
- [x] Target profile editor: resists, sig, speed, radius, HP (gui/targetProfileEditor.py, service/targetProfile.py)
- [x] Built-in target profiles (service/targetProfile.py)
- [x] Global vs per-fit damage pattern pref (pyfaGeneralPreferences.py "Use global damage pattern"): the current fit takes the pattern picked last, Uniform until one is
- [~] Target profile from a fit (graphs and applied DPS), refine: get_stat_dmg_applied against projectee; fits are graph targets, applied DPS in the stats pane is missing

## Import and export

- [x] EFT import via paste dialog (service/port/eft.py)
- [x] EFT import from file on CLI (service/port/port.py importFitFromFiles)
- [x] EFT export to clipboard (service/port/eft.py exportEft)
- [x] EFT import of cargo, charges and mutated items' blocks (eft.py), pyfa's /offline and several fits per paste
- [x] EFT export options: implants, boosters, cargo, mutations, loaded charges (eft.py, gui/copySelectDialog.py), in pyfa's layout with empty slots up to the ship's count
- [~] Import from clipboard with format autodetect (port.py importAuto), item lists go to the additions tabs (fighters to the Fighters tab) or fit a single module; ESI dynamic item links are not fetched
- [x] Renamed item conversions on import (service/conversions/, applied in service/market.py getItem): pyfa's tables in src/port/renames.tsv (scripts/renames.py rebuilds it), applied in every name lookup with chains followed
- [x] Import from file dialog, multiple files (port.py importFitsThreaded)
- [x] EFT .cfg legacy format (port.py importEftCfg); notes come from Description= lines, with | as line breaks
- [x] DNA import/export (service/port/dna.py), chat links and the DNA: variant too; fighters as `typeid;size`
- [~] XML import/export (EVE client format) (service/port/xml.py); notes are the description, exported cut to 400 characters with line breaks as <br>; fighters as "fighter bay" hardware with the squadron size
- [~] ESI JSON import/export (service/port/esi.py); notes are the description, exported cut to 400 characters; fighters with flag 158 and the squadron size
- [~] Multibuy export (service/port/multibuy.py), no Optimize Prices option
- [x] EFS stats JSON export (service/port/efs.py), the copy dialog's "EFS" format: pyfa's fields, weapons, repairs and projections, and the weapon bonus multipliers measured with Tech II test weapons against a capsule
- [x] Ship stats export (service/port/shipstats.py), the copy dialog's "Stats" format
- [x] Mutated item (muta) format (service/port/muta.py): pasting one mutated item fits it into the current fit, drone copy/paste carries mutations; ESI dynamic item links are not fetched
- [x] HTML export of all fits (pyfaHTMLExportPreferences.py), full and minimal
- [x] Export item stats to CSV (gui/itemStats.py)
- [x] Export implants/boosters/charges lists (builtinContextMenus/additionsExportAll.py)

## Item stats window

- [x] Item stats window with tabs, one OS window per item, cascaded, Shift replaces the last one used, Escape closes, values follow fit edits live instead of a Refresh button (gui/itemStats.py)
- [x] Description (builtinItemStatsViews/itemDescription.py)
- [x] Traits (builtinItemStatsViews/itemTraits.py)
- [x] Attributes with base vs modified values, raw toggle, grouping (builtinItemStatsViews/itemAttributes.py, attributeGrouping.py), refine: iter_attrs, get_attr
- [x] Requirements tree (builtinItemStatsViews/itemRequirements.py)
- [x] Dependents: what a skill unlocks (builtinItemStatsViews/itemDependants.py)
- [~] Effects list (builtinItemStatsViews/itemEffects.py), refine: iter_effects, set_effect_mode. Active shows whether the effect runs on the fitted item; pyfa's debug-only toggle by double-click is left out
- [x] Affected by tree with item and attribute views, real names toggle, expand all (builtinItemStatsViews/itemAffectedBy.py), refine: iter_modifiers (Modification, Affector)
- [x] Properties: the type's raw facts, shown always where pyfa needs debug mode (builtinItemStatsViews/itemProperties.py)
- [x] Compare variations side by side, sortable, raw toggle, highlight on double-click (builtinItemStatsViews/itemCompare.py)
- [x] Mutations with roll sliders (builtinItemStatsViews/itemMutator.py, attributeSlider.py): worst to best, a field for exact values, Reset defaults, Random stats, Revert changes; letting go of a slider is one undo step in the main window, where pyfa commits on close
- [x] Price in item stats: in the Compare tab, and in the window header (gui/itemStats.py)

## Graphs

The graph window (Fit > Graphs, Ctrl+G) samples numbers read once per fit from refine (`Engine::graph_snapshot`: per-weapon damage and cycle timing, weapon/ewar/repair attributes, ship stats) with pyfa's application formulas (src/graphs). Turret and missile application are checked against refine's `get_stat_dmg_applied`, and damage over time against refine's simulation.

- [x] Graph window and fit/target lists (graphs/gui): one window, fits and targets added from menus, removed with Delete, double-click or the menu, pyfa's extra columns per graph
- [x] Damage stats: DPS/volley/damage inflicted vs distance, time, target speed (m/s, %), target sig (m, %), attacker and target vectors (graphs/data/fitDamageStats); turrets, launchers, vortons, smartbombs, bombs, guided bombs, doomsdays, drones, fighter abilities (attack, missiles, bombs, kamikaze, each read from refine on its own) and breacher pods (the smaller of their cap and their share of the target's HP, times its breacher resistance). Fighter rearming is left out of the time axis
- [x] Application profile, best ammo per distance (graphs/data/fitApplicationProfile): every charge of the chosen meta tier loaded into each weapon group in refine, the best per distance summed, segments colored, dashed or plain by charge. Charge colors are spread evenly rather than pyfa's per-ammo table
- [x] Capacitor amount/regen vs time and cap amount, pyfa's cap simulator with stagger, injectors and reloads (graphs/data/fitCapacitor)
- [x] Shield amount/regen vs time and shield amount, HP or EHP (graphs/data/fitShieldRegen), following the main window's stats pane raw/effective toggle
- [x] Mobility: speed, distance, momentum, bump speed and distance vs time (graphs/data/fitMobility)
- [x] Warp time vs distance in AU or km, from the speed with warp-incompatible modules off (graphs/data/fitWarpTime)
- [x] Lock time vs target sig (graphs/data/fitLockTime)
- [x] Remote reps: rate and total vs distance and time, ancillary reload option (graphs/data/fitRemoteReps)
- [x] Ewar stats: neuts, webs, ECM, damps, TDs, GDs, TPs vs distance with target resistance (graphs/data/fitEwarStats)
- [x] ECM burst + scanres damps (graphs/data/fitEcmBurstScanresDamps), listed when the window is opened with Ctrl+Alt+G, as in pyfa
- [x] Graph options: ignore resists, apply projected (own webs, painters and scrams, which turn off the target's microwarpdrive), ignore lock range, ignore drone control range, drone mode, target resist mode, the Application Profile's own two, style and ammo meta, legend and Y = 0 (builtinContextMenus/graph*.py), remembered between runs. Webs and paints stack among themselves rather than with what already affects a target fit
- [x] Plot with different ammo (builtinContextMenus/graphFitAmmoPicker.py): a fit row submenu adding the fit again with another charge in one weapon type; pyfa's own dialog is switched off in its code
- [x] Line color per fit, lightness and line style per target (builtinViewColumns/graphColor.py, graphLineStyle.py, graphLightness.py), from the theme's palette where it has the hue
- [x] Legend, "Always show Y = 0", readout at the hovered or clicked X (right-click clears)
- [x] Built-in target profiles and the ideal target (eos/saveddata/targetProfile.py)
- [ ] Drag fits from the fittings browser into the graph lists: gpui drags stay inside their window, and the graph window is a window of its own

## Preferences

- [x] Preferences dialog (gui/preferenceDialog.py), its own window, changes apply to every window right away
- [~] General (pyfaGeneralPreferences.py): character implants by default, slot colors, racks and labels, reopen fits, tab tooltips, ship browser tooltips, Omega icons, open in new tab, ammo change all, mutated names, tab info, global character, global damage pattern and compact skills tooltip done; missing: language, item name language, theme choice, animate gauges
- [x] Engine: strict skills, factor in reload, global default spool (pyfaEnginePreferences.py), refine: set_default_spool, set_default_optional_reloads
- [~] Stat view layout (pyfaStatViewPreferences.py), each section shown or hidden
- [x] Context menu toggles (pyfaContextMenuPreferences.py)
- [~] Market and prices: source, system, search delay, price total composition (pyfaMarketPreferences.py); missing "Show market shortcuts" and the four meta button modes
- [x] Network/proxy (pyfaNetworkPreferences.py), for price and ESI requests
- [~] ESI (pyfaEsiPreferences.py): Client ID, callback port and the callback URL to register with a copy button, the scopes to register; pyfa's login modes and server choice are not there
- [x] Database paths and resets (pyfaDatabasePreferences.py): shows where fits, characters and the cache live; Delete All Damage Pattern Profiles, Delete All Target Profiles and Delete All Prices, each after pyfa's question
- [ ] Logging (pyfaLoggingPreferences.py)
- [ ] Update checks (pyfaUpdatePreferences.py, service/update.py)
- [x] HTML export path (pyfaHTMLExportPreferences.py)
- [x] Attribute overrides editor (gui/propertyEditor.py, eos/saveddata/override.py, mainMenuBar.py Global > Attribute Overrides and Turn Overrides On/Off): a window to search a type or pick an overridden one, type a base value per attribute, reset one or all of a type, clear all, and import/export pyfa's override CSV; overrides and the on/off switch are saved; while on they replace the type's base value for every item of it, in projected and command fits, graphs and item stats too, and the item stats window marks them. refine has no API for base values, so the engine's adapted data is patched on load and rebuilt in the background when overrides change. The search leaves out skills; pyfa searches everything

## ESI and SSO

- [x] EVE SSO login, manage characters (gui/ssoLogin.py, service/esiAccess.py, service/esi.py): the PKCE flow for native apps with a listener on localhost for the callback, access tokens checked against the SSO's keys (RS256, issuer, audience, expiry), refreshed shortly before they run out. Refresh tokens live in esi.json in the data directory, owner-only. gyfa ships no client ID: the user registers an app and pastes its ID in Preferences > ESI. Character > Manage ESI Characters lists them with Add and Remove, Character > Add ESI Character logs one in directly
- [x] Fetch skills from ESI (characterEditor.py): the editor's EVE SSO tab links a character to a logged-in EVE character, Get Skills replaces its skills and also takes implants (esi-clones.read_implants.v1, which pyfa does not ask for) and security status; logging in from All 0 or All 5 makes a new character named after the EVE one and fills it in
- [x] Browse ESI fittings, import to local (gui/esiFittings.py), Ctrl+B: a window with the character's fittings by ship, search, ship icons and the picked fitting's contents; several can be imported at once with Ctrl+click
- [x] Delete ESI fittings (esiFittings.py), one, several or all, after a confirmation
- [x] Export fit to EVE via ESI (esiFittings.py, mainMenuBar.py), Ctrl+E: charges, implants and boosters into the cargo as options, fighters in the fighter bay. The window keeps the fit as it was when opened
- [ ] pyfa's SSO server mode and the Singularity test server (pyfaEsiPreferences.py)

## Prices

- [x] Price fetching with cache (service/price.py): cached on disk with ETag and expiry
- [ ] Fallback to other price sources when the chosen one fails (service/price.py fetchPrices)
- [~] Default price source: gyfa defaults to the ESI average, pyfa to Fuzzwork Jita
- [~] Market sources: Fuzzwork, EVE Tycoon, evemarketdata, cevemarket (service/marketSources/), ESI average and Fuzzwork at the five hubs or universe; EVE Tycoon, evemarketdata and cevemarket missing
- [x] Price column and price stats view (builtinViewColumns/price.py, builtinStatsViews/priceViewFull.py)
- [ ] Optimize fit price: swap to cheaper equivalent modules (mainMenuBar.py optimizeFitPrice, Ctrl+D)
- [x] Delete all cached prices (pyfaDatabasePreferences.py)

## Misc

- [x] Load EVE data from local dump (gyfa data.rs, pyfa eos/db gamedata)
- [x] Skip unknown items on import with a status message (port.py)
- [~] In-game icons, in progress (gui/bitmap_loader.py)
- [~] Keyboard shortcuts (mainMenuBar.py, mainFrame.py registerMenu): Ctrl+T, W, Alt+W, Z, Y, Shift+Z, C, V, O, S, P, I, K, G, Alt+G, B, E, F (search box of the browser shown), Space (switch market and fittings browser), Tab, Shift+Tab, PgUp/PgDn (next/previous tab), F4 and Alt+F4 (close tab, close all), 1..9 (additions tab), Alt+1..9 (market item) done; missing Ctrl+D (optimize price), Ctrl+B and Ctrl+E (ESI)
- [x] About dialog, version info (gui/aboutData.py)
- [x] Error dialog with details (gui/errorDialog.py): unreadable data files, failed saves and panics, with the details to copy. A panic that ends gyfa is kept in crash.txt and shown on the next start
- [ ] Update notifications (gui/updateDialog.py, service/update.py)
- [ ] Data prefetch/cache warmup (service/prefetch.py)
- [~] Dark/bright themes (pyfaGeneralPreferences.py): gyfa follows the Omarchy theme live, with a plain dark theme otherwise; no theme choice
- [ ] Translations (pyfa locale/)
- [x] Help links: wiki, forums (mainMenuBar.py)
- [ ] Dev tools window (gui/devTools.py)
- [x] Ammo service: flat and structured ammo lists, best ammo per range (service/ammo.py), in the Charge menus and the Application Profile graph
- [x] Structures and services fitting (eos service slots): citadels, engineering complexes and refineries from the market, structure modules and rigs, a service rack with online/offline, EFT in and out, Citadel System Security, drones/implants/boosters/command/notes tabs disabled like pyfa; refine: add_service, get_stat_service_slots, set_sec_zone. pyfa does not model fuel, neither does gyfa
- [x] Faction warfare system effects: pyfa's FW/Insurgency environment effects, in the Projected tab's environment menu, refine: add_fw_effect
- [x] Fitting wizard (gyfa extra, no pyfa counterpart): Ctrl+Shift+W or Fit > Fitting Wizard opens one dialog laid out like the preferences: sections for weapons (sized to the hull, ranked by its mastery and bonuses), propulsion, tackle and utility, tank, and damage modules and rigs, defaults picked from the ship's mastery, bonuses and slots. A global bling tier (T1, compact, T2, faction, deadspace, officer) sets where every module starts, and each module row has its own variant dropdown with tier, meta level and price, e.g. a Quad LiF Restrained MWD; the last choices are saved for the next fit. The tank section picks a style (buffer, active, brick, none), shield or armor, and for active tanks a goal (max resists, max repair, balanced); Find best searches tanks for the free slots and rigs off the UI thread (plates, extenders, multispectrum and damage-type hardeners and membranes, the reactive armor hardener, damage control, repairers, boosters, ancillaries, amplifiers, resist and repair rigs) against the fit's damage pattern within the CPU, powergrid and calibration left, and shows the best five as cards to pick from. The preview shows each rack, CPU, powergrid and calibration, the new modules' price, EHP, resists per layer, sustained repair, capacitor and DPS; "Make it fit" steps modules down to meta and drops the least wanted ones, and Apply fills the free slots (or cleared racks) as one undo step

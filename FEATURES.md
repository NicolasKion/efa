# Feature parity with pyfa

Rough coverage: the core loop works (pick ship, add modules/charges/drones/implants/boosters, see the main stats, EFT in/out), plus the pyfa layout with in-game icons and the main module columns, which is maybe 20-25% of pyfa's surface.
Biggest gaps: characters and skills (everything is all V), target profiles and user damage patterns, projected/command/fleet effects, fighters, cargo editing, item stats window, graphs, prices, ESI/SSO, and most context menus.
Most of the missing calculation is already in refine (fleets, projections, system effects, stances, fighters, mutations, applied dps, cap sim, RAH, spool, validation, attribute modification trees), so the bulk of the remaining work is UI and persistence.
Not in refine: prices, ESI/SSO, file formats, skill plans, graph sampling over distance/time (needs looping over refine stats), lock/warp time formulas.

Legend: `[x]` done, `[~]` partial, `[ ]` missing. "refine:" names the engine API that likely covers the calculation.

## Fit management and tabs

- [x] Multiple fit tabs (gui/chrome_tabs.py, gui/mainFrame.py)
- [x] Fits persisted between runs (eos/db, eframe storage in gyfa)
- [x] New fit (mainMenuBar.py New Tab)
- [x] Delete fit (shipBrowser fitItem.py)
- [x] Close tab without deleting the fit, close all tabs (mainMenuBar.py), Ctrl+W and Ctrl+Alt+W
- [x] Reopen previous fits on startup as a preference (pyfaGeneralPreferences.py)
- [ ] Rename fit (fitCommands/gui/fitRename.py)
- [ ] Copy/duplicate fit (builtinShipBrowser/fitItem.py)
- [~] Reorder tabs by drag, tab tooltips (chrome_tabs.py), tooltips with the fit and hull behind a preference
- [ ] Open fit in new tab vs same tab (builtinContextMenus/fitOpenNewTab.py)
- [x] Undo/redo for every fit change (gui/fitCommands/, mainMenuBar.py): one history per fit, every action and market add, Ctrl+Z/Ctrl+Y
- [ ] Disable fitting restrictions toggle (fitCommands/gui/fitRestrictionToggle.py), refine: validate_fast/validate_verbose with ValOptions
- [ ] Fit validation messages (over CPU/PG/slots/skills/max group fitted/rig size etc.) (eos/saveddata/fit.py), refine: validate_verbose
- [ ] System security zone per fit (builtinContextMenus/fitSystemSecurity.py), refine: set_sec_zone
- [ ] Pilot security status per fit (builtinContextMenus/fitPilotSecurity.py), refine: set_sec_status
- [ ] Per-fit character selection (gui/characterSelection.py)
- [~] Per-fit damage pattern and target profile (gui/builtinStatsViews, contextMenus/damagePatternChange.py), damage pattern done, no target profile
- [x] Fit notes, carried by duplicates, shown in the fittings browser tooltip cut to 200 characters (builtinAdditionPanes/notesView.py, builtinShipBrowser/fitItem.py); EFT text has no notes in pyfa either
- [ ] Mark fit as fleet booster / command fit (builtinAdditionPanes/commandView.py)
- [x] Backup all fits to XML (mainMenuBar.py backupFits)
- [ ] Fit browser lite dialog for picking fits (gui/fitBrowserLite.py)

## Market browser

- [x] Market group tree (builtinMarketBrowser/marketTree.py)
- [x] Search box (builtinMarketBrowser/searchBox.py, pfSearchBox.py)
- [x] Double-click to add to fit (builtinMarketBrowser/itemView.py)
- [x] Item icons (itemView.py), from the client CDN, cached on disk
- [ ] Meta group filter buttons: T1, T2, faction, storyline, deadspace, officer, structure (builtinMarketBrowser/metaButton.py)
- [ ] Recently used items list (itemView.py, service/market.py)
- [ ] "Charges for active fit" filter (itemView.py)
- [~] Search delay and meta button behaviour prefs (pyfaMarketPreferences.py), search delay done
- [ ] Jump to market group from an item (builtinContextMenus/itemMarketJump.py)
- [ ] Drag item from market onto fit/slot/additions (itemView.py, fittingView.py)
- [ ] Item tooltip with description/price (itemView.py)
- [ ] Omega-only marker (builtinViewColumns/omega.py)
- [ ] Hide items that do not fit the current ship (market filtering in service/market.py)
- [ ] Jargon/abbreviation search, e.g. "lse", "hml" (service/jargon/)

## Ship browser

- [~] Ships reachable via market tree, no dedicated ship browser (gui/shipBrowser.py)
- [ ] Ship category/group navigation with back/forward (builtinShipBrowser/navigationPanel.py, categoryItem.py)
- [ ] Race filter (builtinShipBrowser/raceSelector.py)
- [ ] Saved fits listed under each ship, fit counts per group (builtinShipBrowser/shipItem.py, fitItem.py)
- [ ] Recent fits view (navigationPanel.py)
- [ ] Search saved fits by name (navigationPanel.py)
- [ ] Hide empty ship groups (navigationPanel.py)
- [ ] Ship tooltip with traits (shipItem.py)
- [ ] Jump from fit to ship in browser (builtinContextMenus/shipJump.py)

## Fitting view and columns
- [x] Columns auto-size to content, resize by dragging the header edge, double-click resets (gui/display.py); widths are kept across restarts, pyfa forgets them

- [x] High/mid/low/rig/subsystem racks with empty slot rows (builtinViews/fittingView.py)
- [x] Module state cycling offline/online/active/overload (builtinViewColumns/state.py), refine: set_state
- [x] Remove module (double-click) (fittingView.py)
- [x] Ship name header with icon and fit name editing (fittingView.py)
- [x] Module table with attribute columns (builtinViewColumns/attributeDisplay.py)
- [ ] Rack labels and rack separators prefs (pyfaGeneralPreferences.py)
- [x] Color rows by slot type (pyfaGeneralPreferences.py)
- [ ] Highlight modules with unmet skills or invalid fitting in red (fittingView.py), refine: validate_verbose skill_reqs
- [ ] Drag and drop modules between slots, swap/rearrange (fittingView.py, fitCommands/gui/localModule)
- [~] Shift/ctrl drag to copy module, drag module to cargo (fitCommands/gui/localModuleCargo), module/charge to and from cargo done with Ctrl to copy, copying between slots missing
- [ ] Multi-select and bulk state change (fittingView.py)
- [ ] Delete key removes selection (fittingView.py)
- [x] Column: ship/item icon and charge icon (baseIcon.py, ammoIcon.py)
- [x] Column: CPU, PG per module (attributeDisplay.py), refine: item get_attr
- [x] Column: capacitor usage per module (capacitorUse.py), refine: item get_attr
- [x] Column: optimal + falloff range (maxRange.py), refine: item get_attr
- [ ] Column: misc info per module type (dps, rep amount, web strength, etc.) (misc.py), refine: item stats get_stat_dmg, get_stat_rps
- [ ] Column: heat damage / overload info (heat.py)
- [ ] Column: price (price.py)
- [~] Column: charge name and remaining charges/reload (ammo.py), name and icon only, refine: get_charge_count, get_charged_cycle_count
- [x] Column: drone EHP and drone regen (droneEhp.py, droneRegen.py), refine: item stats get_stat_ehp, get_stat_erps
- [ ] Column: fighter abilities (abilities.py), refine: iter_abilities
- [x] Column: booster side effects (sideEffects.py), refine: iter_side_effects
- [ ] Column: projection range (projectionRange.py), refine: proj get_range
- [ ] Column: target resists, damp scan res (targetResists.py, dampScanRes.py)
- [ ] Strategic cruiser subsystem slot changes reflected live (fittingView.py), refine: get_stat_*_slots handles it
- [ ] T3 destroyer stance/mode switching (builtinContextMenus/shipModeChange.py), refine: set_stance
- [ ] Ship row context menu (fittingView.py)

## Context menu actions

- [x] Change module state (gyfa right-click State)
- [x] Load/unload charge on module (builtinContextMenus/moduleAmmoChange.py), refine: set_charge_type_id
- [x] Remove item (builtinContextMenus/itemRemove.py)
- [ ] Charge picker grouped by damage type/range with ammo stats (moduleAmmoChange.py)
- [x] Change charge in all modules of the same type (pyfaGeneralPreferences.py), Ctrl/Alt does the other
- [ ] Item variations: swap to T2/faction/deadspace variant (builtinContextMenus/itemVariationChange.py)
- [ ] Fill all slots with module (builtinContextMenus/moduleFill.py)
- [ ] Fill rack / item fill (builtinContextMenus/itemFill.py)
- [~] Change item amount (drones, cargo, charges) (builtinContextMenus/itemAmountChange.py), cargo done
- [ ] Show item stats (builtinContextMenus/itemStats.py)
- [ ] Jump to market group (itemMarketJump.py)
- [ ] Project item onto fit (builtinContextMenus/itemProject.py), refine: add_proj
- [ ] Projection range (builtinContextMenus/itemProjectionRange.py), refine: proj set_range
- [ ] Mutate item / mutaplasmid apply and edit (builtinContextMenus/itemMutations.py), refine: set_mutator_type_id, mutate, set_roll
- [ ] Export mutated module (builtinContextMenus/moduleMutatedExport.py)
- [ ] Spool-up percentage for spooling modules (builtinContextMenus/moduleSpool.py), refine: set_spool_override, set_default_spool
- [ ] Reactive armor hardener pattern (builtinContextMenus/moduleRahPattern.py), refine: set_rah_incoming_dps
- [x] Factor in reload time toggle (builtinContextMenus/factorReload.py), refine: StatTimeOptions reload
- [ ] Resist mode for EHP display (builtinContextMenus/resistMode.py)
- [ ] Set damage pattern from loaded ammo (builtinContextMenus/ammoToDmgPattern.py)
- [ ] Damage pattern switcher (builtinContextMenus/damagePatternChange.py)
- [ ] Target profile switcher/adder/editor (builtinContextMenus/targetProfile/)
- [x] Add charge/ammo to cargo, fill cargo (builtinContextMenus/cargoAdd.py, cargoAddAmmo.py, cargoFill.py)
- [x] Split/merge drone stacks (builtinContextMenus/droneSplitStack.py, droneAddStack.py), merge by dragging a stack onto another
- [ ] Fighter ability toggles (builtinContextMenus/fighterAbilities.py), refine: get_ability / set state
- [x] Booster side effects toggles (builtinContextMenus/boosterSideEffects.py), refine: get_side_effect, set_state
- [x] Apply/save implant set (builtinContextMenus/implantSetApply.py, implantSetSave.py)
- [ ] Add fit from browser / currently open fits as projected or command (builtinContextMenus/fitAddBrowse.py, fitAddCurrentlyOpen.py, commandFitAdd.py)
- [ ] Add command link bursts directly (builtinContextMenus/commandLinkAdd.py)
- [ ] Add environment effect (wormhole, incursion, abyssal weather, etc.) (builtinContextMenus/envEffectAdd.py), refine: add_sw_effect, add_proj_effect
- [ ] Show skills affecting item, change skill level inline (builtinContextMenus/skillAffectors.py), refine: iter_modifiers, set_level
- [~] Price source options (builtinContextMenus/priceOptions.py), source and system in the preferences
- [ ] Copy/paste/export/import additions lists (builtinContextMenus/additionsExportAll.py, additionsExportSelection.py, additionsImport.py)
- [~] Context menu enable/disable preferences (pyfaContextMenuPreferences.py): variations, fill with module, spoolup, additions copy/paste and fill cargo hide; set as damage pattern, change skills and project onto fit are stored for when those entries exist

## Additions pane

- [x] Drones pane: pyfa's columns (state, name, max range, misc, speed, EHP, regen) from per-drone engine stats, sorted by drone class, state click launches/recalls the selection, double-click removes one drone (Alt: stack), Delete/Escape/Ctrl+A, market drops (Alt: full stack), drag a stack onto another to merge, Ctrl to copy (builtinAdditionPanes/droneView.py), refine: add_drone, item stats. No price column yet
- [x] Implants and boosters list (builtinAdditionPanes/implantView.py, boosterView.py), refine: add_implant, add_booster
- [x] Cargo: parsed from EFT and exported, added from the market (double-click with Ctrl/Alt batches, drag, menus), edited, dragged to and from the fit (builtinAdditionPanes/cargoView.py)
- [~] Tabbed additions pane, in progress (gui/additionsPane.py)
- [x] Tab names with counts (pyfaGeneralPreferences.py "Extra info in Additions panel tab names"): none, active or all items
- [x] Drone stack amount editing, separate stacks (droneView.py)
- [ ] Fighters tab: squadrons, light/heavy/support tubes, abilities (builtinAdditionPanes/fighterView.py), refine: add_fighter, get_stat_launched_*_fighters, get_stat_fighter_bay_volume
- [x] Cargo tab with amounts and volume, sorted like pyfa, Delete/Ctrl+A/Escape keys (cargoView.py); price column waits for a price source
- [x] Commodities, materials, deployables and containers in the catalog for cargo; market tree shows pyfa's Deployable Structures and Filaments roots, search adds ice products (service/market.py)
- [x] Implants: use character implants vs fit implants switch (implantView.py); the All 5 character has no implants until characters exist
- [x] Implant slot conflict handling: one implant per slot, the new one replaces the old, listed in slot order (implantView.py, eos HandledImplantList)
- [x] Implants pane columns: state checkbox (multi-select toggle), slot, icon, name; drag from the market onto the pane (implantView.py). No price column until prices exist, no Delete/Ctrl+A/Esc keys yet
- [x] Booster slot conflict and side effects (boosterView.py): one booster per slot, listed by slot, on/off state, side effect and duration columns, drop from the market; no price column or keyboard shortcuts yet, refine: set_state, iter_side_effects
- [ ] Projected tab: fits, modules, drones, fighters, environment effects (builtinAdditionPanes/projectedView.py), refine: add_proj, add_proj_effect, add_sw_effect
- [ ] Projected item states and amounts (fitCommands/gui/projectedChangeStates.py)
- [ ] Command tab: fleet booster fits (builtinAdditionPanes/commandView.py), refine: add_fleet, set_fleet
- [x] Notes tab: edits kept with the fit right away and written out shortly after, character count in the tab title, Ctrl+Backspace and undo from the text field (builtinAdditionPanes/notesView.py)

## Stats panes

- [x] Resources: hardpoints, launched drones or fighter squadrons, calibration, CPU and PG gauges, drone bay or fighter bay (cargo bay while the cargo tab is open), bandwidth, with pyfa's tooltips (builtinStatsViews/resourcesViewFull.py), refine: get_stat_launched_fighters, get_stat_fighter_bay_volume
- [x] Resistances: gauges per layer, EHP or raw HP per layer and in the header (EHP/HP button), resist multiplier tooltips, incoming damage pattern row (builtinStatsViews/resistancesViewFull.py), refine: get_stat_ehp(Some(DpsProfile)), set_rah_incoming_dps
- [x] Damage pattern per fit: Uniform, Selected Ammo and pyfa's builtin patterns from the header menu; user patterns not yet (damagePatternChange.py)
- [x] Recharge rates: passive shield, reinforced and sustained shield/armor/hull in EHP/s or HP/s, armor spool range tooltip (builtinStatsViews/rechargeViewFull.py), refine: get_stat_rps
- [x] Firepower: weapon and drone DPS (sentries count as drones), volley, total, spool and damage type tooltips, damage type bar, swap to mining yield (builtinStatsViews/firepowerViewFull.py), refine: get_stat_dmg
- [x] Factor in Reload Time from the firepower menu, also used by the capacitor simulation (factorReload.py), refine: StatTimeOptions::Sim
- [ ] Applied DPS vs target resists (firepowerViewFull.py "Effective" and targetProfile/switcher.py): refine's get_stat_dmg_applied needs a projectee item, there are no abstract target profiles yet
- [x] Mining yield: miner, drone, total with yield, drain and efficiency tooltips (builtinStatsViews/miningyieldViewFull.py), refine: get_stat_mps
- [x] Remote reps: capacitor, shield, armor, hull with spool tooltips, plus neutralization when the fit neuts (builtinStatsViews/outgoingViewFull.py), refine: get_stat_outgoing_rps, get_stat_outgoing_cps, get_stat_outgoing_nps
- [x] Capacitor: capacity, stable or lasts, delta with recharge/use tooltip, neutralizer resistance with effective capacity (builtinStatsViews/capacitorViewFull.py), refine: get_stat_cap_sim, get_stat_cap_balance, get_stat_neut_resist
- [x] Targeting and misc: targets, range, scan res with lock times, sensor strength with type and jam chance, drone range, speed, align with mass and agility, signature with probe size, warp speed with max warp distance and core strength, cargo with special holds (builtinStatsViews/targetingMiscViewMinimal.py), refine: get_stat_* sensors, mobility, drone control range, incoming jam
- [x] Bombing: bombs to kill per damage type and Covert Ops level, collapsed by default as pyfa hides it (builtinStatsViews/bombingViewFull.py); wormhole bomb damage effects not applied
- [x] Price: ship, fittings, character, drones, cargo, total from ESI prices, with the Drones/Cargo/Implants && Boosters total toggles (builtinStatsViews/priceViewFull.py, priceOptions.py)
- [x] Collapsible stat sections with header menus, also on right-click (gui/toggle_panel.py, statsPane.py)
- [~] Per-view full/minimal/hidden preference (pyfaStatViewPreferences.py), shown or hidden; gyfa has no minimal views
- [ ] Capability flags: can warp/jump/dock/tether (no pyfa view), refine: get_stat_can_* (gyfa-only extra)
- [ ] Neut resist, breach resist, d-scan range (no pyfa view), refine: get_stat_neut_resist, get_stat_breach_resist, get_stat_dscan_range

## Characters and skills

- [~] Character with all skills V (service/character.py), fixed, no choice
- [ ] All 0 / All 5 built-in characters (service/character.py)
- [ ] Character editor: skill tree, set levels, unlearn (gui/characterEditor.py), refine: add_skill, set_level
- [ ] Create, rename, copy, delete, save/revert characters (characterEditor.py, mainMenuBar.py)
- [ ] Character implants tab (characterEditor.py)
- [ ] Character security status (characterEditor.py), refine: set_sec_status
- [ ] Import skills from ESI (characterEditor.py "Get Skills"), needs SSO
- [ ] Import/export skills to clipboard (characterEditor.py)
- [ ] Import character file (EVEMon XML) (mainMenuBar.py importCharacter)
- [ ] Export skills needed for fit (mainMenuBar.py exportSkillsNeeded), refine: validate_verbose skill_reqs
- [ ] Missing skills tooltip on fit (gui/characterSelection.py)
- [ ] Global character vs per-fit character pref (pyfaGeneralPreferences.py)
- [ ] Enforce strict skill requirements pref (pyfaEnginePreferences.py), refine: ValOptions skill_reqs
- [x] Implant set editor: new, rename, copy, delete, add from search, remove, clipboard import/export (gui/setEditor.py, builtinViews/implantEditor.py, service/implantSet.py)
- [x] Precalculated implant sets (service/precalcImplantSet.py, db_update.py processImplantSets)

## Damage and target profiles

- [ ] Damage pattern editor with built-in NPC presets (gui/patternEditor.py, service/damagePattern.py), refine: set_default_incoming_dps
- [ ] Damage pattern import/export (patternEditor.py)
- [ ] Target profile editor: resists, sig, speed, radius (gui/targetProfileEditor.py, service/targetProfile.py)
- [ ] Built-in target profiles (service/targetProfile.py)
- [ ] Global vs per-fit damage pattern pref (pyfaGeneralPreferences.py)
- [ ] Target profile from a fit (graphs and applied DPS), refine: get_stat_dmg_applied against projectee

## Import and export

- [x] EFT import via paste dialog (service/port/eft.py)
- [x] EFT import from file on CLI (service/port/port.py importFitFromFiles)
- [x] EFT export to clipboard (service/port/eft.py exportEft)
- [~] EFT import of cargo and charges (eft.py), pyfa's /offline and several fits per paste; no mutations
- [~] EFT export options: implants, boosters, cargo, mutations, loaded charges (eft.py, gui/copySelectDialog.py), in pyfa's layout with empty slots up to the ship's count; no mutations yet
- [x] Import from clipboard with format autodetect (port.py importAuto), item lists go to the additions tabs or fit a single module
- [x] Import from file dialog, multiple files (port.py importFitsThreaded)
- [x] EFT .cfg legacy format (port.py importEftCfg); notes come from Description= lines, with | as line breaks
- [x] DNA import/export (service/port/dna.py), chat links and the DNA: variant too
- [x] XML import/export (EVE client format) (service/port/xml.py); notes are the description, exported cut to 400 characters with line breaks as <br>
- [x] ESI JSON import/export (service/port/esi.py); notes are the description, exported cut to 400 characters
- [~] Multibuy export (service/port/multibuy.py), no Optimize Prices
- [ ] EFS stats JSON export (service/port/efs.py)
- [x] Ship stats export (service/port/shipstats.py)
- [ ] Mutated item (muta) format (service/port/muta.py)
- [x] HTML export of all fits (pyfaHTMLExportPreferences.py), full and minimal
- [ ] Export item stats to CSV (gui/itemStats.py)
- [ ] Export implants/boosters/charges lists (builtinContextMenus/additionsExportAll.py)

## Item stats window

- [ ] Item stats window with tabs (gui/itemStats.py)
- [ ] Description (builtinItemStatsViews/itemDescription.py)
- [ ] Traits (builtinItemStatsViews/itemTraits.py)
- [ ] Attributes with base vs modified values, raw toggle, grouping (builtinItemStatsViews/itemAttributes.py, attributeGrouping.py), refine: iter_attrs, get_attr
- [ ] Requirements tree (builtinItemStatsViews/itemRequirements.py)
- [ ] Dependents: what a skill unlocks (builtinItemStatsViews/itemDependants.py)
- [ ] Effects list (builtinItemStatsViews/itemEffects.py), refine: iter_effects, set_effect_mode
- [ ] Affected by tree (builtinItemStatsViews/itemAffectedBy.py), refine: iter_modifiers (Modification, Affector)
- [ ] Properties (builtinItemStatsViews/itemProperties.py)
- [ ] Compare variations side by side (builtinItemStatsViews/itemCompare.py)
- [ ] Mutations with roll sliders (builtinItemStatsViews/itemMutator.py, attributeSlider.py), refine: get_mutation, set_roll, mutate_raw
- [ ] Price in item stats (gui/itemStats.py)

## Graphs

- [ ] Graph window and fit/target lists (graphs/gui, graphs/data/base)
- [ ] Damage stats: DPS/volley/damage vs distance, time, target speed/sig (graphs/data/fitDamageStats), refine: get_stat_dmg_applied, needs sampling loop
- [ ] Application profile (graphs/data/fitApplicationProfile)
- [ ] Capacitor over time (graphs/data/fitCapacitor), refine: get_stat_cap_sim
- [ ] Shield regen vs amount (graphs/data/fitShieldRegen)
- [ ] Mobility: speed/distance vs time (graphs/data/fitMobility), refine: get_stat_speed, get_stat_agility
- [ ] Warp time vs distance (graphs/data/fitWarpTime), refine: get_stat_warp_speed
- [ ] Lock time vs target sig (graphs/data/fitLockTime), refine: get_stat_scan_res
- [ ] Remote reps vs distance (graphs/data/fitRemoteReps), refine: get_stat_outgoing_rps_applied
- [ ] Ewar stats: webs, TPs, damps, neuts vs distance (graphs/data/fitEwarStats), refine: get_stat_outgoing_nps_applied
- [ ] ECM/burst/scanres damp effect (graphs/data/fitEcmBurstScanresDamps), refine: get_stat_incoming_jam
- [ ] Graph context options: ignore resists, apply projected, drone mode, ammo picker, lock/drone range markers (builtinContextMenus/graph*.py)
- [ ] Line color, style, lightness per fit (builtinViewColumns/graphColor.py, graphLineStyle.py, graphLightness.py)
- [ ] Optimal ammo graph (builtinContextMenus/graphAmmoOptimal*.py)

## Preferences

- [x] Preferences dialog (gui/preferenceDialog.py), its own window, changes apply to every window right away
- [~] General: theme, language, tooltips, rack layout, omega icons, mutated names, tab info (pyfaGeneralPreferences.py), theme follows Omarchy live; character implants by default, slot colors, reopen fits, tab tooltips, open in new tab, ammo change all and tab info done
- [~] Engine: strict skills, factor in reload, global default spool (pyfaEnginePreferences.py), reload and default spool done, refine: set_default_spool, set_default_optional_reloads
- [~] Stat view layout (pyfaStatViewPreferences.py), each section shown or hidden
- [x] Context menu toggles (pyfaContextMenuPreferences.py)
- [~] Market and prices: source, search delay, meta buttons, price total composition (pyfaMarketPreferences.py), all but the meta buttons and market view modes
- [x] Network/proxy (pyfaNetworkPreferences.py), for price requests
- [ ] ESI/SSO server and auto-login (pyfaEsiPreferences.py)
- [ ] Database paths and resets (pyfaDatabasePreferences.py)
- [ ] Logging (pyfaLoggingPreferences.py)
- [ ] Update checks (pyfaUpdatePreferences.py, service/update.py)
- [x] HTML export path (pyfaHTMLExportPreferences.py)
- [ ] Attribute overrides editor (gui/propertyEditor.py, mainMenuBar.py), refine: set_value on items if exposed

## ESI and SSO

- [ ] EVE SSO login, manage characters (gui/ssoLogin.py, service/esiAccess.py, service/esi.py)
- [ ] Fetch skills from ESI (characterEditor.py)
- [ ] Browse ESI fittings, import to local (gui/esiFittings.py)
- [ ] Delete ESI fittings (esiFittings.py)
- [ ] Export fit to EVE via ESI (esiFittings.py, mainMenuBar.py)

## Prices

- [ ] Price fetching with cache (service/price.py)
- [~] Market sources: Fuzzwork, EVE Tycoon, evemarketdata, cevemarket (service/marketSources/), ESI average and Fuzzwork at the five hubs or universe
- [ ] Price column and price stats view (builtinViewColumns/price.py, builtinStatsViews/priceViewFull.py)
- [ ] Optimize fit price: swap to cheaper equivalent modules (mainMenuBar.py optimizeFitPrice)
- [ ] Delete all cached prices (pyfaDatabasePreferences.py)

## Misc

- [x] Load EVE data from local dump (gyfa data.rs, pyfa eos/db gamedata)
- [x] Skip unknown items on import with a status message (port.py)
- [~] In-game icons, in progress (gui/bitmap_loader.py)
- [~] Keyboard shortcuts (mainMenuBar.py: ctrl+T/W/Z/Y/C/V/O/S/G/K/I/B/E), all but G and K (their windows) and B and E (ESI)
- [x] About dialog, version info (gui/aboutData.py)
- [ ] Error dialog with details (gui/errorDialog.py)
- [ ] Update notifications (gui/updateDialog.py, service/update.py)
- [ ] Data prefetch/cache warmup (service/prefetch.py)
- [ ] Dark/bright themes (pyfaGeneralPreferences.py)
- [ ] Translations (pyfa locale/)
- [x] Help links: wiki, forums (mainMenuBar.py)
- [ ] Dev tools window (gui/devTools.py)
- [ ] Ammo service: best ammo per range (service/ammo.py)
- [ ] Structures and services fitting (eos service slots), refine: add_service, get_stat_service_slots
- [ ] Faction warfare system effects (no pyfa equivalent beyond env effects), refine: add_fw_effect

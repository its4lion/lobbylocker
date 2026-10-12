use crate::{
    catalogue,
    firewall::{self, FirewallOutcome, Request},
    inspection::{self, Inspection, RegionState, Verification},
    latency::{self, PingSample},
    model::{
        resolve_rules, selected_game_ids, selected_targets, FirewallRule, FirewallScope,
        FirewallTarget, Game, Preferences, Snapshot,
    },
    store::Store,
    Result,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

/// Minimum gap between silent startup refreshes of official server feeds.
const DATA_REFRESH_INTERVAL_SECONDS: u64 = 24 * 60 * 60;

fn merge_overwatch_defaults(game: &mut Game, defaults: Game) {
    let mut endpoint_ids: BTreeSet<String> = game
        .regions
        .iter()
        .flat_map(|region| region.endpoints.iter().map(|endpoint| endpoint.id.clone()))
        .collect();
    for region in defaults.regions {
        if let Some(saved) = game.regions.iter_mut().find(|saved| saved.id == region.id) {
            // Older bundled Google templates predate providerScope. Adding the
            // trusted mapping lets explicit refresh update them while preserving
            // custom endpoints.
            if saved.provider_scope.is_none() && region.provider_scope.is_some() {
                saved.provider_scope = region.provider_scope;
            }
            // Explicit refresh also migrates newly bundled trusted ranges into
            // existing regions and updates older bundled entries by their stable
            // IDs. Custom endpoints are never replaced.
            for endpoint in region.endpoints {
                if let Some(saved_endpoint) = saved
                    .endpoints
                    .iter_mut()
                    .find(|saved_endpoint| saved_endpoint.id == endpoint.id)
                {
                    if !saved_endpoint.custom {
                        *saved_endpoint = endpoint;
                    }
                    continue;
                }
                let target_exists = saved.endpoints.iter().any(|saved_endpoint| {
                    saved_endpoint.address == endpoint.address
                        && saved_endpoint.protocol == endpoint.protocol
                        && saved_endpoint.ports == endpoint.ports
                });
                if !target_exists && endpoint_ids.insert(endpoint.id.clone()) {
                    saved.endpoints.push(endpoint);
                }
            }
        } else {
            endpoint_ids.extend(region.endpoints.iter().map(|endpoint| endpoint.id.clone()));
            game.regions.push(region);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PendingKeys {
    Resolved(BTreeSet<inspection::RuleKey>),
    // Invalid imported program scopes still need an edit baseline. Comparing
    // error strings would miss target edits while the same validation error remains.
    Invalid(BTreeSet<(String, String, Option<String>, Option<String>)>),
}

#[derive(Clone)]
pub struct Runtime {
    pub snapshot: Snapshot,
    pub samples: BTreeMap<String, PingSample>,
    pub histories: BTreeMap<String, Vec<f64>>,
    /// Immutable OS readback, independent of the pending catalogue and selections.
    pub applied_targets: Option<Vec<FirewallTarget>>,
    pub applied_count: Option<usize>,
    /// Attribution of observed rules; IDs alone do not prove current target coverage.
    pub applied_game_ids: Vec<String>,
    pub region_states: BTreeMap<String, RegionState>,
    pub inspected_at: Option<u64>,
    pub inspection: Option<Inspection>,
    pub verification: Verification,
    pub verification_error: String,
    /// Comparison baseline only, never active-state evidence. Before the first
    /// successful read this is the loaded selection; afterwards it is native keys.
    pending_baseline: PendingKeys,
    pub dirty: bool,
    pub message: String,
    pub generation: u64,
}

pub struct Engine {
    store: Store,
    state: Mutex<Runtime>,
    operation: Mutex<()>,
}

impl Engine {
    pub fn new(store: Store) -> Result<Arc<Self>> {
        let snapshot = store.snapshot()?;
        let pending_baseline = Self::pending_keys(&snapshot);
        Ok(Arc::new(Self {
            store,
            state: Mutex::new(Runtime {
                snapshot,
                samples: BTreeMap::new(),
                histories: BTreeMap::new(),
                applied_targets: None,
                region_states: BTreeMap::new(),
                inspected_at: None,
                inspection: None,
                verification: Verification::Unverified,
                verification_error: String::new(),
                applied_count: None,
                applied_game_ids: vec![],
                pending_baseline,
                dirty: false,
                message: "Loading firewall status. No rules are changed automatically.".into(),
                generation: 0,
            }),
            operation: Mutex::new(()),
        }))
    }

    pub fn runtime(&self) -> Runtime {
        self.state.lock().unwrap().clone()
    }

    /// Never writes preferences, recovery hints, or firewall rules. The UI reads
    /// on launch and allows authentication if existing privileges are insufficient.
    pub fn verify_firewall(&self, elevated: bool) -> Result<()> {
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        // Inspection is independent of desired-rule validation, missing games,
        // executable paths, and saved switch positions.
        self.accept_readback(firewall::inspect_firewall(elevated))
    }

    fn record_inspection(&self, report: Inspection) -> Result<()> {
        report.validate()?;
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        let relevant = state
            .snapshot
            .preferences
            .library_game_ids
            .iter()
            .chain(&state.snapshot.preferences.last_applied_game_ids)
            .chain(&state.applied_game_ids)
            .cloned()
            .chain(selected_game_ids(
                &state.snapshot.games,
                &state.snapshot.preferences,
            ))
            .collect::<BTreeSet<_>>();
        let targets = inspection::observed_targets(
            &state.snapshot.games,
            &report,
            FirewallScope::native(),
            &relevant,
        );
        state.applied_game_ids = targets
            .iter()
            .filter(|t| !t.game_id.is_empty())
            .map(|t| t.game_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        state.applied_count = Some(report.rules.len());
        state.applied_targets = Some(targets);
        state.pending_baseline = PendingKeys::Resolved(report.keys());
        state.inspected_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_millis() as u64,
        );
        state.inspection = Some(report);
        state.verification_error.clear();
        Self::reconcile(&mut state);
        state.message = match state.verification {
            Verification::Matched => "Saved selections match active LobbyLocker rules.",
            Verification::Empty => "No active LobbyLocker rules found. Saved selections are unchanged.",
            _ => "Active LobbyLocker rules were read from the OS firewall. They differ from saved selections; the active list remains authoritative.",
        }.into();
        Ok(())
    }

    /// Desired edits only affect pending differences. They cannot manufacture or
    /// erase the observed addresses, executable scopes, or active status.
    fn reconcile(state: &mut Runtime) {
        let Some(report) = &state.inspection else {
            state.dirty = Self::pending_keys(&state.snapshot) != state.pending_baseline;
            return;
        };
        let actual = report.keys();
        let desired = resolve_rules(&state.snapshot.games, &state.snapshot.preferences)
            .and_then(|rules| inspection::expected_keys(&rules));
        state.dirty = desired.as_ref().map_or(true, |keys| *keys != actual);
        state.verification = if actual.is_empty() {
            Verification::Empty
        } else if state.dirty {
            Verification::Different
        } else {
            Verification::Matched
        };
        state.region_states =
            inspection::region_states(&state.snapshot.games, report, FirewallScope::native());
    }

    /// Ignore names, probe settings, ordering, duplicates, and library curation.
    /// Missing Windows programs are allowed in this comparison only: generating
    /// rules still requires resolve_rules and can never fall back to global scope.
    fn pending_keys(snapshot: &Snapshot) -> PendingKeys {
        let rules = selected_targets(&snapshot.games, &snapshot.preferences)
            .into_iter()
            .flat_map(|target| {
                target
                    .endpoints
                    .into_iter()
                    .map(move |endpoint| FirewallRule {
                        endpoint,
                        program: target.executable_path.clone(),
                    })
            })
            .collect::<Vec<_>>();
        Self::pending_rules(&rules)
    }

    fn pending_rules(rules: &[FirewallRule]) -> PendingKeys {
        match inspection::expected_keys(rules) {
            Ok(keys) => PendingKeys::Resolved(keys),
            Err(_) => PendingKeys::Invalid(
                rules
                    .iter()
                    .map(|rule| {
                        (
                            rule.endpoint.address.clone(),
                            format!("{:?}", rule.endpoint.protocol),
                            rule.endpoint.ports.clone(),
                            rule.program
                                .as_ref()
                                .map(|path| path.replace('/', "\\").to_lowercase()),
                        )
                    })
                    .collect(),
            ),
        }
    }

    fn invalidate_inspection(&self, error: &str) -> Result<()> {
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        state.applied_count = None;
        state.applied_targets = None;
        state.inspection = None;
        state.region_states.clear();
        state.inspected_at = None;
        state.verification = Verification::Unverified;
        state.verification_error = error.into();
        // Keep attributed IDs accessible as recovery hints, never active proof.
        Self::reconcile(&mut state);
        state.message =
            "Could not read firewall status. Existing blocks have not been changed by this check."
                .into();
        Ok(())
    }

    pub fn preferences(&self, mut preferences: Preferences) -> Result<()> {
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        // Only successful firewall operations change the recovery hint.
        preferences.last_applied_game_ids =
            self.runtime().snapshot.preferences.last_applied_game_ids;
        self.store.save_preferences(&preferences)?;
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        state.snapshot.preferences = preferences;
        state.generation += 1;
        Self::reconcile(&mut state);
        state.message = if state.dirty {
            "Selection saved. Apply changes to update the firewall."
        } else {
            "Selection saved."
        }
        .into();
        Ok(())
    }

    /// Persists a UI language without changing pending firewall selections or measurements.
    pub fn set_language(&self, code: &str) -> Result<()> {
        crate::model::validate_id(code)?;
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        let mut preferences = state.snapshot.preferences.clone();
        preferences.language = code.into();
        self.store.save_preferences(&preferences)?;
        state.snapshot.preferences = preferences;
        Ok(())
    }

    pub fn save_game(&self, game: &Game) -> Result<()> {
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        self.store.save_game(game)?;
        let snapshot = self.store.snapshot()?;
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        state.snapshot = snapshot;
        state.generation += 1;
        Self::reconcile(&mut state);
        state.message = format!(
            "Saved games/{}.json. Existing firewall rules are unchanged until Apply.",
            game.id
        );
        Ok(())
    }

    /// Library discovery settings never change pending firewall selections or probes.
    pub fn set_game_library_paths(&self, paths: Vec<String>) -> Result<()> {
        let excluded = self
            .runtime()
            .snapshot
            .preferences
            .excluded_game_library_paths;
        self.set_game_library_settings(paths, excluded)
    }

    pub fn set_game_library_settings(
        &self,
        paths: Vec<String>,
        excluded: Vec<String>,
    ) -> Result<()> {
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        let mut preferences = state.snapshot.preferences.clone();
        preferences.game_library_paths = paths;
        preferences.excluded_game_library_paths = excluded;
        self.store.save_preferences(&preferences)?;
        state.snapshot.preferences = preferences;
        Ok(())
    }

    /// Curating the sidebar is not a firewall operation. Newly selected detected
    /// games get empty configs; existing files and block selections are preserved.
    pub fn set_library_games(
        &self,
        mut ids: Vec<String>,
        detected: &[crate::installed::InstalledGame],
    ) -> Result<()> {
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        ids.sort();
        ids.dedup();
        let mut preferences = self.runtime().snapshot.preferences;
        preferences.library_game_ids = ids;
        preferences.validate()?;
        let snapshot = self.store.snapshot()?;
        let mut new_games = Vec::new();
        for id in &preferences.library_game_ids {
            if snapshot.games.iter().any(|game| &game.id == id) {
                continue;
            }
            let installed = detected
                .iter()
                .find(|game| &game.id == id && !crate::installed::is_launcher_tool(game))
                .ok_or("Choose a detected or configured game.")?;
            new_games.push(crate::installed::empty_game(id.clone(), &installed.name)?);
        }
        for game in &new_games {
            self.store.save_game(game)?;
        }
        self.store.save_preferences(&preferences)?;
        let snapshot = self.store.snapshot()?;
        self.state.lock().map_err(|e| e.to_string())?.snapshot = snapshot;
        Ok(())
    }

    /// Silent background refresh for the game the user just opened.
    ///
    /// Only games with an official feed are eligible, at most once a day each, and
    /// only when the user actually opens that game's page. Bundled defaults are
    /// never merged here (that stays an explicit refresh), failures are swallowed,
    /// and the attempt is recorded before any request so an unreachable feed cannot
    /// cause a request on every visit.
    pub fn ensure_fresh_data(&self, game_id: &str) -> Result<()> {
        if !catalogue::has_official_feed(game_id) {
            return Ok(());
        }
        if self
            .store
            .data_refresh_checked_at(game_id)
            .is_some_and(|at| {
                crate::now_seconds().saturating_sub(at) < DATA_REFRESH_INTERVAL_SECONDS
            })
        {
            return Ok(());
        }
        self.store.mark_data_refresh_checked(game_id)?;
        // Serialize with every other mutation so a late response cannot overwrite
        // edits, apply, or reset made while the request was in flight.
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        let game = match self
            .runtime()
            .snapshot
            .games
            .into_iter()
            .find(|game| game.id == game_id)
        {
            Some(game) => game,
            None => return Ok(()),
        };
        let Ok(updated) = catalogue::refresh(&game) else {
            return Ok(());
        };
        if updated == game || self.store.save_game(&updated).is_err() {
            return Ok(());
        }
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        state.snapshot = self.store.snapshot()?;
        state.generation += 1;
        Self::reconcile(&mut state);
        Ok(())
    }

    pub fn refresh_game(&self, game_id: &str) -> Result<()> {
        // Serialize the full refresh so custom edits cannot be overwritten by a late response.
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        let mut game = self
            .runtime()
            .snapshot
            .games
            .into_iter()
            .find(|g| g.id == game_id)
            .ok_or("Game not found.")?;
        if game.id == "overwatch2" {
            // Upgrade bundled targets only on an explicit refresh, never on startup.
            // Existing regions, custom endpoints, and selections remain untouched.
            let defaults = self.store.default_game(game_id)?;
            merge_overwatch_defaults(&mut game, defaults);
        }
        let updated = catalogue::refresh(&game)?;
        self.store.save_game(&updated)?;
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        state.snapshot = self.store.snapshot()?;
        state.generation += 1;
        Self::reconcile(&mut state);
        state.message = "Server file refreshed. Review the new targets before applying.".into();
        Ok(())
    }

    pub fn apply(&self) -> Result<()> {
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        let runtime = self.runtime();
        // Invalid desired input cannot change or invalidate a prior OS snapshot.
        let rules = resolve_rules(&runtime.snapshot.games, &runtime.snapshot.preferences)?;
        let result = firewall::change_with_readback(&Request::Apply { rules });
        match result {
            Ok(outcome) => {
                let ids = selected_game_ids(&runtime.snapshot.games, &runtime.snapshot.preferences);
                self.finish_apply(ids, outcome)
            }
            Err(error) => {
                self.invalidate_inspection(&error)?;
                self.state.lock().map_err(|e| e.to_string())?.message = error.clone();
                Err(error)
            }
        }
    }

    // Recovery hints never confirm blocking. Only the helper's OS readback does.
    fn save_recovery(&self, ids: Vec<String>) -> Result<()> {
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        state.snapshot.preferences.last_applied_game_ids = ids;
        self.store
            .save_preferences(&state.snapshot.preferences)
            .map_err(|error| {
                let message =
                    format!("Firewall updated, but saving recovery information failed: {error}");
                state.message = message.clone();
                message
            })
    }

    fn accept_readback(&self, result: Result<Inspection>) -> Result<()> {
        match result.and_then(|report| self.record_inspection(report)) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.invalidate_inspection(&error)?;
                Err(error)
            }
        }
    }

    fn finish_apply(&self, ids: Vec<String>, outcome: FirewallOutcome) -> Result<()> {
        let readback = self.accept_readback(outcome.inspection);
        let recovery = if outcome.operation_error.is_none() {
            let state = self.runtime();
            let hints = if readback.is_ok() && !state.dirty {
                // Preserve known games affected by shared system-wide rules,
                // even when their own switches are now off.
                state.applied_game_ids
            } else {
                // An unreadable or incomplete result must not erase recovery
                // access to previously owned targets.
                state
                    .snapshot
                    .preferences
                    .last_applied_game_ids
                    .into_iter()
                    .chain(state.applied_game_ids)
                    .chain(ids)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect()
            };
            self.save_recovery(hints)
        } else {
            Ok(())
        };
        if let Some(error) = outcome.operation_error {
            self.state.lock().map_err(|e| e.to_string())?.message = error.clone();
            return Err(error);
        }
        readback.map_err(|error| {
            format!("Firewall update completed, but its active rules could not be read: {error}")
        })?;
        recovery?;
        if self.runtime().dirty {
            return Err("Apply completed, but active LobbyLocker rules differ from saved selections. The firewall list shows the actual rules.".into());
        }
        self.state.lock().map_err(|e| e.to_string())?.message = "Firewall updated and read back successfully. Rules remain active when the app is closed.".into();
        Ok(())
    }

    pub fn reset(&self) -> Result<()> {
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        {
            let mut state = self.state.lock().map_err(|e| e.to_string())?;
            state.generation += 1;
        }
        match firewall::change_with_readback(&Request::Reset {}) {
            Ok(outcome) => self.finish_reset(outcome),
            Err(error) => {
                self.invalidate_inspection(&error)?;
                self.state.lock().map_err(|e| e.to_string())?.message = error.clone();
                Err(error)
            }
        }
    }

    fn finish_reset(&self, outcome: FirewallOutcome) -> Result<()> {
        let readback = self.accept_readback(outcome.inspection);
        if let Some(error) = outcome.operation_error {
            self.state.lock().map_err(|e| e.to_string())?.message = error.clone();
            return Err(error);
        }
        readback.map_err(|error| format!("Reset completed, but remaining rules could not be read. Saved selections and recovery hints were kept: {error}"))?;
        if self.runtime().applied_count != Some(0) {
            return Err("Reset completed, but active LobbyLocker rules remain. Saved selections and recovery hints were kept.".into());
        }
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        state.snapshot.preferences.last_applied_game_ids.clear();
        state.dirty = false;
        state.snapshot.preferences.blocked_regions.clear();
        self.store
            .save_preferences(&state.snapshot.preferences)
            .map_err(|e| {
                format!("Firewall rules were removed, but saving preferences failed: {e}")
            })?;
        state.message =
            "No active LobbyLocker blocks remain. Other firewall rules were untouched.".into();
        Ok(())
    }

    /// A read-only network check invoked by the user, never by a timer or startup.
    /// Measurements never change selections or firewall rules.
    pub fn measure_latency(&self, game_id: &str, region_id: Option<&str>) -> Result<()> {
        self.measure_latency_filtered(game_id, region_id, None)
    }

    pub fn measure_area_latency(&self, game_id: &str, area: &str) -> Result<()> {
        self.measure_latency_filtered(game_id, None, Some(area))
    }

    fn measure_latency_filtered(
        &self,
        game_id: &str,
        region_id: Option<&str>,
        area: Option<&str>,
    ) -> Result<()> {
        let before = self.runtime();
        let mut game = before
            .snapshot
            .games
            .iter()
            .find(|g| g.id == game_id)
            .cloned()
            .ok_or("Game not found.")?;
        if let Some(region_id) = region_id {
            game.regions.retain(|r| r.id == region_id);
            if game.regions.is_empty() {
                return Err("Server location not found.".into());
            }
        }
        if let Some(area) = area {
            game.regions.retain(|r| r.area == area);
            if game.regions.is_empty() {
                return Err("Server region not found.".into());
            }
        }
        if !game.regions.iter().any(|r| r.probe_target.is_some()) {
            return Err(
                "No ping targets configured. Open Advanced and add a responding probe IP.".into(),
            );
        }
        let samples = latency::measure(&[game]);
        let _operation = self.operation.lock().map_err(|e| e.to_string())?;
        self.record_latency(before.generation, &samples)
    }

    fn record_latency(&self, generation: u64, samples: &[PingSample]) -> Result<()> {
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        if state.generation != generation {
            return Ok(());
        }
        for sample in samples {
            if let Some(ms) = sample.latency_ms {
                let history = state
                    .histories
                    .entry(sample.region_key.clone())
                    .or_default();
                history.push(ms);
                if history.len() > 24 {
                    history.remove(0);
                }
            }
            state
                .samples
                .insert(sample.region_key.clone(), sample.clone());
        }
        state.message = "Manual ping check complete. No firewall rules changed.".into();
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/unit/engine.rs"]
mod tests;

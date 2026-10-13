import { invoke, isTauri } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { selectedLocale, translate } from "./i18n";
import { libraryGames, libraryCandidates, blockGameIds } from "./game";
import { missingPrograms } from "./firewall";
import { pingRequest } from "./ping";
import { Requests } from "./requests";
import type {
  Connection,
  Game,
  Languages,
  NewGame,
  Preferences,
  Scan,
  Status,
} from "./types";

export class Backend {
  status = $state<Status | null>(null);
  languages = $state<Languages>({ locales: [], warnings: [] });
  installed = $state<Scan>({ games: [], warnings: [] });
  scanned = $state(false);
  scanning = $state(false);
  connections = $state<Connection[]>([]);
  busy = $state(false);
  firewallAction = $state<"apply" | "reset" | "read" | null>(null);
  firewallSlow = $state(false);
  firewallFeedback = $state<"apply" | "reset" | "error" | null>(null);
  firewallError = $state("");
  error = $state("");
  selected = $state("");
  customIcons = $state<Record<string, string>>({});
  pickedIcon = $state<string | null>(null);
  updateVersion = $state("");
  updateNotes = $state("");
  updateChecking = $state(false);
  updateInstalling = $state(false);
  updateError = $state("");
  updateChecked = $state(false);
  updatePreview = $state(false);
  private availableUpdate: Update | null = null;
  private requests = new Requests(invoke);
  private iconPaths = "";
  private startupScanAttempted = false;
  private startupReadAttempted = false;
  private statusReloaded = false;
  private freshDataPending = new Set<string>();
  get games() {
    return libraryGames(
      this.status?.snapshot.games ?? [],
      this.status?.snapshot.preferences.libraryGameIds ?? [],
      blockGameIds(this.status),
    );
  }
  get candidates() {
    return libraryCandidates(
      this.status?.snapshot.games ?? [],
      this.installed,
      this.scanned,
      this.status?.snapshot.preferences.libraryGameIds ?? [],
      blockGameIds(this.status),
    );
  }
  get libraryFolders() {
    const added = new Set(
      this.status?.snapshot.preferences.gameLibraryPaths ?? [],
    );
    // Native discovery already canonicalizes saved paths and launcher aliases.
    return [...new Set(this.installed.libraryPaths ?? [...added])]
      .sort((a, b) => a.localeCompare(b))
      .map((path) => ({
        path,
        source: added.has(path) ? "Added by you" : "Automatically detected",
      }));
  }
  get game() {
    return this.games.find((game) => game.id === this.selected);
  }
  get code() {
    return this.status?.snapshot.preferences.language ?? "en";
  }
  get locale() {
    return selectedLocale(this.languages.locales, this.code);
  }
  t = (key: string, values: Record<string, string | number> = {}) =>
    translate(this.languages.locales, this.code, key, values);

  async initialize() {
    if (!isTauri()) {
      this.error =
        "Open LobbyLocker with pnpm dev. This browser page has no native backend.";
      return;
    }
    await this.run(async () => {
      const [status, languages] = await Promise.all([
        invoke<Status>("get_status"),
        invoke<Languages>("get_languages"),
      ]);
      this.status = status;
      this.languages = languages;
      this.selected = this.games[0]?.id ?? "";
      if (!this.startupReadAttempted) {
        this.startupReadAttempted = true;
        await this.firewallProgress("read", async () => {
          try {
            await this.readSnapshot();
            this.firewallError = "";
          } catch (error) {
            this.firewallError = String(error);
            // A denied read must not prevent the library from loading, nor
            // invent active blocks or pending changes from saved switches.
            try {
              await this.reload();
            } catch {
              // Keep the loaded library even if the status bridge also fails.
            }
            this.clearReadback();
          }
        });
      }
      this.selected = this.games[0]?.id ?? "";
      await this.refreshCustomIcons();
      if (!this.startupScanAttempted) {
        this.startupScanAttempted = true;
        await this.detectGames();
      }
      // Selections saved but not yet written are applied now rather than
      // waiting for the next toggle.
      void this.writeBlocks();
    }, false);
    if (this.status?.selfUpdateSupported) void this.checkForUpdates(true);
  }

  /** Refresh on page open; the backend limits feed requests to every 10 minutes. */
  async ensureFreshData(gameId: string) {
    if (!isTauri() || !gameId || this.freshDataPending.has(gameId)) return;
    this.freshDataPending.add(gameId);
    try {
      await invoke("ensure_fresh_data", { gameId });
      await this.reload();
    } catch {
      // Never surfaced. Offline use is normal, not an error worth showing.
    } finally {
      this.freshDataPending.delete(gameId);
    }
  }

  async checkForUpdates(silent = false) {
    if (
      this.updatePreview ||
      !isTauri() ||
      !this.status?.selfUpdateSupported ||
      this.updateChecking ||
      this.updateInstalling
    )
      return;
    this.updateChecking = true;
    this.updateError = "";
    try {
      if (this.availableUpdate) await this.availableUpdate.close();
      this.availableUpdate = await check({ timeout: 30_000 });
      this.updateVersion = this.availableUpdate?.version ?? "";
      this.updateNotes = this.availableUpdate?.body ?? "";
      this.updateChecked = true;
    } catch (error) {
      if (!silent) this.updateError = String(error);
    } finally {
      this.updateChecking = false;
    }
  }

  toggleUpdatePreview() {
    if (
      !import.meta.env.DEV ||
      this.busy ||
      this.updateChecking ||
      this.updateInstalling
    )
      return;
    this.updatePreview = !this.updatePreview;
    this.updateVersion = this.updatePreview
      ? "0.0.2"
      : (this.availableUpdate?.version ?? "");
    this.updateNotes = this.updatePreview
      ? "LOCAL PREVIEW — nothing will be installed."
      : (this.availableUpdate?.body ?? "");
    this.updateChecked = this.updatePreview || !!this.availableUpdate;
    this.updateError = "";
  }

  async installUpdate() {
    if (
      this.updatePreview ||
      !this.availableUpdate ||
      this.busy ||
      this.updateInstalling
    )
      return;
    this.busy = true;
    this.updateInstalling = true;
    this.updateError = "";
    try {
      await this.availableUpdate.downloadAndInstall();
      if (this.status?.snapshot.platform !== "windows") await relaunch();
    } catch (error) {
      this.updateError = String(error);
    } finally {
      this.updateInstalling = false;
      this.busy = false;
    }
  }

  async reload() {
    this.statusReloaded = false;
    this.status = await invoke<Status>("get_status");
    this.statusReloaded = true;
    if (!this.game) this.selected = this.games[0]?.id ?? "";
    await this.refreshCustomIcons();
  }

  private async refreshCustomIcons() {
    const paths = (this.status?.snapshot.games ?? [])
      .filter((game) => game.executablePath)
      .map((game) => [game.id, game.executablePath]);
    const fingerprint = JSON.stringify(paths);
    if (fingerprint === this.iconPaths) return;
    this.customIcons = paths.length
      ? await this.requests.request<Record<string, string>>("get_custom_icons")
      : {};
    this.iconPaths = fingerprint;
  }

  async run(action: () => Promise<unknown>, reload = true): Promise<boolean> {
    if (this.busy) return false;
    this.busy = true;
    this.error = "";
    this.firewallFeedback = null;
    let success = false;
    try {
      await action();
      success = true;
    } catch (error) {
      this.error = String(error);
    } finally {
      if (reload) {
        try {
          await this.reload();
        } catch (error) {
          this.error = [this.error, String(error)].filter(Boolean).join("\n");
          success = false;
        }
      }
      this.busy = false;
    }
    return success;
  }

  command(name: string, args?: Record<string, unknown>) {
    if (
      name === "apply_rules" ||
      name === "reset_rules" ||
      name === "verify_firewall"
    ) {
      return this.updateFirewall(
        name,
        name === "apply_rules"
          ? "apply"
          : name === "reset_rules"
            ? "reset"
            : "read",
        args,
      );
    }
    return this.run(() => this.requests.request(name, args));
  }
  private async updateFirewall(
    command: string,
    action: "apply" | "reset" | "read",
    args?: Record<string, unknown>,
  ) {
    if (this.busy) return false;
    return this.firewallProgress(action, async () => {
      this.statusReloaded = false;
      const success = await this.run(() =>
        action === "read"
          ? this.readSnapshot()
          : args
            ? this.requests.request(command, args)
            : this.requests.request(command),
      );
      if (!success && !this.statusReloaded && this.status) {
        // Without a fresh backend response, do not display stale OS status. A
        // reported mutation error WITH successful readback still shows actual rules.
        this.clearReadback();
      }
      if (this.status?.appliedCount != null) this.firewallError = "";
      else
        this.firewallError =
          this.status?.verificationError ||
          this.error ||
          "Could not read firewall status.";
      if (action === "read") this.error = "";
      else if (
        !success &&
        this.statusReloaded &&
        this.status?.verificationError &&
        this.error.includes(this.status.verificationError)
      ) {
        // A completed update with unreadable results is one failure, not two
        // alerts. Keep the operation context in the expandable readback details.
        this.firewallError = this.error;
        this.error = "";
      }
      this.firewallFeedback =
        action === "read" ? null : success ? action : "error";
      return success;
    });
  }
  readFirewall() {
    return this.command("verify_firewall", { elevated: true });
  }
  private async readSnapshot() {
    // Native inspection tries existing privileges first, then requests OS
    // authentication if necessary. This command cannot mutate firewall rules.
    const inspected = await this.requests.request<Status>("verify_firewall", {
      elevated: true,
    });
    if (inspected) this.status = inspected;
    if (!inspected || inspected.appliedCount === null) {
      throw new Error(
        inspected?.verificationError || "Could not read firewall status.",
      );
    }
  }
  private clearReadback() {
    if (!this.status) return;
    this.status.appliedCount = null;
    this.status.appliedTargets = null;
    this.status.verification = "unverified";
    this.status.regionStates = {};
    this.status.inspectedAt = null;
    // Unknown blocking is not itself an edit. Keep the backend's pending state.
  }
  private async firewallProgress<T>(
    action: "apply" | "reset" | "read",
    operation: () => Promise<T>,
  ) {
    this.firewallAction = action;
    this.firewallSlow = false;
    // A progress hint, not a timeout: never release the guard while the helper runs.
    const timer = setTimeout(() => {
      this.firewallSlow = true;
    }, 30_000);
    try {
      return await operation();
    } finally {
      clearTimeout(timer);
      this.firewallAction = null;
      this.firewallSlow = false;
    }
  }
  preferences(change: (preferences: Preferences) => void) {
    if (!this.status || this.busy) return Promise.resolve(false);
    // An independent plain copy: the rollback must not share structure with the
    // live preferences, or a later mutation would rewrite the restore value.
    const previous = structuredClone(
      $state.snapshot(this.status.snapshot.preferences),
    ) as Preferences;
    const preferences = structuredClone(previous);
    const before = JSON.stringify(preferences.blockedRegions);
    change(preferences);
    const blocksChanged = JSON.stringify(preferences.blockedRegions) !== before;
    return this.command("set_preferences", { preferences }).then((saved) => {
      // Only a toggle writes rules. Adding targets or refreshing server data
      // must never change the firewall on its own.
      if (saved && blocksChanged) {
        void this.writeBlocks(previous);
      }
      return saved;
    });
  }

  apply() {
    return this.command("apply_rules");
  }

  /**
   * Writes pending selections immediately.
   *
   * Never runs while another operation holds the guard, and never for a game
   * that still needs an executable, so a pending choice cannot silently reach
   * the firewall in a partial state.
   *
   * A rejected administrator prompt leaves the firewall untouched, so the
   * selection is put back: a saved switch must never claim a state the
   * operating system does not have.
   */
  private async writeBlocks(rollback?: Preferences) {
    if (this.busy || !this.status?.dirty) return;
    if (missingPrograms(this.status).length) return;
    if (await this.apply()) return;
    if (!rollback || this.busy) return;
    await this.command("set_preferences", { preferences: rollback });
  }

  saveGame(game: Game) {
    return this.command("save_game", { game: $state.snapshot(game) });
  }
  private async detectGames() {
    this.scanning = true;
    try {
      this.installed = await this.requests.request<Scan>("scan_games");
      this.scanned = true;
      if (!this.game) this.selected = this.games[0]?.id ?? "";
    } finally {
      this.scanning = false;
    }
  }
  scan() {
    return this.run(async () => {
      this.startupScanAttempted = true;
      this.iconPaths = "";
      await this.detectGames();
    });
  }
  async addLibrary() {
    let added = false;
    const success = await this.run(async () => {
      const scan = await this.requests.request<Scan | null>(
        "select_game_library",
      );
      if (scan) {
        this.installed = scan;
        this.scanned = true;
        added = true;
      }
    });
    return success && added;
  }
  saveLibraryGames(ids: string[]) {
    return this.command("set_library_games", { ids: [...new Set(ids)] });
  }
  removeLibrary(path: string) {
    return this.run(async () => {
      this.installed = await this.requests.request<Scan>(
        "remove_game_library",
        { path },
      );
      this.scanned = true;
    });
  }
  discover() {
    return this.run(async () => {
      this.connections = await this.requests.request<Connection[]>(
        "discover_connections",
      );
    });
  }
  checkPing(regionId?: string) {
    const request = pingRequest(this.game, regionId);
    return request
      ? this.command("check_ping", request)
      : Promise.resolve(false);
  }
  async pickExecutable(): Promise<string | null> {
    let path: string | null = null;
    await this.run(async () => {
      const selection = await this.requests.request<{
        path: string;
        icon: string | null;
      } | null>("select_game_executable");
      if (selection) {
        path = selection.path;
        this.pickedIcon = selection.icon;
      }
    }, false);
    return path;
  }
  create(input: NewGame) {
    return this.run(async () => {
      this.selected = await this.requests.request<string>("create_game", {
        input,
      });
    });
  }
  importGame() {
    return this.run(async () => {
      const id = await this.requests.request<string | null>("import_game");
      if (id) {
        this.selected = id;
      }
    });
  }
}

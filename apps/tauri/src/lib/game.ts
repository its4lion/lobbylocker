import type {
  Game,
  Preferences,
  Region,
  RegionState,
  Scan,
  Status,
} from "./types";

const builtinGames = new Set(["cs2", "deadlock", "overwatch2", "dbd"]);

/** Sidebar membership is explicit. Blocking games stay accessible for recovery. */
export function libraryGames(
  configured: Game[],
  chosenIds: string[],
  forcedIds: string[] = [],
): Game[] {
  const visible = new Set([...chosenIds, ...forcedIds]);
  return configured.filter((game) => visible.has(game.id));
}

export function blockGameIds(status: Status | null): string[] {
  if (!status) return [];
  return [
    ...new Set([
      ...status.appliedGameIds,
      ...status.snapshot.preferences.lastAppliedGameIds,
      ...status.snapshot.preferences.blockedRegions.map(
        (key) => key.split(":")[0],
      ),
    ]),
  ];
}

export function gameBlockState(
  id: string,
  status: Status | null,
): "blocked" | "none" | undefined {
  if (!status || status.appliedCount === null) return undefined;
  const states = Object.entries(status.regionStates ?? {})
    .filter(([key]) => key.startsWith(`${id}:`))
    .map(([, state]) => state);
  // A binary badge means some current targets are blocked, not that every
  // server is covered. Exact coverage remains available in the firewall list.
  if (states.some((state) => state === "blocked" || state === "partial"))
    return "blocked";
  if (states.length && states.every((state) => state === "none")) return "none";
  return status.appliedCount === 0 ? "none" : undefined;
}

function isSteamTool(id: string, name: string) {
  if (!id.startsWith("steam-")) return false;
  const lower = name.toLowerCase();
  return (
    [
      "steam-1493710",
      "steam-1070560",
      "steam-1391110",
      "steam-1628350",
      "steam-228980",
      "steam-250820",
    ].includes(id) ||
    lower === "proton" ||
    lower.startsWith("proton ") ||
    lower.startsWith("steam linux runtime") ||
    lower.startsWith("steamworks common redistributables") ||
    lower === "steamvr"
  );
}

export interface LibraryCandidate {
  id: string;
  name: string;
  configured: boolean;
  availability: "Installed" | "Custom game" | "Not found";
  launcher?: string;
}

/** Built-in JSON files alone are not choices; runtimes and launcher tools are hidden. */
export function libraryCandidates(
  configured: Game[],
  scan: Scan,
  scanned: boolean,
  chosenIds: string[],
  forcedIds: string[] = [],
): LibraryCandidate[] {
  const known = new Set([...chosenIds, ...forcedIds]);
  const detected = new Map(
    (scanned ? scan.games : []).map((game) => [game.id, game]),
  );
  const choices = new Map<string, LibraryCandidate>();
  for (const game of configured) {
    const installed = detected.get(game.id);
    if (!installed && builtinGames.has(game.id) && !known.has(game.id))
      continue;
    if (isSteamTool(game.id, game.name) && !forcedIds.includes(game.id))
      continue;
    choices.set(game.id, {
      id: game.id,
      name: game.name,
      configured: true,
      launcher: installed?.launcher,
      availability: installed
        ? "Installed"
        : builtinGames.has(game.id) || /^(steam|epic|xbox)-/.test(game.id)
          ? "Not found"
          : "Custom game",
    });
  }
  for (const game of detected.values()) {
    if (!choices.has(game.id) && !isSteamTool(game.id, game.name))
      choices.set(game.id, {
        id: game.id,
        name: game.name,
        launcher: game.launcher,
        configured: false,
        availability: "Installed",
      });
  }
  return [...choices.values()].sort((a, b) => a.name.localeCompare(b.name));
}

export const areas = [
  "Europe",
  "Middle East",
  "North America",
  "South America",
  "Asia",
  "Oceania",
  "Africa",
  "Custom",
];
export function areasInOrder(regions: Region[]): string[] {
  return [...new Set(regions.map((region) => region.area))].sort((a, b) => {
    const ai = areas.indexOf(a),
      bi = areas.indexOf(b);
    return (
      (ai < 0 ? areas.length : ai) - (bi < 0 ? areas.length : bi) ||
      a.localeCompare(b)
    );
  });
}
export function gameAreas(game: Game): string[] {
  return areasInOrder(game.regions);
}
export function regionKey(game: Game, region: Region): string {
  return `${game.id}:${region.id}`;
}
export function toggleKeys(
  values: string[],
  keys: string[],
  enabled: boolean,
): string[] {
  const remaining = values.filter((value) => !keys.includes(value));
  return enabled ? [...new Set([...remaining, ...keys])] : remaining;
}
export function areaSelection(
  game: Game,
  area: string,
  preferences: Preferences,
) {
  const keys = game.regions
    .filter((r) => r.area === area && r.endpoints.length > 0)
    .map((r) => regionKey(game, r));
  const selected = keys.filter((key) =>
    preferences.blockedRegions.includes(key),
  ).length;
  return {
    keys,
    selected,
    checked: keys.length > 0 && selected === keys.length,
  };
}

export function regionSelection(
  game: Game,
  region: Region,
  preferences?: Preferences,
) {
  const key = regionKey(game, region);
  const available = region.endpoints.length > 0;
  return {
    key,
    available,
    checked: available && (preferences?.blockedRegions.includes(key) ?? false),
  };
}

/** Saved switches describe desired configuration, never current OS blocking. */
export function selectionDescription(
  checked: boolean,
  selected: number,
): string {
  return !checked && selected ? "Some servers selected" : "";
}

export function regionBlockState(
  status: Status | null,
  key: string,
): RegionState {
  if (!status || status.appliedCount === null) return "unknown";
  if (status.appliedCount === 0) return "none";
  return status.regionStates?.[key] ?? "unknown";
}

export function blockDescription(state: RegionState): string {
  return state === "blocked" || state === "partial"
    ? "Blocked"
    : state === "none"
      ? "Not blocked"
      : "";
}

export function regionBlockDescription(
  status: Status | null,
  game: Game,
  region: Region,
): string {
  return region.endpoints.length
    ? blockDescription(regionBlockState(status, regionKey(game, region)))
    : "No targets configured";
}

/** Binary labels report block presence; don't imply complete coverage. */
export function blockNote(status: Status | null, keys: string[]): string {
  const states = keys.map((key) => regionBlockState(status, key));
  if (!states.length || states.every((state) => state === "unknown")) return "";
  return states.includes("partial") ||
    (states.includes("blocked") && states.some((state) => state !== "blocked"))
    ? "Some targets are blocked. See Firewall for the exact addresses, protocols, ports, and executable scopes."
    : "Based on rules read from the OS firewall. Only LobbyLocker rules are included.";
}

export function areaBlockDescription(
  status: Status | null,
  keys: string[],
): string {
  const states = keys.map((key) => regionBlockState(status, key));
  if (!states.length) return "No targets configured";
  if (states.some((state) => state === "blocked" || state === "partial"))
    return "Blocked";
  return states.every((state) => state === "none") ? "Not blocked" : "";
}

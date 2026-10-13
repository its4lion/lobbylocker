export type Protocol = "udp" | "tcp" | "any";
export type RegionState = "blocked" | "partial" | "none" | "unknown";
export interface Endpoint {
  id: string;
  address: string;
  protocol: Protocol;
  ports: string | null;
  custom: boolean;
}
export interface Region {
  id: string;
  name: string;
  area: string;
  probeTarget: string | null;
  probeCustom?: boolean;
  providerScope?: string;
  endpoints: Endpoint[];
}
export interface Game {
  schemaVersion: number;
  id: string;
  name: string;
  source: string | null;
  updatedAt: string | null;
  executablePath?: string | null;
  note: string;
  regions: Region[];
}
export interface Preferences {
  schemaVersion: number;
  blockedRegions: string[];
  gameLibraryPaths: string[];
  excludedGameLibraryPaths: string[];
  libraryGameIds: string[];
  lastAppliedGameIds: string[];
  language: string;
}
export interface PingSample {
  regionKey: string;
  target: string;
  latencyMs: number | null;
  error: string | null;
  measuredAt: number;
}
export interface FirewallTarget {
  gameId: string;
  gameName: string;
  regionId: string;
  regionName: string;
  executablePath?: string | null;
  endpoints: Endpoint[];
}
export interface Status {
  snapshot: {
    games: Game[];
    preferences: Preferences;
    configDir: string;
    platform: string;
  };
  selfUpdateSupported?: boolean;
  samples: Record<string, PingSample>;
  histories: Record<string, number[]>;
  appliedTargets: FirewallTarget[] | null;
  appliedCount: number | null;
  appliedGameIds: string[];
  regionStates?: Record<string, RegionState>;
  inspectedAt?: number | null;
  verification?: "unverified" | "matched" | "empty" | "different";
  verificationError?: string;
  dirty: boolean;
  message: string;
}
export interface Locale {
  code: string;
  name: string;
  direction: "ltr" | "rtl";
  messages: Record<string, string>;
}
export interface Languages {
  locales: Locale[];
  warnings: string[];
}
export interface InstalledGame {
  id: string;
  name: string;
  launcher: string;
  directory: string;
}
export interface Scan {
  games: InstalledGame[];
  warnings: string[];
  icons?: Record<string, string>;
  libraryPaths?: string[];
}
export interface Connection {
  protocol: string;
  remoteIp: string;
  remotePort: number;
  process: string;
  state: string;
}
export interface NewGame {
  candidateId: string | null;
  name: string;
  area: string;
  address: string;
  protocol: Protocol;
  ports: string | null;
  executablePath: string | null;
}

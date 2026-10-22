import { beforeEach, describe, expect, it, vi } from "vitest";
import { Backend } from "./backend.svelte";
import type { Status, Game } from "./types";

const native = vi.hoisted(() => ({
  invoke: vi.fn(),
  isTauri: vi.fn(() => true),
}));
const updater = vi.hoisted(() => ({
  check: vi.fn(),
  relaunch: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => native);
vi.mock("@tauri-apps/plugin-updater", () => ({ check: updater.check }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: updater.relaunch }));

function status(): Status {
  return {
    snapshot: {
      games: [],
      configDir: "test",
      platform: "linux",
      preferences: {
        schemaVersion: 1,
        blockedRegions: [],
        gameLibraryPaths: [],
        excludedGameLibraryPaths: [],
        libraryGameIds: [],
        lastAppliedGameIds: [],
        language: "en",
      },
    },
    samples: {},
    histories: {},
    appliedTargets: null,
    appliedCount: null,
    appliedGameIds: [],
    dirty: false,
    message: "Loaded",
  };
}

function emptyRead(loaded = status()): Status {
  loaded.appliedCount = 0;
  loaded.appliedTargets = [];
  loaded.verification = "empty";
  loaded.dirty = loaded.snapshot.preferences.blockedRegions.length > 0;
  return loaded;
}

beforeEach(() => {
  vi.clearAllMocks();
  native.isTauri.mockReturnValue(true);
  updater.check.mockResolvedValue(null);
  native.invoke.mockImplementation(async (command: string) => {
    if (command === "get_status") return status();
    if (command === "get_languages") return { locales: [], warnings: [] };
    if (command === "verify_firewall") return emptyRead();
    if (command === "scan_games") return { games: [], warnings: [] };
    return undefined;
  });
});

describe("native bridge", () => {
  it("serializes background icon reads with user actions without leaking native busy errors", async () => {
    const loaded = emptyRead();
    loaded.snapshot.games = [
      { id: "example", name: "Example", executablePath: "/games/example.exe" },
    ] as Game[];
    loaded.snapshot.preferences.libraryGameIds = ["example"];
    let finishIcons!: () => void;
    let locked = false;
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "get_status") return structuredClone(loaded);
      if (command === "ensure_fresh_data") return;
      if (locked) throw "Another operation is running. Please wait.";
      if (command === "get_custom_icons") {
        locked = true;
        await new Promise<void>((resolve) => {
          finishIcons = resolve;
        });
        locked = false;
        return {};
      }
    });
    const app = new Backend();
    app.status = loaded;
    const refresh = app.ensureFreshData("example");
    await vi.waitFor(() => expect(locked).toBe(true));
    expect(app.busy).toBe(false);
    const change = app.command("set_language", { code: "ar" });
    expect(app.busy).toBe(true);
    await Promise.resolve();
    expect(
      native.invoke.mock.calls.some(([command]) => command === "set_language"),
    ).toBe(false);
    finishIcons();
    await refresh;
    expect(await change).toBe(true);
    expect(app.error).toBe("");
    expect(app.busy).toBe(false);
  });

  it("checks all or one configured server only on explicit request, without changing blocks", async () => {
    const loaded = emptyRead();
    loaded.snapshot.games = [
      {
        schemaVersion: 1,
        id: "example",
        name: "Example",
        source: null,
        updatedAt: null,
        note: "",
        regions: [
          {
            id: "a",
            name: "A",
            area: "Europe",
            probeTarget: "203.0.113.1",
            endpoints: [],
          },
          {
            id: "b",
            name: "B",
            area: "Europe",
            probeTarget: "203.0.113.2",
            endpoints: [],
          },
          {
            id: "c",
            name: "C",
            area: "Asia",
            probeTarget: null,
            endpoints: [],
          },
        ],
      },
    ];
    loaded.snapshot.preferences.libraryGameIds = ["example"];
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "get_status") return structuredClone(loaded);
    });
    const app = new Backend();
    app.status = loaded;
    app.selected = "example";
    expect(native.invoke).not.toHaveBeenCalled();
    expect(await app.checkPing()).toBe(true);
    expect(native.invoke).toHaveBeenCalledWith("check_ping", {
      gameId: "example",
    });
    expect(await app.checkPing("b")).toBe(true);
    expect(native.invoke).toHaveBeenCalledWith("check_ping", {
      gameId: "example",
      regionId: "b",
    });
    expect(await app.checkPing("c")).toBe(false);
    expect(await app.checkPing("missing")).toBe(false);
    expect(
      native.invoke.mock.calls.filter(([command]) => command === "check_ping"),
    ).toHaveLength(2);
    expect(app.status?.snapshot.preferences.blockedRegions).toEqual([]);
    expect(
      native.invoke.mock.calls.every(([command]) =>
        ["check_ping", "get_status"].includes(command),
      ),
    ).toBe(true);
  });

  it("selects the first library game on startup and preserves valid choices on reload", async () => {
    const loaded = emptyRead();
    loaded.snapshot.games = [
      { id: "cs2", name: "Counter-Strike 2" },
      { id: "deadlock", name: "Deadlock" },
    ] as Game[];
    loaded.snapshot.preferences.libraryGameIds = ["cs2", "deadlock"];
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "get_status" || command === "verify_firewall")
        return structuredClone(loaded);
      if (command === "get_languages") return { locales: [], warnings: [] };
      if (command === "scan_games") return { games: [], warnings: [] };
    });

    const app = new Backend();
    await app.initialize();
    expect(app.selected).toBe("cs2");
    app.selected = "deadlock";
    await app.reload();
    expect(app.selected).toBe("deadlock");

    loaded.snapshot.preferences.libraryGameIds = ["cs2"];
    await app.reload();
    expect(app.selected).toBe("cs2");
    loaded.snapshot.preferences.libraryGameIds = [];
    await app.reload();
    expect(app.selected).toBe("");
    expect(
      native.invoke.mock.calls.some(([command]) =>
        [
          "apply_rules",
          "reset_rules",
          "check_ping",
          "set_preferences",
        ].includes(command),
      ),
    ).toBe(false);
  });

  it("checks automatically and installs a signed update only after confirmation", async () => {
    const loaded = emptyRead();
    loaded.selfUpdateSupported = true;
    const update = {
      version: "0.0.2",
      body: "Security and reliability fixes.",
      close: vi.fn(),
      downloadAndInstall: vi.fn(),
    };
    updater.check.mockResolvedValue(update);
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "get_status" || command === "verify_firewall")
        return loaded;
      if (command === "get_languages") return { locales: [], warnings: [] };
      if (command === "scan_games") return { games: [], warnings: [] };
    });

    const app = new Backend();
    await app.initialize();
    await vi.waitFor(() => expect(app.updateVersion).toBe("0.0.2"));
    expect(update.downloadAndInstall).not.toHaveBeenCalled();

    await app.installUpdate();
    expect(update.downloadAndInstall).toHaveBeenCalledOnce();
    expect(updater.relaunch).toHaveBeenCalledOnce();
  });

  it("detects once on startup but keeps games hidden until chosen", async () => {
    const loaded = status();
    loaded.snapshot.games = ["cs2", "deadlock", "overwatch2"].map(
      (id) => ({ id, name: id }) as Game,
    );
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "get_status") return loaded;
      if (command === "get_languages") return { locales: [], warnings: [] };
      if (command === "verify_firewall") return emptyRead(loaded);
      if (command === "scan_games")
        return {
          games: [
            {
              id: "deadlock",
              name: "Deadlock",
              launcher: "Steam",
              directory: "/games/deadlock",
            },
          ],
          warnings: [],
        };
    });
    const app = new Backend();
    expect(app.games).toEqual([]);
    expect(app.selected).toBe("");
    expect(app.game).toBeUndefined();
    await app.initialize();
    expect(app.games).toEqual([]);
    expect(app.candidates.map((g) => g.id)).toEqual(["deadlock"]);
    expect(app.selected).toBe("");
    expect(native.invoke.mock.calls.map((call) => call[0])).toEqual([
      "get_status",
      "get_languages",
      "verify_firewall",
      "scan_games",
    ]);
    await app.initialize();
    expect(
      native.invoke.mock.calls.filter(([command]) => command === "scan_games"),
    ).toHaveLength(1);
    await app.scan();
    expect(
      native.invoke.mock.calls.filter(([command]) => command === "scan_games"),
    ).toHaveLength(2);
  });
  it("uses a read-only native path picker without scans, pings, or preference changes", async () => {
    const icon = "data:image/png;base64,aWNvbg==";
    native.invoke.mockResolvedValueOnce({ path: "/games/my-game.exe", icon });
    const app = new Backend();
    expect(await app.pickExecutable()).toBe("/games/my-game.exe");
    expect(app.pickedIcon).toBe(icon);
    expect(native.invoke.mock.calls.map((call) => call[0])).toEqual([
      "select_game_executable",
    ]);
  });
  it("adds resolved Overwatch selections as the built-in game without replacing server data or applying blocks", async () => {
    const loaded = emptyRead();
    loaded.snapshot.platform = "windows";
    loaded.snapshot.preferences.libraryGameIds = ["custom-existing"];
    loaded.snapshot.preferences.blockedRegions = ["overwatch2:eu"];
    loaded.dirty = true;
    const game: Game = {
      schemaVersion: 1,
      id: "overwatch2",
      name: "Overwatch 2",
      source: "bundled catalogue",
      updatedAt: null,
      note: "My custom note",
      regions: [
        {
          id: "eu",
          name: "Europe",
          area: "Europe",
          probeTarget: "203.0.113.8",
          endpoints: [
            {
              id: "custom-target",
              address: "203.0.113.9",
              protocol: "udp",
              ports: null,
              custom: true,
            },
          ],
        },
      ],
    };
    loaded.snapshot.games = [
      game,
      { ...game, id: "custom-existing", name: "Existing game" },
    ];
    const selection = {
      path: "C:\\Program Files (x86)\\Overwatch\\_retail_\\Overwatch.exe",
      icon: null,
      gameId: "overwatch2",
    };
    native.invoke.mockImplementation(
      async (command: string, args?: Record<string, unknown>) => {
        if (command === "select_game_executable") return selection;
        if (command === "set_game_executable") {
          expect(args).toEqual({
            gameId: "overwatch2",
            path: selection.path,
          });
          loaded.snapshot.games[0].executablePath = selection.path;
          return true;
        }
        if (command === "set_library_games")
          loaded.snapshot.preferences.libraryGameIds = args?.ids as string[];
        if (command === "get_status") return structuredClone(loaded);
        if (command === "get_custom_icons") return {};
      },
    );
    const app = new Backend();
    app.status = structuredClone(loaded);
    app.selected = "custom-existing";
    expect(await app.addExecutable()).toEqual(selection);
    expect(app.selected).toBe("overwatch2");
    expect(app.game).toEqual({ ...game, executablePath: selection.path });
    expect(app.status?.snapshot.preferences.libraryGameIds).toEqual([
      "custom-existing",
      "overwatch2",
    ]);
    expect(app.status?.snapshot.preferences.blockedRegions).toEqual([
      "overwatch2:eu",
    ]);
    expect(native.invoke.mock.calls.map(([command]) => command)).toEqual([
      "select_game_executable",
      "set_game_executable",
      "set_library_games",
      "get_status",
      "get_custom_icons",
    ]);
    await app.addExecutable();
    expect(app.status?.snapshot.preferences.libraryGameIds).toEqual([
      "custom-existing",
      "overwatch2",
    ]);
  });
  it("keeps ordinary executable selections read-only for the custom-game form", async () => {
    const selection = {
      path: "C:\\Games\\My game.exe",
      icon: "data:image/png;base64,aWNvbg==",
      gameId: null,
    };
    native.invoke.mockResolvedValueOnce(selection);
    const app = new Backend();
    expect(await app.addExecutable()).toEqual(selection);
    expect(app.pickedIcon).toBe(selection.icon);
    expect(native.invoke.mock.calls.map(([command]) => command)).toEqual([
      "select_game_executable",
    ]);
  });
  it("does not modify games on cancellation or an unresolved launcher error", async () => {
    const app = new Backend();
    app.status = status();
    app.pickedIcon = "data:image/png;base64,aWNvbg==";
    native.invoke.mockResolvedValueOnce(null);
    expect(await app.addExecutable()).toBeNull();
    const error =
      "Overwatch Launcher.exe only starts the game. Choose Overwatch.exe in the _retail_ folder after completing the Battle.net installation.";
    native.invoke.mockRejectedValueOnce(error);
    expect(await app.addExecutable()).toBeNull();
    expect(app.error).toBe(error);
    expect(app.pickedIcon).toBe("data:image/png;base64,aWNvbg==");
    expect(app.status.snapshot.preferences.libraryGameIds).toEqual([]);
    expect(
      native.invoke.mock.calls.every(
        ([command]) => command === "select_game_executable",
      ),
    ).toBe(true);
  });
  it("does not add or change a game when executable replacement is declined", async () => {
    const app = new Backend();
    app.status = status();
    app.status.snapshot.games = [
      { id: "overwatch2", executablePath: "C:\\Existing\\Overwatch.exe" },
    ] as Game[];
    app.selected = "custom-existing";
    native.invoke.mockResolvedValueOnce({
      path: "D:\\Overwatch\\_retail_\\Overwatch.exe",
      icon: null,
      gameId: "overwatch2",
    });
    native.invoke.mockResolvedValueOnce(false);
    expect(await app.addExecutable()).toBeNull();
    expect(app.status.snapshot.games[0].executablePath).toBe(
      "C:\\Existing\\Overwatch.exe",
    );
    expect(app.status.snapshot.preferences.libraryGameIds).toEqual([]);
    expect(app.selected).toBe("custom-existing");
    expect(native.invoke.mock.calls.map(([command]) => command)).toEqual([
      "select_game_executable",
      "set_game_executable",
    ]);
  });
  it("uses detected Windows executables on startup and blocks without a second picker", async () => {
    const loaded = emptyRead();
    loaded.snapshot.platform = "windows";
    loaded.snapshot.preferences.libraryGameIds = ["cs2"];
    loaded.snapshot.games = [
      {
        schemaVersion: 1,
        id: "cs2",
        name: "Counter-Strike 2",
        source: null,
        updatedAt: null,
        note: "",
        regions: [
          {
            id: "eu",
            name: "Europe",
            area: "Europe",
            probeTarget: null,
            endpoints: [
              {
                id: "server",
                address: "203.0.113.8",
                protocol: "udp",
                ports: null,
                custom: false,
              },
            ],
          },
        ],
      },
    ];
    const path = "C:\\Games\\Counter-Strike 2\\game\\bin\\win64\\cs2.exe";
    native.invoke.mockImplementation(
      async (command: string, args?: Record<string, unknown>) => {
        if (command === "get_status" || command === "verify_firewall")
          return structuredClone(loaded);
        if (command === "get_languages") return { locales: [], warnings: [] };
        if (command === "get_custom_icons") return {};
        if (command === "scan_games") {
          loaded.snapshot.games[0].executablePath = path;
          return {
            games: [
              {
                id: "cs2",
                name: "Counter-Strike 2",
                launcher: "Steam",
                directory: "C:\\Games\\Counter-Strike 2",
              },
            ],
            warnings: [],
          };
        }
        if (command === "set_preferences") {
          loaded.snapshot.preferences = structuredClone(
            args?.preferences as Status["snapshot"]["preferences"],
          );
          loaded.dirty = true;
        }
        if (command === "apply_rules") {
          expect(loaded.snapshot.games[0].executablePath).toBe(path);
          loaded.dirty = false;
        }
      },
    );
    const app = new Backend();
    await app.initialize();
    expect(app.game?.executablePath).toBe(path);
    expect(native.invoke.mock.calls.map(([command]) => command)).toEqual([
      "get_status",
      "get_languages",
      "verify_firewall",
      "scan_games",
      "get_status",
      "get_custom_icons",
    ]);
    await app.preferences((preferences) =>
      preferences.blockedRegions.push("cs2:eu"),
    );
    await vi.waitFor(() =>
      expect(native.invoke).toHaveBeenCalledWith("apply_rules", undefined),
    );
    await vi.waitFor(() => expect(app.busy).toBe(false));
    expect(app.game?.executablePath).toBe(path);
    expect(
      native.invoke.mock.calls.some(([command]) =>
        ["select_game_executable", "set_game_executable", "save_game"].includes(
          command,
        ),
      ),
    ).toBe(false);
  });
  it("keeps the previously selected executable icon when Browse is cancelled", async () => {
    native.invoke.mockResolvedValueOnce(null);
    const app = new Backend();
    app.pickedIcon = "data:image/png;base64,aWNvbg==";
    expect(await app.pickExecutable()).toBeNull();
    expect(app.pickedIcon).toBe("data:image/png;base64,aWNvbg==");
  });
  it("restores custom executable icons on startup and only refreshes after path changes", async () => {
    const loaded = status();
    const game = {
      id: "custom-example",
      name: "Example",
      executablePath: "/games/example.exe",
    } as Game;
    loaded.snapshot.games = [game];
    loaded.snapshot.preferences.libraryGameIds = [game.id];
    const icons = { "custom-example": "data:image/png;base64,aWNvbg==" };
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "get_status") return loaded;
      if (command === "get_languages") return { locales: [], warnings: [] };
      if (command === "verify_firewall") return emptyRead(loaded);
      if (command === "get_custom_icons") return icons;
      if (command === "scan_games") return { games: [], warnings: [] };
    });
    const app = new Backend();
    await app.initialize();
    expect(app.customIcons).toEqual(icons);
    expect(app.selected).toBe("custom-example");
    await app.reload();
    expect(
      native.invoke.mock.calls.filter(
        ([command]) => command === "get_custom_icons",
      ),
    ).toHaveLength(1);
    game.executablePath = "/games/replacement.exe";
    await app.reload();
    expect(
      native.invoke.mock.calls.filter(
        ([command]) => command === "get_custom_icons",
      ),
    ).toHaveLength(2);
    loaded.snapshot.games = [];
    await app.reload();
    expect(app.customIcons).toEqual({});
  });
  it("automatically reads the firewall with permission allowed and scans; never pings or applies", async () => {
    const app = new Backend();
    await app.initialize();
    expect(native.invoke.mock.calls.map((call) => call[0])).toEqual([
      "get_status",
      "get_languages",
      "verify_firewall",
      "scan_games",
    ]);
    expect(app.status?.appliedCount).toBe(0);
    expect(app.busy).toBe(false);
    expect(app.scanned).toBe(true);
    expect(app.scanning).toBe(false);
    expect(native.invoke).toHaveBeenCalledWith("verify_firewall", {
      elevated: true,
    });
  });
  it("restores blocking on startup with one read and no Apply", async () => {
    const loaded = status();
    loaded.snapshot.games = [{ id: "deadlock", name: "Deadlock" }] as Game[];
    loaded.snapshot.preferences.blockedRegions = ["deadlock:dxb"];
    native.invoke.mockImplementation(
      async (command: string, args?: { elevated: boolean }) => {
        if (command === "get_status") return loaded;
        if (command === "get_languages") return { locales: [], warnings: [] };
        if (command === "verify_firewall") {
          expect(args?.elevated).toBe(true);
          loaded.appliedCount = 2;
          loaded.appliedGameIds = ["deadlock"];
          loaded.dirty = false;
          loaded.verification = "matched";
          return loaded;
        }
        if (command === "scan_games") return { games: [], warnings: [] };
      },
    );
    const app = new Backend();
    await app.initialize();
    await app.initialize();
    expect(app.status?.verification).toBe("matched");
    expect(app.status?.dirty).toBe(false);
    expect(app.selected).toBe("deadlock");
    expect(
      native.invoke.mock.calls.filter(
        ([command]) => command === "verify_firewall",
      ),
    ).toHaveLength(1);
    expect(
      native.invoke.mock.calls.some(([command]) =>
        ["apply_rules", "reset_rules", "check_ping"].includes(command),
      ),
    ).toBe(false);
  });
  it("shows startup cancellation as an error, still scans, and never invents pending changes", async () => {
    const loaded = status();
    loaded.verification = "unverified";
    loaded.verificationError = "Operation not permitted";
    loaded.snapshot.preferences.blockedRegions = ["deadlock:dxb"];
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "get_status") return loaded;
      if (command === "verify_firewall") throw Error("Authorization cancelled");
      if (command === "get_languages") return { locales: [], warnings: [] };
      if (command === "scan_games") return { games: [], warnings: [] };
    });
    const app = new Backend();
    await app.initialize();
    expect(app.error).toBe("");
    expect(app.firewallError).toContain("Authorization cancelled");
    expect(app.status?.verificationError).toContain("not permitted");
    expect(app.status?.snapshot.preferences.blockedRegions).toEqual([
      "deadlock:dxb",
    ]);
    expect(app.busy).toBe(false);
    expect(app.scanned).toBe(true);
    expect(app.status?.dirty).toBe(false);
    expect(app.status?.appliedCount).toBeNull();
    expect(app.firewallAction).toBeNull();
    await app.initialize();
    expect(
      native.invoke.mock.calls.filter(
        ([command]) => command === "verify_firewall",
      ),
    ).toHaveLength(1);
    expect(
      native.invoke.mock.calls.some(([command]) =>
        ["apply_rules", "reset_rules", "set_preferences"].includes(command),
      ),
    ).toBe(false);
  });
  it("restores actual blocking with all saved switches off, without modifying preferences", async () => {
    const loaded = status();
    loaded.snapshot.games = [{ id: "deadlock", name: "Deadlock" }] as Game[];
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "get_status") return loaded;
      if (command === "get_languages") return { locales: [], warnings: [] };
      if (command === "verify_firewall") {
        loaded.appliedCount = 1;
        loaded.appliedGameIds = ["deadlock"];
        loaded.regionStates = { "deadlock:dxb": "blocked" };
        loaded.verification = "different";
        return loaded;
      }
      if (command === "scan_games") return { games: [], warnings: [] };
    });
    const app = new Backend();
    await app.initialize();
    expect(app.games.map((game) => game.id)).toEqual(["deadlock"]);
    expect(app.status?.snapshot.preferences.blockedRegions).toEqual([]);
    expect(app.status?.snapshot.preferences.lastAppliedGameIds).toEqual([]);
    expect(app.status?.regionStates?.["deadlock:dxb"]).toBe("blocked");
    expect(app.status?.appliedCount).toBe(1);
    expect(app.status?.verification).toBe("different");
    expect(native.invoke.mock.calls.map(([command]) => command)).toEqual([
      "get_status",
      "get_languages",
      "verify_firewall",
      "scan_games",
    ]);
  });
  it("read-error Retry restores status without a confirmation badge or applying rules", async () => {
    const loaded = status();
    loaded.appliedCount = 2;
    loaded.verification = "matched";
    loaded.dirty = false;
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "verify_firewall") return loaded;
      if (command === "get_status") return loaded;
    });
    const app = new Backend();
    app.firewallError = "Authorization cancelled";
    expect(await app.readFirewall()).toBe(true);
    expect(app.firewallFeedback).toBeNull();
    expect(app.firewallError).toBe("");
    expect(app.status?.appliedCount).toBe(2);
    expect(native.invoke.mock.calls.map(([command]) => command)).toEqual([
      "verify_firewall",
      "get_status",
    ]);
    expect(app.busy).toBe(false);
  });
  it("keeps read errors visible through navigation and unrelated operations without guessing block status", async () => {
    const loaded = status();
    loaded.snapshot.preferences.blockedRegions = ["deadlock:dxb"];
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "verify_firewall") throw Error("Authorization cancelled");
      if (command === "get_status") return loaded;
    });
    const app = new Backend();
    app.status = loaded;
    expect(await app.readFirewall()).toBe(false);
    expect(app.error).toBe("");
    expect(app.firewallError).toContain("Authorization cancelled");
    await app.command("set_language", { code: "ar" });
    expect(app.firewallError).toContain("Authorization cancelled");
    expect(app.status?.dirty).toBe(false);
    expect(app.status?.appliedCount).toBeNull();
    expect(app.status?.snapshot.preferences.blockedRegions).toEqual([
      "deadlock:dxb",
    ]);
  });
  it("holds the startup guard during authentication and allows only one read request", async () => {
    let complete!: (value: Status) => void;
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "get_status") return status();
      if (command === "get_languages") return { locales: [], warnings: [] };
      if (command === "verify_firewall")
        return new Promise<Status>((resolve) => {
          complete = resolve;
        });
      if (command === "scan_games") return { games: [], warnings: [] };
    });
    const app = new Backend();
    const initializing = app.initialize();
    await vi.waitFor(() => expect(app.firewallAction).toBe("read"));
    expect(app.busy).toBe(true);
    expect(await app.readFirewall()).toBe(false);
    expect(await app.command("apply_rules")).toBe(false);
    expect(await app.command("reset_rules")).toBe(false);
    complete(emptyRead());
    await initializing;
    expect(app.busy).toBe(false);
    expect(app.firewallAction).toBeNull();
    expect(app.firewallFeedback).toBeNull();
    expect(native.invoke.mock.calls.map(([command]) => command)).toEqual([
      "get_status",
      "get_languages",
      "verify_firewall",
      "scan_games",
    ]);
  });
  it("readable mismatched firewall rules remain authoritative even when a mutation reports failure", async () => {
    const loaded = status();
    loaded.appliedCount = 2;
    loaded.appliedGameIds = ["cs2"];
    loaded.regionStates = { "cs2:eu": "partial" };
    loaded.verification = "different";
    loaded.dirty = true;
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "apply_rules")
        throw Error("Only part of the update succeeded");
      if (command === "get_status") return loaded;
    });
    const app = new Backend();
    expect(await app.command("apply_rules")).toBe(false);
    expect(app.status?.appliedCount).toBe(2);
    expect(app.status?.regionStates).toEqual({ "cs2:eu": "partial" });
    expect(app.status?.verification).toBe("different");
    expect(app.firewallFeedback).toBe("error");
    expect(app.error).toContain("Only part");
  });
  it.each(["apply_rules", "reset_rules"])(
    "%s shows one plain-language readback alert and keeps operation details without retrying automatically",
    async (command) => {
      const loaded = status();
      loaded.snapshot.platform = "windows";
      loaded.verification = "unverified";
      loaded.verificationError =
        'Could not parse address reported by Windows ("64.224.26.0/255.0.255.0"): Invalid mask.';
      loaded.snapshot.preferences.blockedRegions = ["overwatch2:blizzard-ams1"];
      const error = `Firewall operation completed, but its active rules could not be read: ${loaded.verificationError}`;
      native.invoke.mockImplementation(async (name: string) => {
        if (name === command) throw Error(error);
        if (name === "get_status") return loaded;
      });
      const app = new Backend();
      expect(await app.command(command)).toBe(false);
      expect(app.error).toBe("");
      expect(app.firewallError).toContain(error);
      expect(app.firewallFeedback).toBe("error");
      expect(app.status?.verification).toBe("unverified");
      expect(app.status?.appliedCount).toBeNull();
      expect(app.status?.snapshot.preferences.blockedRegions).toEqual([
        "overwatch2:blizzard-ams1",
      ]);
      expect(native.invoke.mock.calls.map(([name]) => name)).toEqual([
        command,
        "get_status",
      ]);
    },
  );
  it("keeps a distinct mutation failure visible when readback also fails", async () => {
    const loaded = status();
    loaded.verificationError = "Readback failed";
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "apply_rules") throw Error("Update failed");
      if (command === "get_status") return loaded;
    });
    const app = new Backend();
    expect(await app.command("apply_rules")).toBe(false);
    expect(app.error).toContain("Update failed");
    expect(app.firewallError).toBe("Readback failed");
    expect(app.firewallFeedback).toBe("error");
  });
  it("keeps an empty scan empty and clears the discovery busy state after errors", async () => {
    const app = new Backend();
    await app.initialize();
    expect(app.games).toEqual([]);
    expect(app.selected).toBe("");
    native.invoke.mockRejectedValueOnce(new Error("Could not scan library"));
    expect(await app.scan()).toBe(false);
    expect(app.scanning).toBe(false);
    expect(app.busy).toBe(false);
    expect(app.error).toContain("Could not scan library");
  });
  it("keeps the current detected games when the library folder picker is cancelled", async () => {
    const app = new Backend();
    app.installed = {
      games: [
        { id: "cs2", name: "CS2", launcher: "Steam", directory: "/games/cs2" },
      ],
      warnings: [],
    };
    app.scanned = true;
    native.invoke.mockResolvedValueOnce(null);
    await app.addLibrary();
    expect(app.installed.games[0]?.id).toBe("cs2");
    expect(native.invoke.mock.calls.map(([command]) => command)).toEqual([
      "select_game_library",
      "get_status",
    ]);
  });
  it("updates detection and saved library paths without applying or measuring latency", async () => {
    const loaded = status();
    loaded.snapshot.games = [{ id: "cs2", name: "CS2" } as Game];
    loaded.snapshot.preferences.libraryGameIds = ["cs2"];
    const scan = {
      games: [
        {
          id: "cs2",
          name: "CS2",
          launcher: "Steam",
          directory: "/extra-library/cs2",
        },
      ],
      warnings: [],
    };
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "select_game_library") {
        loaded.snapshot.preferences.gameLibraryPaths = ["/extra-library"];
        return scan;
      }
      if (command === "remove_game_library") {
        loaded.snapshot.preferences.gameLibraryPaths = [];
        return { games: [], warnings: [] };
      }
      if (command === "get_status") return loaded;
    });
    const app = new Backend();
    expect(await app.addLibrary()).toBe(true);
    expect(app.selected).toBe("cs2");
    expect(app.status?.snapshot.preferences.gameLibraryPaths).toEqual([
      "/extra-library",
    ]);
    expect(await app.removeLibrary("/extra-library")).toBe(true);
    expect(app.games.map((game) => game.id)).toEqual(["cs2"]);
    expect(app.selected).toBe("cs2");
    expect(native.invoke.mock.calls.map(([command]) => command)).toEqual([
      "select_game_library",
      "get_status",
      "remove_game_library",
      "get_status",
    ]);
    expect(native.invoke).toHaveBeenCalledWith("remove_game_library", {
      path: "/extra-library",
    });
  });
  it("serializes preference snapshots without optimistic mutation or automatic Apply", async () => {
    const app = new Backend();
    app.status = status();
    await app.preferences((p) => {
      p.blockedRegions.push("cs2:eu");
    });
    expect(native.invoke).toHaveBeenCalledWith("set_preferences", {
      preferences: expect.objectContaining({ blockedRegions: ["cs2:eu"] }),
    });
    expect(app.status.snapshot.preferences.blockedRegions).toEqual([]);
    expect(native.invoke.mock.calls.map((call) => call[0])).toEqual([
      "set_preferences",
      "get_status",
    ]);
  });
  it("lists native default and added folders once using canonical discovery paths", () => {
    const app = new Backend();
    app.status = status();
    app.status.snapshot.preferences.gameLibraryPaths = ["/added/library"];
    app.installed = {
      games: [],
      warnings: [],
      libraryPaths: ["/default/Steam", "/added/library", "/default/Steam"],
    };
    expect(app.libraryFolders).toEqual([
      { path: "/added/library", source: "Added by you" },
      { path: "/default/Steam", source: "Automatically detected" },
    ]);
    app.status.snapshot.preferences.gameLibraryPaths = ["/alias-to-steam"];
    app.installed.libraryPaths = ["/default/Steam"];
    expect(app.libraryFolders.map((folder) => folder.path)).toEqual([
      "/default/Steam",
    ]);
  });
  it("saves chosen games once and restores only the saved library on restart", async () => {
    const loaded = status();
    loaded.snapshot.games = ["cs2", "deadlock"].map(
      (id) => ({ id, name: id }) as Game,
    );
    native.invoke.mockImplementation(
      async (command: string, args?: { ids: string[] }) => {
        if (command === "set_library_games")
          loaded.snapshot.preferences.libraryGameIds = args!.ids;
        if (command === "get_status") return structuredClone(loaded);
        if (command === "get_languages") return { locales: [], warnings: [] };
        if (command === "verify_firewall") return emptyRead(loaded);
        if (command === "scan_games") return { games: [], warnings: [] };
      },
    );
    const app = new Backend();
    app.status = loaded;
    expect(await app.saveLibraryGames(["cs2", "cs2"])).toBe(true);
    expect(app.games.map((game) => game.id)).toEqual(["cs2"]);
    expect(native.invoke.mock.calls.map(([command]) => command)).toEqual([
      "set_library_games",
      "get_status",
    ]);
    expect(native.invoke).toHaveBeenCalledWith("set_library_games", {
      ids: ["cs2"],
    });
    const restarted = new Backend();
    await restarted.initialize();
    expect(restarted.games.map((game) => game.id)).toEqual(["cs2"]);
    expect(restarted.selected).toBe("cs2");
    expect(restarted.candidates[0].availability).toBe("Not found");
    native.invoke.mockRejectedValueOnce(new Error("Save failed"));
    expect(await restarted.saveLibraryGames([])).toBe(false);
    expect(restarted.games.map((game) => game.id)).toEqual(["cs2"]);
  });
  it("reloads authoritative state after errors and clears the busy guard", async () => {
    const app = new Backend();
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "apply_rules") throw new Error("Authorization cancelled");
      return status();
    });
    expect(await app.command("apply_rules")).toBe(false);
    expect(app.error).toContain("Authorization cancelled");
    expect(app.busy).toBe(false);
    expect(native.invoke).toHaveBeenCalledWith("get_status");
  });
  it("blocks repeated actions while an operation is running", async () => {
    const app = new Backend();
    let complete!: () => void;
    const first = app.run(
      () =>
        new Promise<void>((resolve) => {
          complete = resolve;
        }),
      false,
    );
    expect(await app.command("reset_rules")).toBe(false);
    expect(native.invoke).not.toHaveBeenCalled();
    complete();
    await first;
    expect(app.busy).toBe(false);
  });
  it("shows Apply progress, holds the guard while waiting, and only confirms completed operations", async () => {
    vi.useFakeTimers();
    try {
      const app = new Backend();
      const loaded = status();
      app.status = loaded;
      let complete!: () => void;
      native.invoke.mockImplementation(async (command: string) => {
        if (command === "apply_rules") {
          await new Promise<void>((resolve) => {
            complete = resolve;
          });
          loaded.appliedCount = 0;
          loaded.dirty = false;
        }
        if (command === "get_status") return loaded;
      });
      const applying = app.command("apply_rules");
      expect(app.firewallAction).toBe("apply");
      expect(app.firewallFeedback).toBeNull();
      expect(app.busy).toBe(true);
      await vi.advanceTimersByTimeAsync(30_000);
      expect(app.firewallSlow).toBe(true);
      expect(await app.command("apply_rules")).toBe(false);
      expect(await app.command("reset_rules")).toBe(false);
      expect(app.busy).toBe(true);
      expect(native.invoke.mock.calls.map(([command]) => command)).toEqual([
        "apply_rules",
      ]);
      complete();
      expect(await applying).toBe(true);
      expect(app.firewallFeedback).toBe("apply");
      expect(app.firewallAction).toBeNull();
      expect(app.firewallSlow).toBe(false);
      expect(app.busy).toBe(false);
      expect(vi.getTimerCount()).toBe(0);
      expect(native.invoke.mock.calls.map(([command]) => command)).toEqual([
        "apply_rules",
        "get_status",
      ]);
    } finally {
      vi.useRealTimers();
    }
  });
  it("reports authorization failures without optimistic success or a stuck busy state", async () => {
    const app = new Backend();
    const loaded = status();
    loaded.snapshot.preferences.blockedRegions = ["cs2:eu"];
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "apply_rules") throw Error("Authorization cancelled");
      if (command === "get_status") return loaded;
    });
    expect(await app.command("apply_rules")).toBe(false);
    expect(app.firewallFeedback).toBe("error");
    expect(app.firewallAction).toBeNull();
    expect(app.busy).toBe(false);
    expect(app.status?.snapshot.preferences.blockedRegions).toEqual(["cs2:eu"]);
    expect(app.status?.appliedCount).toBeNull();
    expect(app.error).toContain("Authorization cancelled");
    await app.command("scan_games");
    expect(app.firewallFeedback).toBeNull();
  });
  it("never confirms Apply when its authoritative state reload fails", async () => {
    const app = new Backend();
    app.status = status();
    app.status.appliedCount = 5;
    app.status.appliedGameIds = ["cs2"];
    app.status.appliedTargets = [
      {
        gameId: "cs2",
        gameName: "CS2",
        regionId: "eu",
        regionName: "Europe",
        endpoints: [],
      },
    ];
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "get_status") throw Error("Status unavailable");
    });
    expect(await app.command("apply_rules")).toBe(false);
    expect(app.firewallFeedback).toBe("error");
    expect(app.status.appliedCount).toBeNull();
    expect(app.status.appliedTargets).toBeNull();
    expect(app.status.appliedGameIds).toEqual(["cs2"]);
    expect(app.status.dirty).toBe(false);
    expect(app.busy).toBe(false);
    expect(app.firewallAction).toBeNull();
  });
  it("reports Reset progress and completion using the same serialized operation guard", async () => {
    const app = new Backend();
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "reset_rules") expect(app.firewallAction).toBe("reset");
      if (command === "get_status") {
        const cleared = status();
        cleared.appliedCount = 0;
        cleared.dirty = false;
        return cleared;
      }
    });
    expect(await app.command("reset_rules")).toBe(true);
    expect(app.firewallFeedback).toBe("reset");
    expect(app.firewallAction).toBeNull();
    expect(app.busy).toBe(false);
  });
  it("reports browser-only mode instead of creating a fake native backend", async () => {
    native.isTauri.mockReturnValue(false);
    const app = new Backend();
    await app.initialize();
    expect(app.error).toContain("pnpm dev");
    expect(native.invoke).not.toHaveBeenCalled();
  });
});

describe("background server data", () => {
  it("refreshes on each page open, deduplicates in-flight requests, and never blocks the UI", async () => {
    let finishRefresh!: () => void;
    const pending = new Promise<void>((resolve) => {
      finishRefresh = resolve;
    });
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "get_status") return status();
      if (command === "get_languages") return { locales: [], warnings: [] };
      if (command === "verify_firewall") return emptyRead();
      if (command === "scan_games") return { games: [], warnings: [] };
      if (command === "ensure_fresh_data") return pending;
      return undefined;
    });
    const app = new Backend();
    await app.initialize();

    const fresh = native.invoke.mock.calls.filter(
      (call) => call[0] === "ensure_fresh_data",
    );
    // Startup must not prefetch every game.
    expect(fresh).toHaveLength(0);

    const first = app.ensureFreshData("cs2");
    await app.ensureFreshData("cs2");
    await app.ensureFreshData("");
    expect(
      native.invoke.mock.calls.filter(
        (call) => call[0] === "ensure_fresh_data",
      ),
    ).toEqual([["ensure_fresh_data", { gameId: "cs2" }]]);
    finishRefresh();
    await first;
    await app.ensureFreshData("cs2");
    expect(
      native.invoke.mock.calls.filter(
        (call) => call[0] === "ensure_fresh_data",
      ),
    ).toHaveLength(2);
    // Never sets the shared busy flag, so Apply stays available.
    expect(app.busy).toBe(false);
    expect(app.error).toBe("");
  });

  it("stays silent when the refresh fails or runs outside the app", async () => {
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "ensure_fresh_data")
        throw new Error("network is unreachable");
      return undefined;
    });
    const app = new Backend();
    await expect(app.ensureFreshData("cs2")).resolves.toBeUndefined();
    await expect(app.ensureFreshData("cs2")).resolves.toBeUndefined();
    expect(app.error).toBe("");
    expect(app.busy).toBe(false);

    native.isTauri.mockReturnValue(false);
    const browser = new Backend();
    await browser.ensureFreshData("cs2");
    expect(
      native.invoke.mock.calls.filter(
        (call) => call[0] === "ensure_fresh_data",
      ),
    ).toHaveLength(2);
  });
});

describe("immediate firewall writes", () => {
  function blocked(): Status {
    const loaded = emptyRead();
    loaded.dirty = true;
    loaded.snapshot.preferences.blockedRegions = [
      "overwatch2:gcp-europe-north1",
    ];
    return loaded;
  }

  it("saves Windows scope changes without applying and rejects unknown scopes", async () => {
    const loaded = blocked();
    loaded.snapshot.platform = "windows";
    native.invoke.mockImplementation(
      async (command: string, args?: Record<string, unknown>) => {
        if (command === "get_status") return structuredClone(loaded);
        if (command === "set_preferences")
          loaded.snapshot.preferences = structuredClone(
            args?.preferences as Status["snapshot"]["preferences"],
          );
        if (command === "apply_rules") loaded.dirty = false;
      },
    );
    const app = new Backend();
    app.status = structuredClone(loaded);
    expect(await app.setBlockingScope("unknown")).toBe(false);
    expect(native.invoke).not.toHaveBeenCalled();
    expect(await app.setBlockingScope("systemWide")).toBe(true);
    expect(app.status?.snapshot.preferences.windowsBlockingScope).toBe(
      "systemWide",
    );
    expect(app.status?.snapshot.preferences.blockedRegions).toEqual([
      "overwatch2:gcp-europe-north1",
    ]);
    expect(native.invoke.mock.calls.map(([command]) => command)).toEqual([
      "set_preferences",
      "get_status",
    ]);
    expect(await app.apply()).toBe(true);
    expect(native.invoke).toHaveBeenCalledWith("apply_rules", undefined);
    native.invoke.mockClear();
    app.status!.snapshot.platform = "linux";
    expect(await app.setBlockingScope("executable")).toBe(false);
    expect(native.invoke).not.toHaveBeenCalled();
  });

  it("writes Windows all-apps blocks without choosing any executable", async () => {
    const loaded = blocked();
    loaded.snapshot.platform = "windows";
    loaded.snapshot.preferences.windowsBlockingScope = "systemWide";
    loaded.snapshot.games = [
      {
        schemaVersion: 1,
        id: "overwatch2",
        name: "Overwatch 2",
        source: null,
        updatedAt: null,
        note: "",
        regions: [
          {
            id: "gcp-europe-north1",
            name: "Europe",
            area: "Europe",
            probeTarget: null,
            endpoints: [
              {
                id: "server",
                address: "203.0.113.8",
                protocol: "udp",
                ports: null,
                custom: false,
              },
            ],
          },
        ],
      },
    ];
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "get_status") return structuredClone(loaded);
      if (command === "apply_rules") loaded.dirty = false;
    });
    const app = new Backend();
    app.status = structuredClone(loaded);
    await app.preferences((preferences) =>
      preferences.blockedRegions.push("cs2:eu"),
    );
    await vi.waitFor(() =>
      expect(native.invoke).toHaveBeenCalledWith("apply_rules", undefined),
    );
    await vi.waitFor(() => expect(app.busy).toBe(false));
    expect(app.status?.snapshot.games[0].executablePath).toBeUndefined();
    expect(
      native.invoke.mock.calls.some(([command]) =>
        ["select_game_executable", "set_game_executable"].includes(command),
      ),
    ).toBe(false);
  });

  it("writes as soon as a toggle is saved, with no waiting", async () => {
    const loaded = blocked();
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "get_status") return loaded;
      if (command === "verify_firewall") return emptyRead();
      if (command === "apply_rules") {
        loaded.dirty = false;
        return undefined;
      }
      return undefined;
    });
    const app = new Backend();
    app.status = loaded;

    await app.preferences((p) =>
      p.blockedRegions.push("deadlock:gcp-europe-north1"),
    );
    await vi.waitFor(() =>
      expect(
        native.invoke.mock.calls.filter((call) => call[0] === "apply_rules"),
      ).toHaveLength(1),
    );
    // The write runs to completion before the guard is released.
    await vi.waitFor(() => expect(app.busy).toBe(false));
    expect(loaded.dirty).toBe(false);
    expect(app.error).toBe("");
  });

  it("puts the selection back when the administrator prompt is rejected", async () => {
    const saved: string[][] = [];
    const loaded = blocked();
    native.invoke.mockImplementation(
      async (command: string, args?: Record<string, unknown>) => {
        if (command === "get_status") return loaded;
        if (command === "verify_firewall") return emptyRead();
        if (command === "set_preferences") {
          saved.push([
            ...(args?.preferences as { blockedRegions: string[] })
              .blockedRegions,
          ]);
          loaded.dirty = true;
          loaded.snapshot.preferences.blockedRegions = [
            ...(args?.preferences as { blockedRegions: string[] })
              .blockedRegions,
          ];
          return undefined;
        }
        if (command === "apply_rules") {
          // The user declines: nothing was written to the firewall.
          throw new Error("Firewall operation failed or UAC was cancelled");
        }
        return undefined;
      },
    );
    const app = new Backend();
    app.status = loaded;

    // Was blocked, user switches it off, the prompt is rejected.
    await app.preferences((p) => (p.blockedRegions = []));
    await vi.waitFor(() => expect(saved).toHaveLength(2));

    expect(saved[0]).toEqual([]);
    expect(saved[1]).toEqual(["overwatch2:gcp-europe-north1"]);
    await vi.waitFor(() =>
      expect(app.status?.snapshot.preferences.blockedRegions).toEqual([
        "overwatch2:gcp-europe-north1",
      ]),
    );
    // The switch reads from the saved selection, so it goes back on by itself.
    expect(app.busy).toBe(false);
  });

  it("writes nothing when the change did not touch blocking selections", async () => {
    const loaded = blocked();
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "get_status") return loaded;
      return undefined;
    });
    const app = new Backend();
    app.status = loaded;

    await app.preferences((p) => (p.language = "ar"));
    await vi.waitFor(() =>
      expect(native.invoke.mock.calls.length).toBeGreaterThan(0),
    );
    expect(
      native.invoke.mock.calls.filter((call) => call[0] === "apply_rules"),
    ).toHaveLength(0);
  });

  it("never writes for a game that still needs an executable", async () => {
    const loaded = blocked();
    loaded.snapshot.platform = "windows";
    loaded.snapshot.games = [
      {
        schemaVersion: 1,
        id: "overwatch2",
        name: "Overwatch 2",
        source: null,
        updatedAt: null,
        note: "",
        regions: [
          {
            id: "gcp-europe-north1",
            name: "Europe",
            area: "Europe",
            probeTarget: null,
            endpoints: [
              {
                id: "e1",
                address: "203.0.113.8",
                protocol: "udp",
                ports: null,
                custom: false,
              },
            ],
          },
        ],
      },
    ];
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "get_status") return loaded;
      if (command === "apply_rules") return undefined;
      return undefined;
    });
    const app = new Backend();
    app.status = loaded;
    await app.preferences((p) =>
      p.blockedRegions.push("cs2:gcp-europe-north1"),
    );
    await vi.waitFor(() =>
      expect(
        native.invoke.mock.calls.filter(
          (call) => call[0] === "set_preferences",
        ),
      ).toHaveLength(1),
    );
    expect(
      native.invoke.mock.calls.filter((call) => call[0] === "apply_rules"),
    ).toHaveLength(0);
    // The selection stays pending rather than being silently written.
    expect(loaded.dirty).toBe(true);
  });

  it("does not write while another operation holds the guard", async () => {
    const loaded = blocked();
    let release!: () => void;
    native.invoke.mockImplementation(async (command: string) => {
      if (command === "get_status") return loaded;
      if (command === "verify_firewall")
        await new Promise<void>((resolve) => {
          release = resolve;
        });
      if (command === "apply_rules") return undefined;
      return undefined;
    });
    const app = new Backend();
    app.status = loaded;
    const reading = app.command("verify_firewall", { elevated: false });
    await vi.waitFor(() => expect(app.busy).toBe(true));

    // Switches are disabled while busy, so a toggle cannot land mid-operation.
    await expect(
      app.preferences((p) => p.blockedRegions.push("cs2:gcp-europe-north1")),
    ).resolves.toBe(false);
    release();
    await reading;
    expect(
      native.invoke.mock.calls.filter((call) => call[0] === "apply_rules"),
    ).toHaveLength(0);
  });
});

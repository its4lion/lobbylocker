import { describe, expect, it } from "vitest";
import {
  areaPingSamples,
  pingRegions,
  pingRequest,
  serverPingSample,
  pingDescription,
  setProbeTarget,
} from "./ping";
import type { Game, PingSample, Status } from "./types";

const game: Game = {
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
    { id: "c", name: "C", area: "Asia", probeTarget: null, endpoints: [] },
  ],
};

describe("manual ping selection", () => {
  it("preserves probe edits and explicit disabling without changing block targets", () => {
    const edited = setProbeTarget(game, "a", " 203.0.113.9 ");
    expect(edited.regions[0].probeTarget).toBe("203.0.113.9");
    expect(edited.regions[0].probeCustom).toBe(true);
    expect(edited.regions[0].endpoints).toEqual(game.regions[0].endpoints);
    expect(edited.regions[1]).toEqual(game.regions[1]);
    expect(game.regions[0].probeTarget).toBe("203.0.113.1");

    const disabled = setProbeTarget(edited, "a", " ");
    expect(disabled.regions[0].probeTarget).toBeNull();
    expect(disabled.regions[0].probeCustom).toBe(true);
    expect(pingRequest(disabled, "a")).toBeUndefined();
    expect(setProbeTarget(game, "missing", "203.0.113.9")).toEqual(game);
  });

  it("individual cards show only their current configured probe's sample", () => {
    const sample: PingSample = {
      regionKey: "example:a",
      target: "203.0.113.1",
      latencyMs: 42.6,
      error: null,
      measuredAt: 1,
    };
    const status: Status = {
      snapshot: {
        games: [game],
        configDir: "test",
        platform: "linux",
        preferences: {
          schemaVersion: 1,
          blockedRegions: [],
          gameLibraryPaths: [],
          excludedGameLibraryPaths: [],
          libraryGameIds: [game.id],
          lastAppliedGameIds: [],
          language: "en",
        },
      },
      samples: { "example:a": sample },
      histories: {},
      appliedTargets: null,
      appliedCount: null,
      appliedGameIds: [],
      dirty: false,
      message: "Loaded",
    };
    expect(serverPingSample(game, game.regions[0], status)).toBe(sample);
    expect(serverPingSample(game, game.regions[1], status)).toBeUndefined();
    expect(serverPingSample(game, game.regions[0], null)).toBeUndefined();
    expect(
      serverPingSample(
        game,
        { ...game.regions[0], probeTarget: "203.0.113.9" },
        status,
      ),
    ).toBeUndefined();
    expect(
      serverPingSample(game, { ...game.regions[0], probeTarget: null }, status),
    ).toBeUndefined();
    expect(pingDescription(sample)).toBe("43 ms");
    expect(pingDescription({ ...sample, latencyMs: null })).toBe("No reply");
    // Nothing is claimed before the user runs a check, so there is no
    // "not checked" or "no ping target" placeholder to render.
    expect(serverPingSample(game, game.regions[2], status)).toBeUndefined();
  });

  it("offers only locations with a configured probe, regardless of block targets", () => {
    expect(pingRegions(game).map((region) => region.id)).toEqual(["a", "b"]);
    expect(pingRegions(undefined)).toEqual([]);
  });

  it("checks all configured servers of only the selected game", () => {
    expect(pingRequest(game)).toEqual({ gameId: "example" });
  });

  it("checks exactly one location, not every location in its region", () => {
    expect(pingRequest(game, "b")).toEqual({
      gameId: "example",
      regionId: "b",
    });
    expect(pingRegions(game, "b").map((region) => region.id)).toEqual(["b"]);
  });

  it("rejects missing games, unknown locations, and locations without probes", () => {
    expect(pingRequest(undefined)).toBeUndefined();
    expect(pingRequest(game, "not-found")).toBeUndefined();
    expect(pingRequest(game, "c")).toBeUndefined();
    expect(pingRequest({ ...game, regions: [] })).toBeUndefined();
  });

  it("renders only real samples for the card's game, area, and current probe target", () => {
    const samples: Status["samples"] = {
      "example:a": {
        regionKey: "example:a",
        target: "203.0.113.1",
        latencyMs: 42,
        error: null,
        measuredAt: 1,
      },
      "example:b": {
        regionKey: "example:b",
        target: "203.0.113.2",
        latencyMs: null,
        error: "No ICMP reply",
        measuredAt: 1,
      },
      "other:a": {
        regionKey: "other:a",
        target: "203.0.113.1",
        latencyMs: 1,
        error: null,
        measuredAt: 1,
      },
    };
    const status = { samples } as Status;
    expect(
      areaPingSamples(game, "Europe", status).map(({ region, sample }) => [
        region.id,
        sample.latencyMs,
      ]),
    ).toEqual([
      ["a", 42],
      ["b", null],
    ]);
    expect(areaPingSamples(game, "Asia", status)).toEqual([]);
    expect(areaPingSamples(game, "Europe", null)).toEqual([]);
    samples["example:a"].target = "203.0.113.9";
    expect(
      areaPingSamples(game, "Europe", status).map(({ region }) => region.id),
    ).toEqual(["b"]);
  });
});

import { describe, expect, it } from "vitest";
import {
  filterTargets,
  executableBlocking,
  savedTargets,
  missingPrograms,
  scopeWarning,
  verificationIssueMessage,
  verificationIssueRetryable,
} from "./firewall";
import type { Game, Status } from "./types";

const endpoint = {
  id: "one",
  address: "203.0.113.7",
  protocol: "udp" as const,
  ports: "27015-27030",
  custom: true,
};
function status(): Status {
  return {
    snapshot: {
      preferences: {
        blockedRegions: [
          "one:eu",
          "two:eu",
          "empty:eu",
          "unknown:eu",
          "one:eu",
        ],
      },
      games: [
        {
          id: "one",
          name: "One",
          regions: [{ id: "eu", name: "Europe", endpoints: [endpoint] }],
        },
        {
          id: "two",
          name: "Two",
          regions: [{ id: "eu", name: "Europe", endpoints: [endpoint] }],
        },
        {
          id: "empty",
          name: "Empty",
          regions: [{ id: "eu", name: "Europe", endpoints: [] }],
        },
      ] as Game[],
    },
    appliedCount: null,
    appliedTargets: null,
  } as Status;
}

describe("firewall review", () => {
  it("allows explicit all-apps Windows blocking without a game executable", () => {
    const app = status();
    app.snapshot.platform = "windows";
    expect(executableBlocking(app)).toBe(true);
    expect(missingPrograms(app)).toHaveLength(2);
    app.snapshot.games[0].executablePath = "C:\\Games\\One\\game.exe";
    app.appliedTargets = structuredClone(savedTargets(app));
    app.snapshot.preferences.windowsBlockingScope = "systemWide";
    expect(executableBlocking(app)).toBe(false);
    expect(missingPrograms(app)).toEqual([]);
    expect(
      savedTargets(app).every((target) => target.executablePath === null),
    ).toBe(true);
    expect(scopeWarning(app)).toContain("system-wide");
    // Changing scope previews new rules; it cannot rewrite OS readback or paths.
    expect(app.appliedTargets[0].executablePath).toBe(
      "C:\\Games\\One\\game.exe",
    );
    app.snapshot.preferences.windowsBlockingScope = "executable";
    expect(missingPrograms(app).map((target) => target.gameId)).toEqual([
      "two",
    ]);
    app.snapshot.platform = "linux";
    expect(executableBlocking(app)).toBe(false);
    expect(executableBlocking(null)).toBe(false);
  });
  it("requires an executable only for selected nonempty Windows games, without a system-wide fallback", () => {
    const app = status();
    app.snapshot.platform = "windows";
    expect(missingPrograms(app).map((target) => target.gameId)).toEqual([
      "one",
      "two",
    ]);
    app.snapshot.games[0].executablePath = "C:\\Games\\One\\game.exe";
    expect(missingPrograms(app).map((target) => target.gameId)).toEqual([
      "two",
    ]);
    expect(savedTargets(app)[0].executablePath).toBe(
      app.snapshot.games[0].executablePath,
    );
    expect(filterTargets(savedTargets(app), "One\\game.exe")).toHaveLength(1);
    expect(scopeWarning(app)).toContain(
      "only to each selected game executable",
    );
    app.snapshot.platform = "linux";
    expect(missingPrograms(app)).toEqual([]);
    expect(savedTargets(app)[0].executablePath).toBeNull();
    expect(scopeWarning(app)).toContain("system-wide");
    expect(missingPrograms(null)).toEqual([]);
  });
  it("keeps the applied executable immutable after changing its saved path", () => {
    const app = status();
    app.snapshot.platform = "windows";
    app.snapshot.games[0].executablePath = "C:\\Games\\Old\\game.exe";
    app.appliedTargets = structuredClone(savedTargets(app));
    app.snapshot.games[0].executablePath = "D:\\Games\\New\\game.exe";
    expect(app.appliedTargets[0].executablePath).toBe(
      "C:\\Games\\Old\\game.exe",
    );
    expect(savedTargets(app)[0].executablePath).toBe(
      "D:\\Games\\New\\game.exe",
    );
    app.snapshot.preferences.blockedRegions = [];
    expect(missingPrograms(app)).toEqual([]);
  });
  it("shows all selected games including shared targets, ignoring unknown, duplicate, and empty regions", () => {
    const app = status();
    expect(savedTargets(app).map((target) => target.gameName)).toEqual([
      "One",
      "Two",
    ]);
    expect(app.appliedTargets).toBeNull();
    expect(savedTargets(null)).toEqual([]);
  });
  it("searches game, region, address, protocol, and ports without mutating snapshots", () => {
    const targets = savedTargets(status());
    for (const query of [" Europe ", "203.0.113.7", "UDP", "27030"])
      expect(filterTargets(targets, query)).toHaveLength(2);
    expect(filterTargets(targets, "One")).toHaveLength(1);
    expect(filterTargets(targets, "unknown")).toEqual([]);
    const withTwo = [
      {
        ...targets[0],
        endpoints: [
          endpoint,
          { ...endpoint, id: "two", address: "203.0.113.9" },
        ],
      },
    ];
    expect(filterTargets(withTwo, "203.0.113.9")[0].endpoints).toHaveLength(1);
    expect(withTwo[0].endpoints).toHaveLength(2);
  });
  it("keeps the applied snapshot separate from subsequent saved edits and removals", () => {
    const app = status();
    app.appliedTargets = structuredClone(savedTargets(app));
    app.appliedCount = 1;
    app.snapshot.games[0].regions[0].endpoints = [
      { ...endpoint, address: "203.0.113.9" },
    ];
    expect(app.appliedTargets[0].endpoints[0].address).toBe("203.0.113.7");
    expect(savedTargets(app)[0].endpoints[0].address).toBe("203.0.113.9");
    app.snapshot.preferences.blockedRegions = [];
    expect(savedTargets(app)).toEqual([]);
    expect(app.appliedTargets).toHaveLength(2);
  });
  it("explains each readback cause and only offers a retry that can help", () => {
    expect(verificationIssueMessage("managedByDeviceManagement")).toContain(
      "organization manages this device",
    );
    expect(verificationIssueMessage("firewallUnavailable")).toContain(
      "Windows Firewall is turned off",
    );
    expect(verificationIssueMessage("unsupportedFilters")).toContain(
      "cannot verify",
    );
    // An unknown cause keeps the generic explanation instead of a wrong one.
    expect(verificationIssueMessage(null)).toBeNull();
    expect(verificationIssueMessage(undefined)).toBeNull();
    // Retrying cannot change a policy set by the user's organization.
    expect(verificationIssueRetryable("managedByDeviceManagement")).toBe(false);
    expect(verificationIssueRetryable("firewallUnavailable")).toBe(true);
    expect(verificationIssueRetryable("unsupportedFilters")).toBe(true);
    expect(verificationIssueRetryable(null)).toBe(true);
  });
});

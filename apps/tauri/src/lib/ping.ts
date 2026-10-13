import type { Game, PingSample, Region, Status } from "./types";

export function setProbeTarget(
  game: Game,
  regionId: string,
  target: string,
): Game {
  const updated = structuredClone(game);
  const region = updated.regions.find((region) => region.id === regionId);
  if (region) {
    region.probeTarget = target.trim() || null;
    region.probeCustom = true;
  }
  return updated;
}

/** Only explicit configured probe addresses are eligible for manual ICMP checks. */
export function pingRegions(
  game: Game | undefined,
  regionId?: string,
): Region[] {
  return (game?.regions ?? []).filter(
    (region) => region.probeTarget && (!regionId || region.id === regionId),
  );
}

export function pingRequest(
  game: Game | undefined,
  regionId?: string,
): { gameId: string; regionId?: string } | undefined {
  if (!game || !pingRegions(game, regionId).length) return undefined;
  return regionId ? { gameId: game.id, regionId } : { gameId: game.id };
}

export function serverPingSample(
  game: Game,
  region: Region,
  status: Status | null,
): PingSample | undefined {
  if (!region.probeTarget) return undefined;
  const sample = status?.samples[`${game.id}:${region.id}`];
  return sample?.target === region.probeTarget ? sample : undefined;
}

/** Only a real measurement is ever shown; nothing is claimed before a check. */
export function pingDescription(sample: PingSample): string {
  return sample.latencyMs == null
    ? "No reply"
    : `${Math.round(sample.latencyMs)} ms`;
}

export function areaPingSamples(
  game: Game,
  area: string,
  status: Status | null,
): { region: Region; sample: PingSample }[] {
  return pingRegions(game).flatMap((region) => {
    const sample = serverPingSample(game, region, status);
    return region.area === area && sample ? [{ region, sample }] : [];
  });
}

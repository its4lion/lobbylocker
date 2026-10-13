// Fill every providerScope region in data/games/*.json with the current ranges
// from Google's public network feed, so a fresh install can block those regions
// before the first refresh. The same feed backs the in-app refresh, so these
// values match what that refresh would write.
import { readdir, readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const FEED = "https://www.gstatic.com/ipranges/cloud.json";
const gamesDir = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "data", "games");

// Mirrors core/src/catalogue.rs: only "Google Cloud" prefixes in a configured
// scope, deduplicated, in BTreeSet order, with positional endpoint ids.
function addressesFor(prefixes, scope) {
  const addresses = new Set();
  for (const prefix of prefixes) {
    if (prefix.scope !== scope || prefix.service !== "Google Cloud") continue;
    const ipv4 = prefix.ipv4Prefix;
    const ipv6 = prefix.ipv6Prefix;
    if (Boolean(ipv4) === Boolean(ipv6)) continue;
    addresses.add(ipv4 ?? ipv6);
  }
  return [...addresses].sort();
}

const response = await fetch(FEED);
if (!response.ok) throw new Error(`${FEED} returned ${response.status}`);
const { prefixes, creationTime } = await response.json();

for (const entry of await readdir(gamesDir)) {
  if (!entry.endsWith(".json")) continue;
  const path = join(gamesDir, entry);
  const game = JSON.parse(await readFile(path, "utf8"));
  let touched = false;

  for (const region of game.regions ?? []) {
    if (!region.providerScope) continue;
    const addresses = addressesFor(prefixes, region.providerScope);
    if (!addresses.length) throw new Error(`${region.providerScope} returned no ranges`);
    const custom = (region.endpoints ?? []).filter((endpoint) => endpoint.custom);
    // Endpoint ids must stay identical to the ones the in-app refresh writes,
    // otherwise a refresh would treat every bundled address as new.
    region.endpoints = [
      ...addresses.map((address, index) => ({
        id: `${region.id}-${index}`,
        address,
        protocol: "udp",
        ports: null,
        custom: false,
      })),
      ...custom,
    ];
    touched = true;
  }

  if (!touched) continue;
  game.updatedAt = creationTime;
  await writeFile(path, `${JSON.stringify(game, null, 2)}\n`);
  const total = game.regions.reduce((n, r) => n + r.endpoints.length, 0);
  console.log(`${entry}: ${game.regions.length} regions, ${total} addresses`);
}
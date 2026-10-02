// Model choices and per-model controls shown in Settings → Claude.
// The authoritative request rules live in src-tauri/src/ai/anthropic/caps.rs;
// this mirrors only what the UI needs to enable or explain controls.

export interface ModelOption {
  id: string;
  name: string;
  note: string;
}

export const DEFAULT_MODEL = "claude-opus-5-5";

export const MODEL_OPTIONS: readonly ModelOption[] = [
  { id: "claude-opus-5-5", name: "Claude Opus 5.5", note: "Default. Thoughtful and quick at the Quick depth." },
  { id: "claude-sonnet-5-5", name: "Claude Sonnet 5.5", note: "Faster and less expensive." },
  { id: "claude-haiku-4-5", name: "Claude Haiku 4.5", note: "Fastest and least expensive." },
  { id: "claude-fable-5-1", name: "Claude Fable 5.1", note: "Most capable; slower and pricier." },
];

type Family = "opus" | "sonnet" | "haiku" | "fable" | "mythos" | "unknown";

export function parseModel(id: string): { family: Family; version: [number, number] } {
  const tokens = id.trim().toLowerCase().replace(/^claude-/, "").split(/[-.@]/);
  const families: Family[] = ["opus", "sonnet", "haiku", "fable", "mythos"];
  const family = (tokens.find((t) => families.includes(t as Family)) as Family | undefined) ?? "unknown";
  const isVersion = (t: string) => /^\d{1,2}$/.test(t);
  const startsWithFamily = families.includes(tokens[0] as Family);
  const rest = startsWithFamily ? tokens.slice(1) : tokens;
  const numbers: number[] = [];
  for (const t of rest) {
    if (!isVersion(t)) break;
    numbers.push(Number(t));
  }
  return { family, version: [numbers[0] ?? 0, numbers[1] ?? 0] };
}

const atLeast = (v: [number, number], major: number, minor: number) =>
  v[0] > major || (v[0] === major && v[1] >= minor);

/** Whether the API accepts `temperature` for this model. */
export function supportsTemperature(id: string): boolean {
  const { family, version } = parseModel(id);
  if (family === "haiku") return true;
  if (family === "sonnet") return !atLeast(version, 5, 0);
  if (family === "opus") return !atLeast(version, 4, 7);
  return false;
}

/** Whether the API accepts the `effort` (response depth) control for this model. */
export function supportsEffort(id: string): boolean {
  const { family, version } = parseModel(id);
  if (family === "fable" || family === "mythos") return true;
  if (family === "opus") return atLeast(version, 4, 5);
  if (family === "sonnet") return atLeast(version, 4, 6);
  return false;
}

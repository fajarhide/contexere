// @ts-check
// Where the screens get their data: the installed `contexere` binary through the
// Tauri shell, or, in a plain browser, the fixtures generated from a real binary
// (`fixtures/make-fixtures.sh`). Nothing on screen is typed in by hand.

/** A failure the screens explain in words, not a stack trace. */
export class DataError extends Error {
  /**
   * @param {"missing" | "outdated" | "failed"} kind
   * @param {string} message
   */
  constructor(kind, message) {
    super(message);
    this.kind = kind;
  }
}

/** @typedef {{id: string, name: string, tier: string, present: boolean, ok: boolean, warnings: string[]}} Agent */
/** @typedef {{checks: {name: string, ok: boolean, message: string}[], agents: Agent[]}} Doctor */
/** @typedef {{label: string, commands: number, input_tokens: number, output_tokens: number, savings_pct: number, measurement_method: string}} Period */
/** @typedef {{command: string, count: number, savings_pct: number, bytes_saved: number}} CommandRow */
/** @typedef {{periods: Period[], commands: CommandRow[], agents: {agent: string, count: number, savings_pct: number}[]}} Stats */
/** @typedef {{timestamp: number, command: string, route: string, summary: string, input_bytes: number, output_bytes: number, handle?: string}} TimelineItem */

const params = new URLSearchParams(location.search);
// `?fixture=empty|active|missing|outdated|slow` picks a state in a browser.
const fixture = params.get("fixture") ?? "active";

/** @returns {boolean} */
function inTauri() {
  return "__TAURI_INTERNALS__" in window;
}

/**
 * Runs one allowed `contexere` call and returns its stdout.
 * @param {string[]} args
 * @returns {Promise<string>}
 */
async function run(args) {
  if (inTauri()) {
    // @ts-ignore: injected by the Tauri runtime
    const { invoke } = window.__TAURI_INTERNALS__;
    try {
      return await invoke("contexere", { args });
    } catch (e) {
      const text = String(e);
      if (text.startsWith("missing:")) throw new DataError("missing", text.slice(8));
      throw new DataError("failed", text);
    }
  }
  if (fixture === "missing") {
    throw new DataError("missing", "No contexere binary found.");
  }
  if (fixture === "slow") await new Promise((r) => setTimeout(r, 1500));
  const dir = fixture === "empty" ? "empty" : "active";
  const name =
    args[0] === "doctor" ? "doctor.json"
    : args[0] === "stats" ? "stats.json"
    : args[0] === "query" ? "timeline.json"
    : "retrieve.txt";
  const res = await fetch(`../fixtures/${dir}/${name}`);
  if (!res.ok) throw new DataError("failed", `fixture ${dir}/${name}: ${res.status}`);
  return res.text();
}

/**
 * @param {string[]} args
 * @returns {Promise<any>}
 */
async function runJson(args) {
  const out = await run(args);
  try {
    return JSON.parse(out);
  } catch {
    throw new DataError("failed", `contexere ${args[0]} did not print JSON`);
  }
}

/** @returns {Promise<Doctor>} */
export async function doctor() {
  const d = await runJson(["doctor", "--json"]);
  // `agents` arrived in 0.9.0 (#1177); an older binary has only the flat list.
  if (!Array.isArray(d.agents) || fixture === "outdated") {
    const version = d.checks?.find((/** @type {any} */ c) => c.name === "binary")?.message;
    throw new DataError("outdated", version ?? "an older contexere");
  }
  return d;
}

/** @returns {Promise<Stats>} */
export function stats() {
  return runJson(["stats", "--json"]);
}

/** @returns {Promise<TimelineItem[]>} */
export async function timeline() {
  const q = await runJson(["query", "timeline", "today", "--json"]);
  return Array.isArray(q.results) ? q.results : [];
}

/**
 * The original output behind a handle.
 * @param {string} handle
 * @returns {Promise<string>}
 */
export function original(handle) {
  return run(["retrieve", handle]);
}

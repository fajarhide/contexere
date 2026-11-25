// @ts-check
import { DataError, doctor, stats, timeline, original } from "./data.js";

/**
 * Builds an element. Text always goes in as text, never as markup.
 * @param {string} tag
 * @param {Record<string, string> | null} [attrs]
 * @param {...(Node | string | null | false)} children
 * @returns {HTMLElement}
 */
function el(tag, attrs, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs ?? {})) node.setAttribute(k, v);
  for (const c of children) if (c) node.append(c);
  return node;
}

/**
 * A sentence from the CLI with its `backticked` parts set as code.
 * @param {string} text
 */
function prose(text) {
  const frag = document.createDocumentFragment();
  text.split(/(`[^`]+`)/).forEach((part) => {
    if (part.startsWith("`") && part.endsWith("`") && part.length > 2) {
      frag.append(el("code", null, part.slice(1, -1)));
    } else if (part) {
      frag.append(part);
    }
  });
  return frag;
}

/** @param {number} n */
const count = (n) => n.toLocaleString("en-US");

/** The share of the original that reached the agent, as the motif bar. */
function keptBar(/** @type {number} */ share, /** @type {string} */ label) {
  const pct = Math.max(0, Math.min(1, share)) * 100;
  const fill = el("span");
  fill.style.width = `${pct}%`;
  return el("div", { class: "kept", role: "img", "aria-label": label }, fill);
}

/** @param {HTMLElement} panel @param {string} what */
function showLoading(panel, what) {
  panel.replaceChildren(
    el("div", { class: "state", "aria-busy": "true", role: "status" }, el("h3", null, what)),
  );
}

/** @param {HTMLElement} panel @param {unknown} err */
function showError(panel, err) {
  const e = err instanceof DataError ? err : new DataError("failed", String(err));
  /** @type {[string, Node]} */
  const [title, body] =
    e.kind === "missing"
      ? [
          "This app can't find Contexere on this computer",
          prose(
            "Install it with `brew install fajarhide/tap/contexere`, or `curl -fsSL omni.weekndlabs.com/install | bash`, then check again.",
          ),
        ]
      : e.kind === "outdated"
        ? [
            "This app needs Contexere 0.9.0 or newer",
            prose(`Found ${e.message}. Run \`contexere update\`, then check again.`),
          ]
        : ["Contexere answered with an error", prose(e.message)];
  panel.replaceChildren(
    el("div", { class: "state failed", role: "alert" }, el("h3", null, title), el("p", null, body)),
  );
}

/** What each tier lets Contexere do, in a sentence a user can act on. */
const TIER = {
  Full: "Shortens command output before the agent reads it",
  "Handoff-first": "Shortens output when the agent runs commands through Contexere",
};
const tierText = (/** @type {string} */ t) =>
  /** @type {Record<string, string>} */ (TIER)[t] ?? "Memory and search through MCP";

/** @param {HTMLElement} panel */
async function renderAgents(panel) {
  showLoading(panel, "Checking your agents");
  const d = await doctor();
  const here = d.agents.filter((a) => a.present);
  const working = here.filter((a) => a.ok);
  const attention = here.filter((a) => !a.ok);
  const away = d.agents.filter((a) => !a.present);

  if (here.length === 0) {
    panel.replaceChildren(
      el("div", { class: "state" },
        el("h3", null, "No agent on this computer uses Contexere yet"),
        el("p", null, prose("Install an agent such as Claude Code or Codex, then run `contexere init` to connect it.")),
      ),
    );
    return;
  }

  const lead = el("p", { class: "lead" });
  lead.append(el("strong", null, `${working.length} of ${here.length}`), ` agents here run Contexere.`);
  const sub = attention.length
    ? el("p", { class: "muted" }, `${attention.length === 1 ? "One needs" : `${attention.length} need`} a look before it can.`)
    : el("p", { class: "muted" }, "Nothing needs your attention.");

  const nodes = [lead, sub];
  if (attention.length) {
    nodes.push(
      el("h2", null, "Needs a look"),
      el("div", { class: "attention" },
        ...attention.map((a) =>
          el("article", { class: "card" },
            el("h3", null, a.name),
            el("p", { class: "muted" }, tierText(a.tier)),
            el("ul", null, ...a.warnings.map((w) => el("li", null, prose(w)))),
          ),
        ),
      ),
    );
  }
  if (working.length) {
    nodes.push(
      el("h2", null, "Working"),
      el("ul", { class: "rows" },
        ...working.map((a) =>
          el("li", null, el("span", { class: "name" }, a.name), el("span", { class: "muted" }, tierText(a.tier))),
        ),
      ),
    );
  }
  if (away.length) {
    nodes.push(
      el("details", null,
        el("summary", null, `Not on this computer (${away.length})`),
        el("p", { class: "muted" }, away.map((a) => a.name).join(", ")),
        el("p", { class: "muted" }, prose("Install one of these and run `contexere init` to connect it.")),
      ),
    );
  }
  panel.replaceChildren(...nodes);
}

const PERIOD_NAMES = { today: "Today", this_week: "This week", all_time: "All time" };
const PERIOD_PHRASE = { today: "today", this_week: "this week", all_time: "since you installed it" };
let period = "this_week";

/** @param {HTMLElement} panel */
async function renderSavings(panel) {
  showLoading(panel, "Adding up what Contexere saved");
  const s = await stats();
  const all = s.periods.find((p) => p.label === "all_time");
  if (!all || all.commands === 0) {
    panel.replaceChildren(
      el("div", { class: "state" },
        el("h3", null, "Nothing measured yet"),
        el("p", null, "Savings show up here after your agent runs its first command through Contexere."),
      ),
    );
    return;
  }

  const draw = () => {
    const p = s.periods.find((x) => x.label === period) ?? all;
    const saved = Math.max(0, p.input_tokens - p.output_tokens);
    const phrase = /** @type {Record<string, string>} */ (PERIOD_PHRASE)[p.label] ?? "";

    const buttons = el("div", { class: "periods", role: "group", "aria-label": "Period" },
      ...s.periods.map((x) => {
        const b = el("button", { class: "quiet", type: "button", "aria-pressed": String(x.label === p.label) },
          /** @type {Record<string, string>} */ (PERIOD_NAMES)[x.label] ?? x.label);
        b.addEventListener("click", () => { period = x.label; draw(); /** @type {HTMLElement | null} */ (panel.querySelector(`[aria-pressed="true"]`))?.focus(); });
        return b;
      }),
    );

    const lead = el("p", { class: "lead" });
    if (p.commands === 0) {
      lead.append(`No commands ${phrase}.`);
    } else {
      lead.append(el("strong", null, `${count(saved)} tokens`), ` kept out of your agents' context ${phrase}.`);
    }
    const note = el("p", { class: "muted figure-note" },
      p.commands === 0
        ? "Pick a longer period to see earlier savings."
        : `${count(p.commands)} commands, ${Math.round(p.savings_pct)}% less to read than the raw output. ${p.measurement_method === "estimated" ? "Token counts are estimated." : ""}`,
    );

    const rows = [...s.commands].sort((a, b) => b.bytes_saved - a.bytes_saved);
    const table = el("div", { class: "table-wrap" },
      el("table", null,
        el("thead", null, el("tr", null,
          el("th", { scope: "col" }, "Command"),
          el("th", { scope: "col", class: "num" }, "Runs"),
          el("th", { scope: "col", class: "num" }, "Shorter by"),
          el("th", { scope: "col", class: "bar" }, el("span", { class: "muted" }, "What reached the agent")),
        )),
        el("tbody", null, ...rows.map((c) =>
          el("tr", null,
            el("td", { class: "mono" }, c.command),
            el("td", { class: "num" }, count(c.count)),
            el("td", { class: "num" }, c.savings_pct > 0 ? `${Math.round(c.savings_pct)}%` : "passed through"),
            el("td", { class: "bar" }, keptBar(1 - c.savings_pct / 100, `${Math.round(100 - c.savings_pct)}% of the output reached the agent`)),
          ),
        )),
      ),
    );

    const nodes = [buttons, lead, note, el("h2", null, "By command, all time"), table];
    if (s.agents.length) {
      nodes.push(
        el("h2", null, "By agent, all time"),
        el("ul", { class: "rows" }, ...s.agents.map((a) =>
          el("li", null, el("span", { class: "name" }, a.agent),
            el("span", { class: "muted" }, `${count(a.count)} commands, ${Math.round(a.savings_pct)}% shorter`)),
        )),
      );
    }
    panel.replaceChildren(...nodes);
  };
  draw();
}

const dialog = /** @type {HTMLDialogElement} */ (document.getElementById("original"));
dialog.querySelector("[data-close]")?.addEventListener("click", () => dialog.close());

/** @param {import("./data.js").TimelineItem} item @param {HTMLElement} opener */
async function showOriginal(item, opener) {
  const body = /** @type {HTMLElement} */ (document.getElementById("original-body"));
  /** @type {HTMLElement} */ (document.getElementById("original-cmd")).replaceChildren(
    prose(`\`${item.command}\`, ${count(item.input_bytes)} bytes as printed`),
  );
  body.textContent = "Loading the original";
  dialog.showModal();
  dialog.addEventListener("close", () => opener.focus(), { once: true });
  try {
    body.textContent = await original(/** @type {string} */ (item.handle));
  } catch (err) {
    body.textContent = err instanceof Error ? `Could not load it: ${err.message}` : "Could not load it.";
  }
}

/** @param {HTMLElement} panel */
async function renderActivity(panel) {
  showLoading(panel, "Reading today's activity");
  const items = (await timeline()).slice().reverse();
  if (items.length === 0) {
    panel.replaceChildren(
      el("div", { class: "state" },
        el("h3", null, "Nothing yet today"),
        el("p", null, "Commands your agents run show up here as Contexere handles them."),
      ),
    );
    return;
  }
  const shortened = items.filter((i) => i.output_bytes < i.input_bytes).length;
  const lead = el("p", { class: "lead" });
  lead.append(el("strong", null, `${shortened} of ${items.length}`), " commands today came back shorter.");
  const list = el("ul", { class: "events" },
    ...items.map((i) => {
      const t = new Date(i.timestamp * 1000);
      const time = el("time", { datetime: t.toISOString() },
        t.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }));
      const what = el("div", { class: "what" },
        el("div", { class: "cmd" }, i.command),
        el("div", { class: "muted" }, i.summary),
      );
      const li = el("li", null, time, what);
      if (i.input_bytes > 0) {
        li.append(keptBar(i.output_bytes / i.input_bytes, `${count(i.output_bytes)} of ${count(i.input_bytes)} bytes reached the agent`));
      }
      if (i.handle) {
        const b = el("button", { class: "quiet", type: "button" }, "Show original");
        b.setAttribute("aria-label", `Show the original output of ${i.command}`);
        b.addEventListener("click", () => showOriginal(i, b));
        li.append(b);
      }
      return li;
    }),
  );
  panel.replaceChildren(lead, el("p", { class: "muted" }, "Newest first. Each bar is what reached the agent, inside what the command printed."), list);
}

const screens = {
  agents: renderAgents,
  savings: renderSavings,
  activity: renderActivity,
};
/** @type {Set<string>} */
const loaded = new Set();
const tabs = /** @type {HTMLButtonElement[]} */ ([...document.querySelectorAll('[role="tab"]')]);

/** @param {string} name */
async function load(name) {
  const panel = /** @type {HTMLElement} */ (document.getElementById(`panel-${name}`));
  loaded.add(name);
  try {
    await /** @type {Record<string, (p: HTMLElement) => Promise<void>>} */ (screens)[name](panel);
  } catch (err) {
    loaded.delete(name);
    showError(panel, err);
  }
}

/** @param {HTMLButtonElement} tab @param {boolean} focus */
function select(tab, focus) {
  for (const t of tabs) {
    const on = t === tab;
    t.setAttribute("aria-selected", String(on));
    t.tabIndex = on ? 0 : -1;
    /** @type {HTMLElement} */ (document.getElementById(/** @type {string} */ (t.getAttribute("aria-controls")))).hidden = !on;
  }
  if (focus) tab.focus();
  const name = tab.id.replace("tab-", "");
  if (!loaded.has(name)) load(name);
}

tabs.forEach((tab, i) => {
  tab.addEventListener("click", () => select(tab, false));
  tab.addEventListener("keydown", (e) => {
    const next =
      e.key === "ArrowRight" ? tabs[(i + 1) % tabs.length]
      : e.key === "ArrowLeft" ? tabs[(i - 1 + tabs.length) % tabs.length]
      : e.key === "Home" ? tabs[0]
      : e.key === "End" ? tabs[tabs.length - 1]
      : null;
    if (next) {
      e.preventDefault();
      select(next, true);
    }
  });
});

document.getElementById("refresh")?.addEventListener("click", () => {
  loaded.clear();
  const current = tabs.find((t) => t.getAttribute("aria-selected") === "true") ?? tabs[0];
  load(current.id.replace("tab-", ""));
});

load("agents");

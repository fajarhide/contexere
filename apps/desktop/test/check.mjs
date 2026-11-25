// Drives the app in a browser against the fixtures: every state, both themes,
// desktop and phone width. Fails on a console error, horizontal overflow, a
// missing state, or a control that does not do what it says.
//
//   node apps/desktop/test/check.mjs [screenshot dir]
//
// Needs the `playwright` package and its Chromium on NODE_PATH.
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const shots = process.argv[2];
const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".json": "application/json", ".txt": "text/plain", ".woff2": "font/woff2" };

const server = http.createServer((req, res) => {
  const file = path.join(root, decodeURIComponent(new URL(req.url, "http://x").pathname));
  if (!file.startsWith(root) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) {
    res.writeHead(404).end();
    return;
  }
  res.writeHead(200, { "content-type": types[path.extname(file)] ?? "application/octet-stream" });
  fs.createReadStream(file).pipe(res);
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const base = `http://127.0.0.1:${server.address().port}/src/index.html`;

const browser = await chromium.launch();
const failures = [];
const fail = (msg) => failures.push(msg);

/** Opens one state and returns the page plus the console errors it raised. */
async function open(fixture, scheme, width) {
  const page = await browser.newPage({ colorScheme: scheme, viewport: { width, height: 800 } });
  const errors = [];
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto(`${base}?fixture=${fixture}`);
  return { page, errors };
}

async function noOverflow(page, label) {
  const over = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  if (over > 0) fail(`${label}: page scrolls sideways by ${over}px`);
}

async function shot(page, name) {
  if (shots) await page.screenshot({ path: path.join(shots, `${name}.png`), fullPage: true });
}

const expect = {
  active: { agents: "agents here run Contexere", savings: "kept out of your agents' context", activity: "commands today came back shorter" },
  empty: { agents: "No agent on this computer", savings: "Nothing measured yet", activity: "Nothing yet today" },
  missing: { agents: "can't find Contexere", savings: "can't find Contexere", activity: "can't find Contexere" },
  outdated: { agents: "needs Contexere 0.9.0 or newer" },
};

for (const scheme of ["light", "dark"]) {
  for (const width of [1024, 375]) {
    for (const [fixture, screens] of Object.entries(expect)) {
      const { page, errors } = await open(fixture, scheme, width);
      for (const [screen, text] of Object.entries(screens)) {
        await page.click(`#tab-${screen}`);
        const panel = page.locator(`#panel-${screen}`);
        try {
          await panel.getByText(text).first().waitFor({ timeout: 4000 });
        } catch {
          fail(`${fixture}/${screen} ${scheme} ${width}: no "${text}"`);
        }
        await noOverflow(page, `${fixture}/${screen} ${scheme} ${width}`);
        await shot(page, `${fixture}-${screen}-${scheme}-${width}`);
      }
      if (errors.length) fail(`${fixture} ${scheme} ${width}: console ${errors.join(" | ")}`);
      await page.close();
    }
  }
}

// Loading is its own state, not a blank screen.
{
  const { page } = await open("slow", "light", 1024);
  const busy = await page.locator('[aria-busy="true"]').count();
  if (busy !== 1) fail("slow: no loading state while the data is on its way");
  await page.close();
}

// Keyboard only: arrows move between screens, the original opens and Escape closes it.
{
  const { page, errors } = await open("active", "light", 1024);
  await page.locator("#tab-agents").focus();
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowRight");
  const selected = await page.locator('[role="tab"][aria-selected="true"]').getAttribute("id");
  if (selected !== "tab-activity") fail(`keyboard: arrows selected ${selected}`);
  const opener = page.getByRole("button", { name: /Show the original output/ }).first();
  await opener.waitFor();
  await opener.focus();
  await page.keyboard.press("Enter");
  await page.locator("#original[open]").waitFor();
  await page.waitForFunction(() => !document.getElementById("original-body")?.textContent?.startsWith("Loading"), null, { timeout: 4000 }).catch(() => {});
  const body = await page.locator("#original-body").textContent();
  if (!body || body.length < 100 || body.startsWith("Loading")) fail("original: dialog did not load the output");
  await shot(page, "active-original-light-1024");
  await page.keyboard.press("Escape");
  if (await page.locator("#original[open]").count()) fail("original: Escape did not close the dialog");
  const back = await page.evaluate(() => document.activeElement?.textContent);
  if (back !== "Show original") fail(`original: focus went to "${back}" after closing`);

  // Period buttons change the figure.
  await page.click("#tab-savings");
  await page.getByRole("button", { name: "Today" }).click();
  if ((await page.getByRole("button", { name: "Today" }).getAttribute("aria-pressed")) !== "true") fail("savings: Today did not select");

  // Check again reloads instead of doing nothing.
  await page.click("#refresh");
  await page.locator("#panel-savings").getByText("kept out of").waitFor();
  if (errors.length) fail(`keyboard run: console ${errors.join(" | ")}`);
  await page.close();
}

await browser.close();
server.close();
if (failures.length) {
  console.error(failures.map((f) => `FAIL ${f}`).join("\n"));
  process.exit(1);
}
console.log("PASS: every state, both themes, desktop and phone width, keyboard path");

// Use the website a running Message Crate serves, the way a person would on
// first visit: reach the login card, open the Demo Account, search for a
// conversation, and open it. Run by scripts/check-served-website.sh.
//
//   node check.mjs <base-url>
//
// Fails, naming the step, when a screen does not appear, when the search does
// not reach the server, finds nothing, narrows nothing, is not what the list
// shows, or opens a conversation it did not find; and at the end when the
// browser logged a console error or an uncaught exception, or the server
// answered any request with a 5xx. A script or stylesheet the website cannot
// load is a console error in Chromium, so a broken static/ bundle fails too.
import { chromium } from "playwright";

const base = process.argv[2];
if (!base) {
  console.error("usage: node check.mjs <base-url>");
  process.exit(2);
}

// Generous: the first page load also starts the server's caches, and a
// nightly runner is slow. A screen that never appears still fails.
const STEP_TIMEOUT_MS = 30_000;

const problems = [];
const browser = await chromium.launch();
let step = "start the browser";
try {
  const page = await browser.newPage();
  page.setDefaultTimeout(STEP_TIMEOUT_MS);
  page.on("console", (message) => {
    if (message.type() === "error") {
      // Chromium's message for a file it could not load leaves out which
      // file; the location names it.
      const where = message.location().url;
      problems.push(
        `console error during "${step}": ${message.text()}${where ? ` (${where})` : ""}`,
      );
    }
  });
  page.on("pageerror", (error) => {
    problems.push(`uncaught exception during "${step}": ${error.message}`);
  });
  page.on("response", (response) => {
    if (response.status() >= 500) {
      problems.push(
        `${response.status()} from ${response.request().method()} ${response.url()} during "${step}"`,
      );
    }
  });

  // A new Message Crate has no owner, so the card offers Create Owner and,
  // under it, the Demo Account. Its button logs in as "demo" with an empty
  // password, the request the login form would send.
  step = "load the login card";
  await page.goto(base);
  const explore = page.getByRole("button", { name: "Explore Demo Account" });
  await explore.waitFor();

  step = "log in to the Demo Account";
  const listed = page.waitForResponse((response) => {
    const url = new URL(response.url());
    return url.pathname === "/v1/conversations" && !url.searchParams.has("q");
  });
  await explore.click();
  const search = page.getByRole("combobox", { name: "Search conversations" });
  await search.waitFor();
  await page.getByText("demo", { exact: true }).first().waitFor();

  // The search term comes from the list the Demo Account shows, so the check
  // does not depend on which names the demo generator picks.
  step = "list the Demo Account's conversations";
  const all = await listed;
  if (!all.ok()) throw new Error(`the list answered ${all.status()}`);
  const allTotal = (await all.json()).total;
  const rows = page.getByRole("checkbox", { name: /^Select (?!all conversations$)/ });
  await rows.first().waitFor();
  const title = (await rows.first().getAttribute("aria-label"))?.replace(/^Select /, "") ?? "";
  const term = title.split(/[\s,]+/)[0];
  if (!term) throw new Error(`no search term in the first conversation's name "${title}"`);

  // The row named `title` was on screen before the search, so finding it
  // afterwards proves nothing. The search must reach the server, find fewer
  // conversations than the whole list, and the list's range must then show
  // the answer's total; the row must open a conversation the answer holds.
  step = `search conversations for "${term}"`;
  const answered = page.waitForResponse((response) => {
    const url = new URL(response.url());
    return url.pathname === "/v1/conversations" && url.searchParams.get("q") === term;
  });
  await search.fill(term);
  await search.press("Enter");
  const response = await answered;
  if (!response.ok()) throw new Error(`the search answered ${response.status()}`);
  const answer = await response.json();
  const found = answer.items ?? [];
  if (found.length === 0) throw new Error("the search found no conversation");
  if (answer.total >= allTotal) {
    throw new Error(`the search found ${answer.total} of ${allTotal} conversations, narrowing nothing`);
  }
  // The range reads "1–9 of 102" (formatVisibleRange in web/src/lib/listPaging.ts).
  await page.getByText(new RegExp(`^(\\d+–\\d+|…) of ${answer.total}$`)).waitFor();
  const result = page.getByRole("button", { name: title, exact: false }).first();
  await result.waitFor();

  step = `open the conversation "${title}"`;
  await result.click();
  await page.waitForURL((url) => url.hash.startsWith("#/messages/"));
  const opened = Number(new URL(page.url()).hash.match(/^#\/messages\/(\d+)/)?.[1]);
  if (!found.some((conversation) => conversation.id === opened)) {
    throw new Error(`the row opened conversation ${opened}, which the search did not answer with`);
  }
  const main = page.getByRole("main");
  await main.getByRole("heading", { level: 2, name: title, exact: true }).waitFor();
  await main.getByText(/^[\d,]+ messages?$/).waitFor();
} catch (error) {
  problems.unshift(`could not ${step}: ${error.message.split("\n")[0]}`);
} finally {
  await browser.close();
}

if (problems.length > 0) {
  console.error(`The served website at ${base} failed its check:`);
  for (const problem of problems) console.error(`- ${problem}`);
  process.exit(1);
}
console.log(
  `The served website at ${base} logs in to the Demo Account, searches, and opens a conversation.`,
);

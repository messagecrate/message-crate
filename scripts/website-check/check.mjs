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

// The words the search reads as operators, in any case (word_or_operator in
// crates/server/server/src/search/lex.rs), so a name word that is one of
// them would not search for itself.
const OPERATORS = new Set(["and", "or", "not"]);

// A word of a shown name to search for, and the first name holding it.
// `conversations` are the items of the list answer's first page, the one
// the names on screen were drawn from. A bare word on the Conversations list
// matches, in any case, a conversation's title, its participants' names and
// identities (emit_text in crates/server/server/src/search/emit.rs), and the
// text of the chat handle the conversation is held under. So each
// conversation's text here is its shown title and its participants' names
// and identities, and a word is counted against that text, not against the
// names on screen.
//
// The list answer carries no chat handle, so its text is taken to be in the
// participants already: a one-to-one conversation's handle is the other
// person's identity, and a group's gives no text. Where that is wrong, the
// check fails and never passes wrongly. One known gap: a one-to-one
// conversation with no participant rows is shown under its handle's Contact
// name, which the search does not read (#2737). The demo's one such
// conversation is held under a number on no Contact (EMPTY_ONE_TO_ONE_HANDLE
// in crates/server/demo-seed/src/personas.rs), so it is shown under that
// number, which the search does read.
//
// The word picked is the one the fewest of those conversations hold, and at
// least one must not hold it, so the search finds fewer conversations than
// the list. A word no conversation holds is skipped, as the web app made it
// up: "(unknown)" names a conversation with no title and no participants
// (web/src/lib/conversationName.ts). Words are runs of letters and digits,
// so no punctuation reaches the search syntax, and the operators above are
// skipped; words under three letters are skipped too, as they sit inside too
// many names to narrow anything.
function narrowingTerm(names, conversations) {
  const texts = conversations.map((conversation) =>
    [
      conversation.shown_title ?? "",
      ...(conversation.participants ?? []).flatMap((p) => [p.name ?? "", p.identity ?? ""]),
    ]
      .join("\n")
      .toLowerCase(),
  );
  let best = null;
  for (const name of names) {
    for (const word of name.split(/[^\p{L}\p{N}]+/u)) {
      const lowered = word.toLowerCase();
      if (word.length < 3 || OPERATORS.has(lowered)) continue;
      const holders = texts.filter((text) => text.includes(lowered)).length;
      if (holders === 0) continue;
      if (best === null || holders < best.holders) {
        best = { term: word, title: name, holders };
      }
    }
  }
  if (best === null || best.holders === texts.length) {
    throw new Error(
      `no word of a shown name is missing from one of the ${texts.length} conversations on the list's first page (titles, participant names and identities), so no search could narrow the list: ${JSON.stringify(names)}`,
    );
  }
  return best;
}

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

  // A new Message Crate is in setup, so the card offers Set Up and, under
  // it, the Demo Account. Its button logs in as "demo" with an empty
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

  // The search term comes from the names the list shows and the list answer
  // behind them, so the check does not depend on which names the demo
  // generator picks: it needs only one shown name holding a word that some
  // other listed conversation's title, participant names and addresses all
  // lack (#2731).
  step = "list the Demo Account's conversations";
  const all = await listed;
  if (!all.ok()) throw new Error(`the list answered ${all.status()}`);
  const listing = await all.json();
  const allTotal = listing.total;
  const rows = page.getByRole("checkbox", { name: /^Select (?!all conversations$)/ });
  await rows.first().waitFor();
  const labels = await rows.evaluateAll((boxes) =>
    boxes.map((box) => box.getAttribute("aria-label") ?? ""),
  );
  const titles = labels
    .map((label) => label.replace(/^Select /, ""))
    .filter((name) => name !== "");
  const { term, title } = narrowingTerm(titles, listing.items ?? []);

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

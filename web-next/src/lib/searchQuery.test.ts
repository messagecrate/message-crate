import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { describe, it } from "node:test";
import {
  composeSearchQuery,
  formFromSearchQuery,
  hasSearchCriteria,
  parseSearchQuery,
  parseSizeBytes,
  toFtsMatch,
} from "./searchQuery";

type GoldenCase = {
  name: string;
  input: string;
  expected: ReturnType<typeof parseSearchQuery>;
};

describe("parseSearchQuery goldens", () => {
  const casesPath = path.join(
    process.cwd(),
    "..",
    "tests",
    "fixtures",
    "search",
    "parse-cases.json",
  );
  const cases = JSON.parse(fs.readFileSync(casesPath, "utf8")) as GoldenCase[];

  for (const c of cases) {
    it(c.name, () => {
      // Round-trip through JSON so `prefix: undefined` matches omitted keys in goldens.
      const actual = JSON.parse(JSON.stringify(parseSearchQuery(c.input)));
      assert.deepEqual(actual, c.expected);
    });
  }

  it("relative after:7d yields YYYY-MM-DD (not in goldens — clock-dependent)", () => {
    assert.match(parseSearchQuery("after:7d").after ?? "", /^\d{4}-\d{2}-\d{2}$/);
  });

  it("parseSizeBytes accepts fractional megabytes", () => {
    assert.equal(parseSizeBytes("1.5M"), Math.round(1.5 * 1024 * 1024));
  });
});

describe("composeSearchQuery", () => {
  it("composes phase-2 message result operators", () => {
    const s = composeSearchQuery({
      hasWords: "hello",
      groupBy: "none",
      sort: "date-asc",
      context: 3,
      attachmentFilter: "yes",
      larger: "1M",
      smaller: "10M",
    });
    assert.match(s, /\bgroup:none\b/);
    assert.match(s, /\bsort:date-asc\b/);
    assert.match(s, /\bcontext:3\b/);
    assert.match(s, /\blarger:1M\b/);
    assert.match(s, /\bsmaller:10M\b/);
  });
});

describe("composeSearchQuery", () => {
  it("round-trips message form fields into operators", () => {
    const s = composeSearchQuery({
      mode: "messages",
      within: "Close Friends",
      withPerson: "Ann Lee",
      fromPerson: "me",
      toPerson: "Sam",
      hasWords: "birthday",
      doesntHave: "spam",
      conversationType: "group",
      attachmentFilter: "yes",
      filetype: "image",
      filename: "photo",
    });
    assert.match(s, /with:"Ann Lee"/);
    assert.match(s, /from:me/);
    assert.match(s, /to:Sam/);
    assert.match(s, /within:"Close Friends"/);
    assert.match(s, /birthday/);
    assert.match(s, /-spam/);
    assert.match(s, /is:group/);
    assert.match(s, /has:attachment/);
    assert.match(s, /filetype:image/);
    assert.match(s, /filename:photo/);
    assert.doesNotMatch(s, /in:trash/);

    const parsed = parseSearchQuery(s);
    assert.equal(parsed.with, "Ann Lee");
    assert.equal(parsed.from, "me");
    assert.equal(parsed.to, "Sam");
    assert.equal(parsed.within, "Close Friends");
    assert.deepEqual(parsed.terms, ["birthday"]);
    assert.deepEqual(parsed.exclude, ["spam"]);
    assert.equal(parsed.conversationType, "group");
    assert.equal(parsed.hasAttachment, true);
  });

  it("turns a Date range into after: plus before:", () => {
    const s = composeSearchQuery({
      date: { mode: "between", from: "2020-01-01", to: "2020-03-01" },
    });
    assert.equal(s, "after:2020-01-01 before:2020-03-01");
    const parsed = parseSearchQuery(s);
    assert.equal(parsed.after, "2020-01-01");
    assert.equal(parsed.before, "2020-03-01");
  });

  it("emits only the filled side of a date range", () => {
    assert.equal(
      composeSearchQuery({ date: { mode: "on-or-after", from: "2020-01-01" } }),
      "after:2020-01-01",
    );
    assert.equal(
      composeSearchQuery({ date: { mode: "before", to: "2020-01-01" } }),
      "before:2020-01-01",
    );
    assert.equal(
      composeSearchQuery({
        mode: "contacts",
        firstContact: { mode: "between", from: "2020-01-01" },
      }),
      "search:contacts first-contact:>=2020-01-01",
    );
  });

  it("omits date fields left on Any time", () => {
    assert.equal(
      composeSearchQuery({
        date: { mode: "any", from: "2020-01-01", to: "2020-03-01" },
        firstContact: { mode: "any" },
        lastContact: { mode: "any" },
      }),
      "",
    );
  });

  it("composes only contact fields in contact mode", () => {
    const s = composeSearchQuery({
      mode: "contacts",
      within: "Close Friends",
      firstName: "Ann",
      lastName: "Lee",
      phone: "+1555",
      firstContact: { mode: "on-or-after", from: "2019-06-01" },
      lastContact: { mode: "before", to: "2024-01-15" },
      groupCount: { comparator: ">=", value: "2" },
      messageCount: { comparator: "<", value: "100" },
      withPerson: "ignored",
      hasWords: "ignored",
      hasAttachment: true,
    });
    assert.equal(
      s,
      'search:contacts within:"Close Friends" first:Ann last:Lee phone:+1555 first-contact:>=2019-06-01 last-contact:<2024-01-15 group-count:>=2 message-count:<100',
    );
  });

  it("composes is:nofirst / is:nolast without first/last text", () => {
    assert.equal(
      composeSearchQuery({
        mode: "contacts",
        noFirstName: true,
        noLastName: true,
        firstName: "ignored",
        lastName: "ignored",
        phone: "555",
      }),
      "search:contacts is:nofirst is:nolast phone:555",
    );
  });

  it("emits shared Within but not contact-only fields in message mode", () => {
    assert.equal(
      composeSearchQuery({
        mode: "messages",
        within: "Family",
        handle: "Ann",
        groupCount: { comparator: ">", value: "2" },
        hasWords: "hello",
      }),
      "within:Family hello",
    );
  });

  it("composes with-person name fields in message mode", () => {
    assert.equal(
      composeSearchQuery({
        mode: "messages",
        firstName: "Ann",
        lastName: "Lee",
        phone: "555",
        hasWords: "hello",
      }),
      "first:Ann last:Lee phone:555 hello",
    );
  });
});

describe("formFromSearchQuery", () => {
  it("hydrates Date and First contact from the query bar", () => {
    const form = formFromSearchQuery(
      "after:2000-01-01 first-contact:<2020-10-10",
    );
    assert.deepEqual(form.date, {
      mode: "on-or-after",
      from: "2000-01-01",
      to: "",
    });
    assert.deepEqual(form.firstContact, {
      mode: "before",
      from: "",
      to: "2020-10-10",
    });
    assert.deepEqual(form.lastContact, { mode: "any", from: "", to: "" });
  });

  it("round-trips compose → form for message fields", () => {
    const composed = composeSearchQuery({
      mode: "messages",
      withPerson: "Ann Lee",
      fromPerson: "me",
      toPerson: "Sam",
      hasWords: 'birthday "exact phrase"',
      doesntHave: "spam",
      conversationType: "group",
      attachmentFilter: "yes",
      source: "imessage",
      date: { mode: "between", from: "2020-01-01", to: "2020-03-01" },
    });
    const form = formFromSearchQuery(composed);
    assert.equal(form.mode, "messages");
    assert.equal(form.withPerson, "Ann Lee");
    assert.equal(form.fromPerson, "me");
    assert.equal(form.toPerson, "Sam");
    // Exclusions stay in Has the words when boolean AST is used.
    assert.equal(form.hasWords, 'birthday "exact phrase" -spam');
    assert.equal(form.doesntHave, undefined);
    assert.equal(form.conversationType, "group");
    assert.equal(form.attachmentFilter, "yes");
    assert.equal(form.hasAttachment, true);
    assert.equal(form.source, "imessage");
    assert.deepEqual(form.date, {
      mode: "between",
      from: "2020-01-01",
      to: "2020-03-01",
    });
  });

  it("maps is:direct and plain free text", () => {
    const form = formFromSearchQuery('hello is:direct -"bad word"');
    assert.equal(form.hasWords, 'hello -"bad word"');
    assert.equal(form.doesntHave, undefined);
    assert.equal(form.conversationType, "individual");
  });

  it("hydrates contact form mode and count fields", () => {
    const form = formFromSearchQuery(
      'search:contacts first:Ann last:Lee phone:555 group-count:>=2 message-count:=12',
    );
    assert.equal(form.mode, "contacts");
    assert.equal(form.firstName, "Ann");
    assert.equal(form.lastName, "Lee");
    assert.equal(form.phone, "555");
    assert.equal(form.handle, undefined);
    assert.deepEqual(form.groupCount, { comparator: ">=", value: "2" });
    assert.deepEqual(form.messageCount, { comparator: "=", value: "12" });
  });

  it("keeps handle: on the combined Handle field", () => {
    const name = formFromSearchQuery('search:contacts handle:"Ann Lee"');
    assert.equal(name.handle, "Ann Lee");
    assert.equal(name.firstName, undefined);
    assert.equal(name.phone, undefined);
    const phone = formFromSearchQuery("search:contacts handle:+15550112");
    assert.equal(phone.handle, "+15550112");
    assert.equal(phone.phone, undefined);
  });

  it("hydrates is:nofirst / is:nolast and legacy is:nameless", () => {
    const form = formFromSearchQuery("search:contacts is:nofirst is:nolast");
    assert.equal(form.noFirstName, true);
    assert.equal(form.noLastName, true);
    const legacy = formFromSearchQuery("search:contacts is:nameless");
    assert.equal(legacy.noFirstName, true);
    assert.equal(legacy.noLastName, true);
  });

  it("hydrates from:, to:, subject:, and with:", () => {
    const form = formFromSearchQuery(
      'from:alice to:me subject:hi with:bob has:noattachment',
    );
    assert.equal(form.fromPerson, "alice");
    assert.equal(form.toPerson, "me");
    assert.equal(form.subject, "hi");
    assert.equal(form.withPerson, "bob");
    assert.equal(form.attachmentFilter, "no");
    assert.equal(form.hasWords, undefined);
  });
});

describe("toFtsMatch / hasSearchCriteria", () => {
  it("builds an FTS expression from terms and exclusions", () => {
    const q = parseSearchQuery('hello "exact" -nope');
    assert.equal(toFtsMatch(q), '"hello" AND "exact" AND NOT "nope"');
    assert.equal(hasSearchCriteria(q), true);
  });

  it("compiles OR, grouping, and prefix*", () => {
    assert.equal(toFtsMatch(parseSearchQuery("cat OR dog")), '("cat" OR "dog")');
    assert.equal(
      toFtsMatch(parseSearchQuery("(hello OR world) party")),
      '("hello" OR "world") AND "party"',
    );
    assert.equal(toFtsMatch(parseSearchQuery("avoc*")), "avoc*");
    assert.equal(
      toFtsMatch(parseSearchQuery("avoc* OR pine*")),
      "(avoc* OR pine*)",
    );
    // AND binds tighter than OR
    assert.equal(
      toFtsMatch(parseSearchQuery("hello OR world party")),
      '("hello" OR ("world" AND "party"))',
    );
  });

  it("keeps operators outside the FTS expression", () => {
    const q = parseSearchQuery("from:me (cat OR dog) has:attachment");
    assert.equal(q.from, "me");
    assert.equal(q.hasAttachment, true);
    assert.equal(toFtsMatch(q), '("cat" OR "dog")');
  });

  it("returns null when only metadata filters are set", () => {
    const q = parseSearchQuery("has:attachment is:group with:bob");
    assert.equal(toFtsMatch(q), null);
    assert.equal(hasSearchCriteria(q), true);
  });
});

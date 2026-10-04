import { describe, expect, it } from "vitest";
import type { ImportIssue } from "../../components/import/ImportSummaryPanel";
import type { PushFinishedReport } from "../../lib/tauri";
import {
  EMPTY_RUN_RECORD,
  filesSkippedOverRun,
  issuesToDiscard,
  parseRunRecord,
  type RunPart,
  type RunRecord,
  recordToCarry,
  resolveInRecord,
  wholeRun,
} from "./runRecord";

function report(overrides: Partial<PushFinishedReport> = {}): PushFinishedReport {
  return {
    ok: true,
    cancelled: false,
    session_refused: false,
    messages_attempted: 10,
    messages_inserted: 10,
    messages_deduped: 0,
    messages_failed: 0,
    assets_uploaded: 2,
    assets_bytes: 2_000,
    conversations_ok: 1,
    conversations_total: 1,
    conversations_failed: 0,
    conversations_skipped: 0,
    conversations_cancelled: 0,
    results: [],
    ...overrides,
  };
}

function part(overrides: Partial<RunPart> = {}): RunPart {
  return {
    issues: [],
    durationMs: 1_000.4,
    parseMs: null,
    attachmentsMs: null,
    prepareMs: null,
    uploadMs: null,
    conversations: new Map(),
    report: null,
    ...overrides,
  };
}

/**
 * The fields an Upload row has beside its kind and reason: the conversation
 * it is about, and `item`, which is the conversation itself for a row about
 * the whole conversation.
 */
function upload(conversation: string, item: string = conversation) {
  return { stage: "upload" as const, item, conversation };
}

describe("parseRunRecord", () => {
  it("reads a record back field by field", () => {
    const record = {
      issues: [{ kind: "skip", stage: "staging", item: "a.jpg", reason: "missing" }],
      parseMs: 10,
      uploadMs: 20,
      bytesUploaded: 30,
      messagesInserted: 40,
    };
    expect(parseRunRecord(record)).toEqual(record);
  });

  it("keeps the conversation an Upload row is about", () => {
    const row = { ...upload("a.jsonl", "a.jsonl:big.mov"), kind: "skip", reason: "too large" };
    expect(parseRunRecord({ issues: [], lastStopIssues: [row] }).lastStopIssues).toEqual([row]);
  });

  it("reads anything that is not a record as no earlier part", () => {
    expect(parseRunRecord(null)).toEqual(EMPTY_RUN_RECORD);
    expect(parseRunRecord("garbage")).toEqual(EMPTY_RUN_RECORD);
  });

  it("leaves out a malformed issue and a field that is not a number", () => {
    expect(
      parseRunRecord({
        issues: [{ kind: "skip" }, { kind: "error", stage: "upload", item: "x", reason: "y" }],
        uploadMs: "soon",
      }),
    ).toEqual({ issues: [{ kind: "error", stage: "upload", item: "x", reason: "y" }] });
  });
});

describe("wholeRun", () => {
  it("adds this part's times, bytes and Upload counts to the earlier parts'", () => {
    const carried: RunRecord = {
      issues: [{ kind: "skip", stage: "staging", item: "a.jpg", reason: "missing" }],
      durationMs: 5_000,
      parseMs: 100,
      attachmentsMs: 200,
      prepareMs: 300,
      uploadMs: 400,
      bytesUploaded: 1_000,
      filesParsed: 3,
      messagesParsed: 30,
      filesSucceeded: 2,
      messagesAttempted: 20,
      messagesInserted: 20,
      messagesDeduped: 0,
      attachmentsUploaded: 1,
    };
    const whole = wholeRun(
      carried,
      part({
        issues: [{ kind: "skip", stage: "upload", item: "c.jsonl:big.mov", reason: "too large" }],
        uploadMs: 600.6,
        report: report({ conversations_ok: 1, conversations_skipped: 2, conversations_total: 3 }),
      }),
    );

    expect(whole).toEqual({
      issues: [
        { kind: "skip", stage: "staging", item: "a.jpg", reason: "missing" },
        { kind: "skip", stage: "upload", item: "c.jsonl:big.mov", reason: "too large" },
      ],
      // Whole milliseconds, which is what the server stores.
      durationMs: 6_000,
      parseMs: 100,
      attachmentsMs: 200,
      prepareMs: 300,
      uploadMs: 1_001,
      bytesUploaded: 3_000,
      // A resumed Upload stages nothing, so Staging's counts are the earlier part's.
      filesParsed: 3,
      messagesParsed: 30,
      filesSucceeded: 3,
      messagesAttempted: 30,
      messagesInserted: 30,
      messagesDeduped: 0,
      attachmentsUploaded: 3,
    });
  });

  it("leaves a time no part measured unknown rather than zero", () => {
    expect(wholeRun(EMPTY_RUN_RECORD, part()).uploadMs).toBeUndefined();
  });
});

describe("recordToCarry", () => {
  it("keeps the attachment skips of a stopped Upload and drops what the resume reports again", () => {
    const issues: ImportIssue[] = [
      { kind: "skip", stage: "staging", item: "a.jpg", reason: "missing" },
      { ...upload("a.jsonl", "a.jsonl:big.mov"), kind: "skip", reason: "too large" },
      { ...upload("b.jsonl"), kind: "error", reason: "connection refused" },
      { ...upload("c.jsonl"), kind: "skip", reason: "the Upload was stopped" },
      { kind: "error", stage: "upload", item: "Import", reason: "the server went away" },
    ];
    const carried = recordToCarry(
      EMPTY_RUN_RECORD,
      part({
        issues,
        report: report({
          ok: false,
          results: [
            { file: "a.jsonl", status: "ok", messages: 1, attachments: 1 },
            { file: "b.jsonl", status: "failed", messages: 0, attachments: 0 },
            { file: "c.jsonl", status: "cancelled", messages: 0, attachments: 0 },
          ],
        }),
      }),
    );
    expect(carried.issues).toEqual([
      { kind: "skip", stage: "staging", item: "a.jpg", reason: "missing" },
      { ...upload("a.jsonl", "a.jsonl:big.mov"), kind: "skip", reason: "too large" },
    ]);
  });

  it("keeps what it leaves out apart, for a Discard to send (#1479)", () => {
    const carried = recordToCarry(
      EMPTY_RUN_RECORD,
      part({
        issues: [
          { kind: "skip", stage: "staging", item: "a.jpg", reason: "missing" },
          { ...upload("b.jsonl"), kind: "error", reason: "connection refused" },
          { kind: "error", stage: "upload", item: "Import", reason: "the server went away" },
        ],
        report: report({
          ok: false,
          results: [{ file: "b.jsonl", status: "failed", messages: 0, attachments: 0 }],
        }),
      }),
    );
    expect(carried.lastStopIssues).toEqual([
      { ...upload("b.jsonl"), kind: "error", reason: "connection refused" },
      { kind: "error", stage: "upload", item: "Import", reason: "the server went away" },
    ]);
    expect(issuesToDiscard(parseRunRecord(JSON.parse(JSON.stringify(carried))))).toEqual([
      { kind: "skip", stage: "staging", item: "a.jpg", reason: "missing" },
      { ...upload("b.jsonl"), kind: "error", reason: "connection refused" },
      { kind: "error", stage: "upload", item: "Import", reason: "the server went away" },
    ]);
  });

  it("keeps an earlier stop's failed conversation when the next stop reported nothing on it", () => {
    // Pause 1 left conversation b.jsonl failed. The resumed Upload was paused
    // before the push started, so it reported nothing on b.jsonl.
    const earlier = {
      issues: [],
      lastStopIssues: [
        { ...upload("b.jsonl"), kind: "error", reason: "connection refused" },
        { kind: "error", stage: "upload" as const, item: "Import", reason: "the server went away" },
      ],
    };
    const carried = recordToCarry(earlier, part({ issues: [], report: null }));
    expect(carried.lastStopIssues).toEqual([
      { ...upload("b.jsonl"), kind: "error", reason: "connection refused" },
    ]);
  });

  it("drops an earlier stop's failed conversation once a later Upload reported on it", () => {
    const earlier = {
      issues: [],
      lastStopIssues: [{ ...upload("b.jsonl"), kind: "error", reason: "connection refused" }],
    };
    const carried = recordToCarry(
      earlier,
      part({
        issues: [],
        report: report({
          results: [{ file: "b.jsonl", status: "ok", messages: 1, attachments: 0 }],
        }),
      }),
    );
    expect(carried.lastStopIssues).toEqual([]);
    // A completion covers the whole run, and the resume reported these again.
    expect(wholeRun(earlier, part({ issues: [] })).lastStopIssues).toBeUndefined();
  });
});

describe("the record written while a stage runs (#1639)", () => {
  const skip = { ...upload("a.jsonl", "a.jsonl:big.mov"), kind: "skip", reason: "too large" };

  it("holds an Upload's row apart until its conversation is on the server", () => {
    // A crash now would leave a.jsonl out of the journal, and the resumed
    // Upload would read it again and report the skip again.
    const sending = recordToCarry(EMPTY_RUN_RECORD, part({ issues: [skip] }));
    expect(sending.issues).toEqual([]);
    expect(sending.lastStopIssues).toEqual([skip]);
    expect(issuesToDiscard(sending)).toEqual([skip]);

    const sent = recordToCarry(
      EMPTY_RUN_RECORD,
      part({ issues: [skip], conversations: new Map([["a.jsonl", "ok"]]) }),
    );
    expect(sent.issues).toEqual([skip]);
    expect(sent.lastStopIssues).toEqual([]);
  });

  it("drops an earlier stop's failed conversation as soon as the resumed Upload sends it", () => {
    // Pause 1 left a.jsonl failed. The resumed Upload sends it, and the app
    // closes before that Upload ends: the record written then must not hold
    // the failure for a Discard to send.
    const earlier: RunRecord = {
      issues: [],
      lastStopIssues: [{ ...upload("a.jsonl"), kind: "error", reason: "connection refused" }],
    };
    const resumed = recordToCarry(earlier, part({ conversations: new Map([["a.jsonl", "ok"]]) }));
    expect(issuesToDiscard(resumed)).toEqual([]);
  });

  it("keeps an earlier stop's attachment skip for a conversation the journal shows was sent", () => {
    // The earlier part sent a.jsonl but closed before its record said so.
    // The resumed Upload skips a.jsonl and never reads it again.
    const earlier: RunRecord = { issues: [], lastStopIssues: [skip] };
    const resumed = recordToCarry(
      earlier,
      part({ conversations: new Map([["a.jsonl", "skipped"]]) }),
    );
    expect(resumed.issues).toEqual([skip]);
    expect(resumed.lastStopIssues).toEqual([]);
    expect(
      wholeRun(earlier, part({ conversations: new Map([["a.jsonl", "skipped"]]) })).issues,
    ).toEqual([skip]);
  });

  it("keeps an earlier stop's attachment skip when the conversation failed before it was read", () => {
    // Part 2's prepare of a.jsonl failed, so it reported none of its skips.
    const earlier: RunRecord = { issues: [], lastStopIssues: [skip] };
    const resumed = recordToCarry(
      earlier,
      part({ conversations: new Map([["a.jsonl", "failed"]]) }),
    );
    expect(issuesToDiscard(resumed)).toEqual([skip]);
  });

  it("completes with an earlier stop's attachment skip of a conversation that failed again", () => {
    // A completion is never resumed, so nothing will report the skip again.
    const earlier: RunRecord = { issues: [], lastStopIssues: [skip] };
    expect(
      wholeRun(earlier, part({ conversations: new Map([["a.jsonl", "failed"]]) })).issues,
    ).toEqual([skip]);
  });

  it("drops a row a later try resolved, wherever the record keeps it", () => {
    const media = { kind: "skip", stage: "media" as const, item: "a.jsonl:IMG.HEIC", reason: "x" };
    const record: RunRecord = { issues: [media], lastStopIssues: [skip] };
    expect(resolveInRecord(record, { stage: "media", item: "a.jsonl:IMG.HEIC" })).toEqual({
      issues: [],
      lastStopIssues: [skip],
    });
  });

  it("keeps once a row that a resumed stage reports again", () => {
    // Staging closed after reporting the photo but before writing its
    // conversation, so the resumed Staging read it again.
    const photo = { kind: "error", stage: "staging" as const, item: "IMG_1.HEIC", reason: "x" };
    const carried = recordToCarry(EMPTY_RUN_RECORD, part({ issues: [photo] }));
    expect(wholeRun(carried, part({ issues: [photo] })).issues).toEqual([photo]);
    // An Upload row reported again by the resumed Upload is kept once too.
    const waiting = recordToCarry(EMPTY_RUN_RECORD, part({ issues: [skip] }));
    expect(
      recordToCarry(
        waiting,
        part({ issues: [skip], conversations: new Map([["a.jsonl", "failed"]]) }),
      ).lastStopIssues,
    ).toEqual([skip]);
  });
});

describe("filesSkippedOverRun", () => {
  it("does not count as skipped the conversations an earlier part sent", () => {
    const carried = { issues: [], filesSucceeded: 200 };
    expect(filesSkippedOverRun(carried, report({ conversations_skipped: 205 }))).toBe(5);
    expect(filesSkippedOverRun(EMPTY_RUN_RECORD, report({ conversations_skipped: 4 }))).toBe(4);
  });
});

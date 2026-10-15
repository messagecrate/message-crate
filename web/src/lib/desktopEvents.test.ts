// The desktop app sends its job events to the window as JSON, from Rust
// structs in src-tauri/src/commands/events.rs that have a TypeScript twin in
// types.ts each. Nothing generates one from the other, so a test there writes
// tests/fixtures/desktop-events/events.json from the real structs, and this
// test checks every payload in it against its TypeScript type (#2109): a
// field renamed or removed in Rust, a field the TypeScript type lacks, or a
// word outside a TypeScript union fails here, with one exception that the
// header of src-tauri/src/commands/events/fixture.rs names.
//
// Each type's shape below is checked against the type by the compiler, so a
// field renamed in TypeScript fails the type-check, and the payloads then
// check the shape at run time.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import type {
  DesktopJobErrorEvent,
  ImportFileDoneEvent,
  ImportFileWrittenEvent,
  ImportIssueEvent,
  ImportProgressEvent,
} from "./types";

/** How a field's value is checked: its `typeof`, or every word of a union. */
type ValueShape<V> = [V] extends [boolean]
  ? "boolean"
  : [V] extends [number]
    ? "number"
    : string extends V
      ? "string"
      : [V] extends [string]
        ? Record<V, true>
        : never;

/** An optional field's shape: a payload may leave the field out. */
interface Optional<S> {
  optional: S;
}

/**
 * Every field of `T`, optional ones included, with how its value is checked.
 * `satisfies Shape<T>` refuses a missing field, an extra one, a missing or
 * extra word of a union, and an optional field written as required.
 */
type Shape<T> = {
  [K in keyof T]-?: undefined extends T[K]
    ? Optional<ValueShape<NonNullable<T[K]>>>
    : ValueShape<T[K]>;
};

type AnyValueShape = "boolean" | "number" | "string" | Record<string, true>;

const PROGRESS = {
  step: {
    setup: true,
    parse: true,
    attachments: true,
    prepare: true,
    check: true,
    media: true,
    upload: true,
  },
  done: "number",
  total: "number",
  bytes_done: { optional: "number" },
  bytes_total: { optional: "number" },
  status: { optional: "string" },
  waiting: { optional: { ffmpeg: true, ffprobe: true, wtsexporter: true } },
} satisfies Shape<ImportProgressEvent>;

const ISSUE = {
  kind: { skip: true, error: true, note: true, resolved: true },
  step: { parse: true, attachments: true, prepare: true, media: true, upload: true },
  item: "string",
  reason: "string",
  conversation: { optional: "string" },
} satisfies Shape<ImportIssueEvent>;

const FILE_DONE = {
  file: "string",
  status: { ok: true, skipped: true, failed: true },
} satisfies Shape<ImportFileDoneEvent>;

const FILE_WRITTEN = {
  file: "string",
  status: { written: true, skipped: true },
} satisfies Shape<ImportFileWrittenEvent>;

const ERROR = {
  detail: "string",
  user_message: { optional: "string" },
  canceled: "boolean",
} satisfies Shape<DesktopJobErrorEvent>;

const FIXTURE_PATH = fileURLToPath(
  new URL("../../../tests/fixtures/desktop-events/events.json", import.meta.url),
);

const fixture = JSON.parse(readFileSync(FIXTURE_PATH, "utf8")) as Record<
  string,
  Record<string, unknown>[] | undefined
>;

type FieldShape = AnyValueShape | Optional<AnyValueShape>;

function isOptional(entry: FieldShape): entry is Optional<AnyValueShape> {
  return typeof entry === "object" && "optional" in entry;
}

/** Check every payload sent on `channel` against `shape`. */
function checkChannel(channel: string, shape: Record<string, FieldShape>): void {
  const payloads = fixture[channel] ?? [];
  expect(payloads.length, `the fixture has no ${channel} payload`).toBeGreaterThan(0);
  const fields = Object.keys(shape).sort();
  const required = fields.filter((field) => !isOptional(shape[field]));
  const sent = new Set<string>();
  for (const payload of payloads) {
    const at = `${channel} ${JSON.stringify(payload)}`;
    for (const field of required) {
      expect(payload, `${at} lacks the required ${field}`).toHaveProperty(field);
    }
    for (const [field, value] of Object.entries(payload)) {
      expect(fields, `${at} sends ${field}, which the TypeScript type lacks`).toContain(field);
      sent.add(field);
      const entry = shape[field];
      const valueShape = isOptional(entry) ? entry.optional : entry;
      if (typeof valueShape === "string") {
        expect(typeof value, `${at}: ${field}`).toBe(valueShape);
      } else {
        expect(Object.keys(valueShape), `${at}: ${field} is outside its union`).toContain(value);
      }
    }
  }
  expect([...sent].sort(), `a field of ${channel} that no payload sends`).toEqual(fields);
}

describe("the desktop app's job events", () => {
  it("desktop-job:progress matches ImportProgressEvent", () => {
    checkChannel("desktop-job:progress", PROGRESS);
  });

  it("desktop-job:issue matches ImportIssueEvent", () => {
    checkChannel("desktop-job:issue", ISSUE);
  });

  it("desktop-job:file-done matches ImportFileDoneEvent", () => {
    checkChannel("desktop-job:file-done", FILE_DONE);
  });

  it("desktop-job:file-written matches ImportFileWrittenEvent", () => {
    checkChannel("desktop-job:file-written", FILE_WRITTEN);
  });

  it("desktop-job:error matches DesktopJobErrorEvent", () => {
    checkChannel("desktop-job:error", ERROR);
  });

  it("the fixture holds no channel this test does not check", () => {
    expect(Object.keys(fixture).sort()).toEqual([
      "desktop-job:error",
      "desktop-job:file-done",
      "desktop-job:file-written",
      "desktop-job:issue",
      "desktop-job:progress",
    ]);
  });
});

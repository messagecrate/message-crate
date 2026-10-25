import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { MAX_MERGED_CONTACTS } from "./MergeContactsDialog";

describe("MAX_MERGED_CONTACTS", () => {
  it("is the cap the server's reference states", () => {
    const doc = JSON.parse(
      readFileSync(
        fileURLToPath(new URL("../../../../docs/src/assets/openapi.json", import.meta.url)),
        "utf8",
      ),
    );
    const from = doc.components.schemas.MergeContactRequest.properties.from;
    // `from` holds every contact but the one the others merge into.
    expect(from.maxItems + 1).toBe(MAX_MERGED_CONTACTS);
  });
});

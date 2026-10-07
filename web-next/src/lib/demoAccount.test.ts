import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { isDemoAccount } from "./demoAccount";

describe("demo account id", () => {
  it("matches the id the server fixes, as the string a session carries", () => {
    assert.equal(isDemoAccount("2"), true);
    assert.equal(isDemoAccount(2), true);
  });

  it("does not match other accounts", () => {
    assert.equal(isDemoAccount("1"), false);
    assert.equal(isDemoAccount("102"), false);
    assert.equal(isDemoAccount("00000000-0000-0000-0000-00000000d001"), false);
  });
});

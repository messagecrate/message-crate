import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { serviceLabel } from "./serviceLabel";

describe("serviceLabel", () => {
  it("names each service the server sends in product words", () => {
    assert.equal(serviceLabel("text_message"), "Text Message");
    assert.equal(serviceLabel("email"), "Email");
    assert.equal(serviceLabel("whatsapp"), "WhatsApp");
  });

  it("shows a value it does not know as it is", () => {
    assert.equal(serviceLabel("imessage"), "imessage");
  });
});

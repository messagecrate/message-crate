import { describe, expect, it } from "vitest";
import {
  formatOfferedServiceLabel,
  identityDuplicateKey,
  identityPlaceholder,
  identityValidationError,
  inferService,
  listedServerService,
  OFFERED_SERVICE_OPTIONS,
  OFFERED_SERVICES,
  serverService,
} from "./offeredService";

describe("offeredService", () => {
  it("lists text_message, email, whatsapp for profile/onboarding", () => {
    expect([...OFFERED_SERVICES]).toEqual(["text_message", "email", "whatsapp"]);
    expect(OFFERED_SERVICE_OPTIONS.map((o) => o.value)).toEqual([
      "text_message",
      "email",
      "whatsapp",
    ]);
  });

  it("infers email and phone from handle when service empty", () => {
    expect(inferService("a@example.com", null)).toBe("email");
    expect(inferService("+1 (555) 555-0119", "")).toBe("text_message");
    expect(inferService("alice", null)).toBe("unknown");
  });

  it("prefers an explicit service over inference", () => {
    expect(inferService("a@example.com", "WhatsApp")).toBe("whatsapp");
  });

  it("formats user-facing labels for the handles table", () => {
    expect(formatOfferedServiceLabel("x", "imessage")).toBe("Text Message");
    expect(formatOfferedServiceLabel("x", "whatsapp")).toBe("WhatsApp");
    expect(formatOfferedServiceLabel("a@example.com", null)).toBe("Email");
    expect(formatOfferedServiceLabel("x", null)).toBe("—");
  });
});

describe("serverService", () => {
  it("sends an email address on Text Message, and WhatsApp as WhatsApp", () => {
    expect(serverService("text_message")).toBe("text_message");
    expect(serverService("email")).toBe("text_message");
    expect(serverService("whatsapp")).toBe("whatsapp");
  });
});

describe("listedServerService", () => {
  it("reads the list's three services and names none for any other word", () => {
    expect(listedServerService("text_message")).toBe("text_message");
    expect(listedServerService("email")).toBe("text_message");
    expect(listedServerService("whatsapp")).toBe("whatsapp");
    // Read as Text Message, a word the server does not take would move or miss the
    // identity without a word (#1630).
    for (const word of ["phone", "sms", "whatsap", "unknown", "", null, undefined]) {
      expect(listedServerService(word)).toBeUndefined();
    }
  });
});

describe("identityPlaceholder", () => {
  it("gives each service its own example", () => {
    expect(identityPlaceholder("text_message")).toBe("+1 555-555-0119");
    expect(identityPlaceholder("email")).toBe("you@example.com");
    expect(identityPlaceholder("whatsapp")).toBe("+1 555-555-0119");
  });

  it("gives every option in the picker an example", () => {
    for (const option of OFFERED_SERVICE_OPTIONS) {
      expect(option.placeholder.length).toBeGreaterThan(0);
    }
  });

  it("calls a phone number what the contact drawer calls it", () => {
    expect(OFFERED_SERVICE_OPTIONS.find((o) => o.value === "text_message")?.label).toBe(
      "Text Message",
    );
  });
});

describe("identityValidationError", () => {
  it("passes an empty value, which is a row not filled in yet", () => {
    expect(identityValidationError("text_message", "")).toBeNull();
    expect(identityValidationError("email", "   ")).toBeNull();
  });

  it("accepts the separators people actually type in a number", () => {
    expect(identityValidationError("text_message", "+1 555-555-0119")).toBeNull();
    expect(identityValidationError("text_message", "(555) 555.0119")).toBeNull();
    expect(identityValidationError("whatsapp", "+44 20 7946 0958")).toBeNull();
  });

  it("takes any value on Text Message and WhatsApp, which the server types as other when it is no number", () => {
    // The server stores a WhatsApp internal id and a sender name as identities
    // of type other, and a short code as its digits (#2709), so refusing them
    // here only stops a person adding what an import already adds (#3090).
    expect(identityValidationError("whatsapp", "123456789012345@lid")).toBeNull();
    expect(identityValidationError("text_message", "sam")).toBeNull();
    expect(identityValidationError("text_message", "AMAZON")).toBeNull();
    expect(identityValidationError("text_message", "72975")).toBeNull();
  });

  it("refuses a value longer than the server keeps, on every service", () => {
    // The server refuses an address over 320 characters, counted as Unicode
    // characters after trimming (MAX_IDENTITY_ADDRESS_CHARS).
    const longest = `${"a".repeat(308)}@example.com`;
    expect(longest).toHaveLength(320);
    expect(identityValidationError("email", longest)).toBeNull();
    expect(identityValidationError("whatsapp", "😀".repeat(320))).toBeNull();
    for (const service of ["text_message", "whatsapp", "email"] as const) {
      expect(identityValidationError(service, `${longest}x`)).toBe(
        "An identity can be at most 320 characters.",
      );
    }
  });

  it("accepts an address and rejects what is not one", () => {
    expect(identityValidationError("email", "you@example.com")).toBeNull();
    expect(identityValidationError("email", "you@example")).toMatch(/email address/);
    expect(identityValidationError("email", "+1 555-555-0119")).toMatch(/email address/);
  });
});

describe("identityDuplicateKey", () => {
  it("has no key for a value with nothing to compare", () => {
    expect(identityDuplicateKey("text_message", "")).toBeNull();
    expect(identityDuplicateKey("email", "   ")).toBeNull();
  });

  it("matches the same number however it was typed", () => {
    expect(identityDuplicateKey("text_message", "+1 (555) 555-0119")).toBe(
      identityDuplicateKey("text_message", "+15555550119"),
    );
  });

  it("matches an address regardless of case", () => {
    expect(identityDuplicateKey("email", "You@Example.com")).toBe(
      identityDuplicateKey("email", "you@example.com"),
    );
  });

  it("keeps the same number on two services apart, because that is two accounts", () => {
    expect(identityDuplicateKey("text_message", "+15555550119")).not.toBe(
      identityDuplicateKey("whatsapp", "+15555550119"),
    );
  });

  it("does not fold a number written with and without its country code", () => {
    // Deliberate: guessing at country codes would let this refuse two numbers
    // that really are different.
    expect(identityDuplicateKey("text_message", "+1 555-555-0119")).not.toBe(
      identityDuplicateKey("text_message", "555-555-0119"),
    );
  });

  it("keeps a WhatsApp internal id apart from the number made of its digits", () => {
    expect(identityDuplicateKey("whatsapp", "123456789012345@lid")).not.toBe(
      identityDuplicateKey("whatsapp", "+123456789012345"),
    );
  });

  it("keeps two values with no digit apart, though both are made of number characters", () => {
    expect(identityDuplicateKey("whatsapp", "-")).not.toBe(identityDuplicateKey("whatsapp", "()"));
  });

  it("matches a sender name regardless of case", () => {
    expect(identityDuplicateKey("text_message", "AMAZON")).toBe(
      identityDuplicateKey("text_message", "amazon"),
    );
  });

  it("separates two different accounts", () => {
    expect(identityDuplicateKey("email", "a@example.com")).not.toBe(
      identityDuplicateKey("email", "b@example.com"),
    );
  });
});

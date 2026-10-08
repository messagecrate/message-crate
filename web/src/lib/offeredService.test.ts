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
  it("lists phone, email, whatsapp for profile/onboarding", () => {
    expect([...OFFERED_SERVICES]).toEqual(["phone", "email", "whatsapp"]);
    expect(OFFERED_SERVICE_OPTIONS.map((o) => o.value)).toEqual(["phone", "email", "whatsapp"]);
  });

  it("infers email and phone from handle when service empty", () => {
    expect(inferService("a@example.com", null)).toBe("email");
    expect(inferService("+1 (555) 555-0119", "")).toBe("phone");
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
  it("sends an email address on the phone service, and WhatsApp as WhatsApp", () => {
    expect(serverService("phone")).toBe("phone");
    expect(serverService("email")).toBe("phone");
    expect(serverService("whatsapp")).toBe("whatsapp");
  });
});

describe("listedServerService", () => {
  it("reads the list's three services and names none for any other word", () => {
    expect(listedServerService("phone")).toBe("phone");
    expect(listedServerService("email")).toBe("phone");
    expect(listedServerService("whatsapp")).toBe("whatsapp");
    // Read as phone, a word the server does not take would move or miss the
    // identity without a word (#1630).
    for (const word of ["sms", "whatsap", "unknown", "", null, undefined]) {
      expect(listedServerService(word)).toBeUndefined();
    }
  });
});

describe("identityPlaceholder", () => {
  it("gives each service its own example", () => {
    expect(identityPlaceholder("phone")).toBe("+1 555-555-0119");
    expect(identityPlaceholder("email")).toBe("you@example.com");
    expect(identityPlaceholder("whatsapp")).toBe("+1 555-555-0119");
  });

  it("gives every option in the picker an example", () => {
    for (const option of OFFERED_SERVICE_OPTIONS) {
      expect(option.placeholder.length).toBeGreaterThan(0);
    }
  });

  it("calls a phone number what the contact drawer calls it", () => {
    expect(OFFERED_SERVICE_OPTIONS.find((o) => o.value === "phone")?.label).toBe("Text Message");
  });
});

describe("identityValidationError", () => {
  it("passes an empty value, which is a row not filled in yet", () => {
    expect(identityValidationError("phone", "")).toBeNull();
    expect(identityValidationError("email", "   ")).toBeNull();
  });

  it("accepts the separators people actually type in a number", () => {
    expect(identityValidationError("phone", "+1 555-555-0119")).toBeNull();
    expect(identityValidationError("phone", "(555) 555.0119")).toBeNull();
    expect(identityValidationError("whatsapp", "+44 20 7946 0958")).toBeNull();
  });

  it("rejects a number that is not one", () => {
    expect(identityValidationError("phone", "notaphone")).toMatch(/phone number/);
    expect(identityValidationError("phone", "123")).toMatch(/phone number/);
    expect(identityValidationError("phone", "you@example.com")).toMatch(/phone number/);
  });

  it("rejects a number longer than E.164 allows", () => {
    expect(identityValidationError("phone", "+1234567890123456")).toMatch(/phone number/);
  });

  it("accepts an address and rejects what is not one", () => {
    expect(identityValidationError("email", "you@example.com")).toBeNull();
    expect(identityValidationError("email", "you@example")).toMatch(/email address/);
    expect(identityValidationError("email", "+1 555-555-0119")).toMatch(/email address/);
  });
});

describe("identityDuplicateKey", () => {
  it("has no key for a value with nothing to compare", () => {
    expect(identityDuplicateKey("phone", "")).toBeNull();
    expect(identityDuplicateKey("email", "   ")).toBeNull();
  });

  it("matches the same number however it was typed", () => {
    expect(identityDuplicateKey("phone", "+1 (555) 555-0119")).toBe(
      identityDuplicateKey("phone", "+15555550119"),
    );
  });

  it("matches an address regardless of case", () => {
    expect(identityDuplicateKey("email", "You@Example.com")).toBe(
      identityDuplicateKey("email", "you@example.com"),
    );
  });

  it("keeps the same number on two services apart, because that is two accounts", () => {
    expect(identityDuplicateKey("phone", "+15555550119")).not.toBe(
      identityDuplicateKey("whatsapp", "+15555550119"),
    );
  });

  it("does not fold a number written with and without its country code", () => {
    // Deliberate: guessing at country codes would let this refuse two numbers
    // that really are different.
    expect(identityDuplicateKey("phone", "+1 555-555-0119")).not.toBe(
      identityDuplicateKey("phone", "555-555-0119"),
    );
  });

  it("separates two different accounts", () => {
    expect(identityDuplicateKey("email", "a@example.com")).not.toBe(
      identityDuplicateKey("email", "b@example.com"),
    );
  });
});

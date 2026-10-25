import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { participant } from "../test/apiShapes";
import { conversationServiceLabel, IDENTITY_SERVICE_LABELS, serviceLabel } from "./serviceLabel";
import type { Conversation } from "./types";

function conversation(overrides: Partial<Conversation> = {}): Conversation {
  return {
    id: 3,
    participants: [participant({ name: "Ada Lovelace" })],
    message_count: 1,
    first_message_at: "2024-06-01T12:00:00Z",
    last_message_at: "2024-06-01T12:00:00Z",
    service: "imessage",
    is_group: false,
    shown_title: null,
    tags: [],
    ...overrides,
  };
}

describe("conversationServiceLabel", () => {
  it("calls a one-to-one conversation with no transport of its own after its participant's Text Message service", () => {
    // A conversation imported from two sources names no transport, so the
    // label falls back to the participant's stored service (#3086).
    const conv = conversation({
      service: "unknown",
      participants: [participant({ name: "Ada Lovelace", service: "text_message" })],
    });
    expect(conversationServiceLabel(conv)).toBe("Text Message");
  });

  it("keeps WhatsApp's own name", () => {
    expect(conversationServiceLabel(conversation({ service: "whatsapp" }))).toBe("WhatsApp");
  });

  it("names each service the server sends for a conversation of one import source", () => {
    // The server sends a conversation's service from its messages' import
    // sources (`display_service_label`), so a source id stands in for a
    // service. SMS Backup+ and GO SMS Pro read as Text Message, as they did
    // under the old "contains sms" rule.
    const cases: [string, string | null][] = [
      ["SMS/MMS", "Text Message"],
      ["imessage", "Text Message"],
      ["WhatsApp", "WhatsApp"],
      ["sms-backup-plus", "Text Message"],
      ["go-sms-pro", "Text Message"],
      ["openextract", "Text Message"],
      ["imazing", null],
      ["unknown", null],
    ];
    for (const [service, label] of cases) {
      expect(conversationServiceLabel(conversation({ service, is_group: true }))).toBe(label);
    }
  });

  it("calls a one-to-one conversation of two sources after its participant's WhatsApp service", () => {
    const conv = conversation({
      service: "unknown",
      participants: [participant({ name: "Ada Lovelace", service: "whatsapp" })],
    });
    expect(conversationServiceLabel(conv)).toBe("WhatsApp");
  });

  it("calls a one-to-one iMazing conversation after its participant's service", () => {
    // An iMazing export holds texts and WhatsApp chats alike, so its source
    // id names no service.
    const conv = conversation({
      service: "imazing",
      participants: [participant({ name: "Ada Lovelace", service: "whatsapp" })],
    });
    expect(conversationServiceLabel(conv)).toBe("WhatsApp");
  });
});

describe("serviceLabel", () => {
  it("names every service the server takes for an identity", () => {
    // The list comes from the published OpenAPI document the generated types
    // are built from, so a service the server adds fails here until it has a
    // name (#3262).
    const path = fileURLToPath(new URL("../../../docs/src/assets/openapi.json", import.meta.url));
    const openapi = JSON.parse(readFileSync(path, "utf8")) as {
      components: { schemas: { IdentityService: { enum: string[] } } };
    };
    const services = openapi.components.schemas.IdentityService.enum;
    expect(services.length).toBeGreaterThan(0);
    for (const service of services) {
      expect(Object.keys(IDENTITY_SERVICE_LABELS)).toContain(service);
      expect(serviceLabel(service)).toBe(
        IDENTITY_SERVICE_LABELS[service as keyof typeof IDENTITY_SERVICE_LABELS],
      );
    }
  });

  it("gives each transport of Text Message the same name as the service", () => {
    for (const transport of ["imessage", "ios", "sms", "mms", "sms/mms", "rcs", "SMS"]) {
      expect(serviceLabel(transport)).toBe("Text Message");
    }
  });

  it("names an email address's type, capitalizes any other word, and names nothing for unknown", () => {
    expect(serviceLabel("email")).toBe("Email");
    expect(serviceLabel("signal")).toBe("Signal");
    expect(serviceLabel("weChat")).toBe("WeChat");
    expect(serviceLabel("unknown")).toBeNull();
    expect(serviceLabel("  ")).toBeNull();
    expect(serviceLabel(null)).toBeNull();
  });
});

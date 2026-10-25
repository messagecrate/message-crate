import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { participant } from "../test/apiShapes";
import {
  CONVERSATION_SERVICE_LABELS,
  conversationServiceLabel,
  IDENTITY_SERVICE_LABELS,
  serviceLabel,
} from "./serviceLabel";
import type { Conversation } from "./types";

function conversation(overrides: Partial<Conversation> = {}): Conversation {
  return {
    id: 3,
    participants: [participant({ name: "Ada Lovelace" })],
    message_count: 1,
    first_message_at: "2024-06-01T12:00:00Z",
    last_message_at: "2024-06-01T12:00:00Z",
    service: "text_message",
    is_group: false,
    shown_title: null,
    tags: [],
    ...overrides,
  };
}

/** The enum the published OpenAPI document lists for schema `name`. */
function publishedEnum(name: "IdentityService" | "ConversationService"): string[] {
  // The generated types are built from this document, so a service the
  // server adds fails here until it has a name (#3262).
  const path = fileURLToPath(new URL("../../../docs/src/assets/openapi.json", import.meta.url));
  const openapi = JSON.parse(readFileSync(path, "utf8")) as {
    components: { schemas: Record<string, { enum: string[] }> };
  };
  return openapi.components.schemas[name]?.enum ?? [];
}

describe("conversationServiceLabel", () => {
  it("names every service the server names a conversation by", () => {
    const services = publishedEnum("ConversationService");
    expect(services.length).toBeGreaterThan(0);
    for (const service of services) {
      expect(Object.keys(CONVERSATION_SERVICE_LABELS)).toContain(service);
    }
  });

  it("calls a conversation by the same names as an identity's service", () => {
    expect(conversationServiceLabel(conversation({ service: "text_message" }))).toBe(
      "Text Message",
    );
    expect(conversationServiceLabel(conversation({ service: "whatsapp" }))).toBe("WhatsApp");
    expect(conversationServiceLabel(conversation({ service: "signal", is_group: true }))).toBe(
      "Signal",
    );
  });

  it("shows unknown as Unknown, never as the participant's service", () => {
    // `unknown` is a fault the server logs at import: a message whose
    // transport could not be read, or two services in one conversation. It
    // is shown so it gets looked into, not hidden or read as Text Message
    // (#3277).
    const conv = conversation({
      service: "unknown",
      participants: [participant({ name: "Ada Lovelace", service: "text_message" })],
    });
    expect(conversationServiceLabel(conv)).toBe("Unknown");
  });
});

describe("serviceLabel", () => {
  it("names every service the server takes for an identity", () => {
    const services = publishedEnum("IdentityService");
    expect(services.length).toBeGreaterThan(0);
    for (const service of services) {
      expect(Object.keys(IDENTITY_SERVICE_LABELS)).toContain(service);
      expect(serviceLabel(service)).toBe(
        IDENTITY_SERVICE_LABELS[service as keyof typeof IDENTITY_SERVICE_LABELS],
      );
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

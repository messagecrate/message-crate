import type { components } from "./vault/types.generated";

/**
 * The services a contact's identity can be on, as `/v1` lists them
 * (`Identity.service`): the two a request can name (`IdentityService`), and
 * `email`, which the listing gives an email address on its own.
 */
type ListedService = components["schemas"]["IdentityService"] | "email";

/** Screen words for each listed service. */
const LABELS: Record<ListedService, string> = {
  text_message: "Text Message",
  email: "Email",
  whatsapp: "WhatsApp",
};

function isListedService(service: string): service is ListedService {
  return Object.hasOwn(LABELS, service);
}

/** The screen word for a service; a value the map does not know shows as it is. */
export function serviceLabel(service: string): string {
  return isListedService(service) ? LABELS[service] : service;
}

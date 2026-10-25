/** Screen words for the services an identity is on, as `/v1` names them. */
const LABELS: Record<string, string> = {
  text_message: "Text Message",
  email: "Email",
  whatsapp: "WhatsApp",
};

/** The screen word for a service; a value the map does not know shows as it is. */
export function serviceLabel(service: string): string {
  return LABELS[service] ?? service;
}

import BrandedBubble from "./BrandedBubble";
import type { MessageBubbleProps } from "./chatBubbleShared";

export default function WhatsAppBubble(props: MessageBubbleProps) {
  return <BrandedBubble {...props} senderClassName="text-[var(--whatsapp-brand)]" />;
}

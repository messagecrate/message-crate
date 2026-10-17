import BrandedBubble from "./BrandedBubble";
import type { MessageBubbleProps } from "./chatBubbleShared";

export default function InstagramBubble(props: MessageBubbleProps) {
  return <BrandedBubble {...props} senderClassName="text-[var(--instagram-brand)]" />;
}

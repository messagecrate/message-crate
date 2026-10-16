import MessageAttachments from "../MessageAttachments";
import {
  type MessageBubbleProps,
  ServiceBubbleShell,
  ServiceMessageText,
  TapbackChips,
} from "./chatBubbleShared";

/**
 * A flat-row bubble whose sender name carries a service's brand color.
 *
 * `senderClassName` is the whole Tailwind class, written out in full by each
 * caller: Tailwind generates only the class names it finds in the source, so a
 * class built from a brand name at run time would have no style.
 */
export default function BrandedBubble({
  message,
  highlight,
  isActive,
  onAttachmentClick,
  senderClassName,
}: MessageBubbleProps & { senderClassName: string }) {
  return (
    <ServiceBubbleShell message={message} isActive={isActive} senderClassName={senderClassName}>
      <ServiceMessageText
        text={message.text || ""}
        highlight={highlight}
        mine={message.is_from_me}
      />

      <MessageAttachments message={message} onAttachmentClick={onAttachmentClick} />

      <TapbackChips
        message={message}
        className={`mt-1 ${message.is_from_me ? "justify-end" : "justify-start"}`}
      />
    </ServiceBubbleShell>
  );
}

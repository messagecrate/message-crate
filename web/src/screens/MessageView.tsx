import { useCallback, useDeferredValue, useMemo, useState } from "react";
import AttachmentLightbox from "../components/AttachmentLightbox";
import {
  type ContactPreview,
  contactPreviewFromConversationParticipants,
} from "../components/contactDrawer/contactDrawerTypes";
import SourcesPanel from "../components/SourcesPanel";
import { lightboxImages } from "../lib/lightboxImages";
import { useTimeZone } from "../lib/timeZone";
import type { Conversation, MessageAttachment } from "../lib/types";
import ConversationHeader from "./message/ConversationHeader";
import ConversationView from "./message/ConversationView";
import MessageFindBar from "./message/MessageFindBar";
import { conversationYears, useConversationMessages } from "./message/useConversationMessages";

/**
 * The conversation panel, the right-hand side of the Messages screen, shown
 * the way a phone shows a conversation (#1391). `openAt` opens it at one
 * message, highlighted, with the messages around it, for a search result
 * (#313); without it, the panel opens at the newest message. `openMatched`
 * names the earlier versions the search found that message by, when it found
 * it only by them, and the message opens them, highlighted (#1143).
 */
export default function MessageView({
  conversation,
  openAt = null,
  openMatched,
  onOpenContact,
}: {
  conversation: Conversation;
  openAt?: number | null;
  openMatched?: readonly number[];
  onOpenContact?: (contactId: string, preview: ContactPreview | null) => void;
}) {
  const conversationMessages = useConversationMessages(conversation.id, openAt, openMatched);
  const { messages, find } = conversationMessages;

  const [lightboxItems, setLightboxItems] = useState<MessageAttachment[] | null>(null);
  const [lightboxIndex, setLightboxIndex] = useState(0);
  const [showSources, setShowSources] = useState(false);

  const zone = useTimeZone();
  const years = useMemo(
    () => conversationYears(conversation.first_message_at, conversation.last_message_at, zone),
    [conversation.first_message_at, conversation.last_message_at, zone],
  );

  // Open the image viewer at the clicked photo. Previous/next walks the loaded messages' images.
  const handleAttachmentClick = useCallback(
    (att: MessageAttachment) => {
      const { items, index } = lightboxImages(messages, att);
      setLightboxItems(items);
      setLightboxIndex(index);
    },
    [messages],
  );

  /** Prefer list-API participants; fall back to the loaded page's conversation header. */
  const participants =
    conversation.participants.length > 0
      ? conversation.participants
      : (messages[0]?.conversation.participants ?? []);
  const displayParticipants = useMemo(
    () =>
      participants.map((p) => ({
        label: p.name,
        contact_id: p.contact_id == null ? null : String(p.contact_id),
      })),
    [participants],
  );

  // Re-highlighting a whole conversation is far more work than echoing a keystroke, so
  // the find bar stays on the term while the conversation trails on the deferred one.
  const deferredFindTerm = useDeferredValue(find.open ? find.term : "");

  return (
    <div className="flex h-full flex-col">
      <ConversationHeader
        conversation={conversation}
        displayParticipants={displayParticipants}
        years={years}
        findOpen={find.open}
        onToggleFind={() => (find.open ? find.close() : find.openFind())}
        onJumpToNewest={conversationMessages.jumpToNewest}
        onJumpToYear={(year) => void conversationMessages.jumpToYear(year)}
        onOpenContact={(contactId) => {
          onOpenContact?.(
            contactId,
            contactPreviewFromConversationParticipants(contactId, participants),
          );
        }}
        onShowSources={() => setShowSources(true)}
      />

      {find.open ? (
        <MessageFindBar
          findTerm={find.term}
          onFindTermChange={find.setTerm}
          matchCount={find.total}
          matchPosition={find.position}
          searching={find.searching}
          onPrevMatch={find.prevMatch}
          onNextMatch={find.nextMatch}
          onClose={find.close}
        />
      ) : null}

      <ConversationView
        messages={messages}
        loading={conversationMessages.loading}
        error={conversationMessages.error}
        findTerm={deferredFindTerm}
        highlightId={conversationMessages.highlightId}
        isGroup={conversation.is_group}
        hasOlder={conversationMessages.hasOlder}
        hasNewer={conversationMessages.hasNewer}
        loadingOlder={conversationMessages.loadingOlder}
        loadingNewer={conversationMessages.loadingNewer}
        onLoadOlder={conversationMessages.loadOlder}
        onLoadNewer={conversationMessages.loadNewer}
        landing={conversationMessages.landing}
        jumping={conversationMessages.jumping}
        onAttachmentClick={handleAttachmentClick}
      />

      {lightboxItems && (
        <AttachmentLightbox
          items={lightboxItems}
          currentIndex={lightboxIndex}
          onClose={() => setLightboxItems(null)}
          onPrev={() =>
            setLightboxIndex((i) => (i - 1 + lightboxItems.length) % lightboxItems.length)
          }
          onNext={() => setLightboxIndex((i) => (i + 1) % lightboxItems.length)}
        />
      )}

      {showSources && (
        <SourcesPanel conversationId={conversation.id} onClose={() => setShowSources(false)} />
      )}
    </div>
  );
}

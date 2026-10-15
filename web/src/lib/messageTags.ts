import { groupFromSlug, groupSlug } from "./contactGroups";
import {
  createNameCollection,
  useNameCollectionActions,
  useSetNamedSetMembers,
} from "./nameCollection";
import { keys } from "./queryKeys";
import reservedNames from "./reservedNames.json";
import { reservedError, reservedSet } from "./reservedSetNames";
import { forTag } from "./searchQuery";
import {
  createMessageTag,
  deleteMessageTag,
  listMessageTags,
  updateMessageTag,
  updateMessageTagMembers,
} from "./serverApi";

/**
 * Names that must not be created as Message Tags. The server's test writes
 * them to `reservedNames.json`, and fails when the file differs from the
 * names the server refuses.
 */
const RESERVED_TAG_NAMES = reservedSet(reservedNames.messageTag);

export function reservedTagError(name: string): string {
  return reservedError(reservedNames.messageTag, name);
}

export const messageTags = createNameCollection({
  routes: {
    list: listMessageTags,
    create: createMessageTag,
    update: updateMessageTag,
    remove: deleteMessageTag,
    updateMembers: updateMessageTagMembers,
  },
  key: keys.messageTags.all,
  chips: [{ key: keys.conversations.lists, field: "tags", shape: "pages" }],
  label: "tag",
  forName: forTag,
  reservedNames: RESERVED_TAG_NAMES,
  reservedError: reservedTagError,
});

export function isReservedTagName(name: string): boolean {
  return messageTags.isReserved(name);
}

// Slugs are URL syntax, not vocabulary — tags and groups share one rule.
export const tagSlug = groupSlug;
export const tagFromSlug = groupFromSlug;

/** Build the conversation-list query for a tag page plus optional typed search. */
export const tagListQuery = messageTags.listQuery;

/** Create, rename, delete, and set membership on Message Tags. */
export function useMessageTagActions() {
  return useNameCollectionActions(messageTags);
}

/** Put conversations in or out of one Message Tag, drawn before the server answers. */
export function useSetMessageTagMembers() {
  return useSetNamedSetMembers(messageTags);
}

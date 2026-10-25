import { groupFromSlug, groupSlug } from "./contactGroups";
import { createNameCollection } from "./nameCollection";
import { keys } from "./queryKeys";
import reservedNames from "./reservedNames.json";
import { forTag } from "./searchQuery";
import {
  createMessageTag,
  deleteMessageTag,
  listMessageTags,
  updateMessageTag,
  updateMessageTagMembers,
} from "./serverApi";

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
  forName: forTag,
  reserved: reservedNames.messageTag,
});

export function isReservedTagName(name: string): boolean {
  return messageTags.isReserved(name);
}

// Slugs are URL syntax, not vocabulary — tags and groups share one rule.
export const tagSlug = groupSlug;
export const tagFromSlug = groupFromSlug;

/** Build the conversation-list query for a tag page plus optional typed search. */
export const tagListQuery = messageTags.listQuery;

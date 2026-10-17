import { UNKNOWN_GROUP_LABEL } from "./unknownGroup";

/**
 * The words the sidebar and the toolbar menus use for Contact Groups and
 * Message Tags, in one place so the two surfaces never disagree.
 */

/** The words one sidebar section of named sets uses. */
export type NavEntityCopy = {
  /** Section id used for the collapse state. */
  id: string;
  /** Section heading, e.g. "Contact Groups". */
  title: string;
  /** Route prefix for one entity, e.g. "/group". */
  routeBase: string;
  /** Route for the "none of these" page, e.g. "/no-group". */
  emptyRoute: string;
  /**
   * A permanent row rendered above the list, for a collection the server
   * computes rather than the person curating. It cannot be renamed or
   * deleted, so it carries no options menu.
   */
  permanentRoute?: string;
  /** Label of the permanent row, e.g. "Unknown". */
  permanentLabel?: string;
  /** Label of the "none of these" row, e.g. "No group". */
  emptyLabel: string;
  /** Where a delete sends the user when they were on the deleted page. */
  fallbackRoute: string;
  addLabel: string;
  createTitle: string;
  renameTitle: string;
  namePlaceholder: string;
  /** Menu button label, completed with the entity name. */
  optionsLabel: (name: string) => string;
  /** What the delete confirmation says is removed and what is kept. */
  deleteBody: (name: string) => string;
  createError: string;
  renameError: string;
  deleteError: string;
};

export const CONTACT_GROUP_COPY: NavEntityCopy = {
  id: "contact-groups",
  title: "Contact Groups",
  routeBase: "/group",
  emptyRoute: "/no-group",
  emptyLabel: "No Contact Group",
  // Unknown is a permanent group the server computes from contact state: a
  // contact with no identity, or with identities and no preferred name. It
  // empties as the person names or links what is in it.
  permanentRoute: "/unknown",
  permanentLabel: UNKNOWN_GROUP_LABEL,
  fallbackRoute: "/contacts",
  addLabel: "Create Contact Group",
  createTitle: "Create Contact Group",
  renameTitle: "Rename Contact Group",
  namePlaceholder: "Contact Group name",
  optionsLabel: (name) => `Contact Group options for ${name}`,
  deleteBody: (name) =>
    `Removes the Contact Group ${name} and takes every contact out of it. The contacts themselves stay in your Message Crate.`,
  createError: "Could not create Contact Group",
  renameError: "Could not rename Contact Group",
  deleteError: "Could not delete Contact Group",
};

export const MESSAGE_TAG_COPY: NavEntityCopy = {
  id: "message-tags",
  title: "Message Tags",
  routeBase: "/tag",
  emptyRoute: "/no-tag",
  emptyLabel: "No Message Tag",
  fallbackRoute: "/",
  addLabel: "Create Message Tag",
  createTitle: "Create Message Tag",
  renameTitle: "Rename Message Tag",
  namePlaceholder: "Message Tag name",
  optionsLabel: (name) => `Message Tag options for ${name}`,
  deleteBody: (name) =>
    `Removes the Message Tag ${name} and takes it off every conversation that carries it. The conversations themselves stay in your Message Crate.`,
  createError: "Could not create Message Tag",
  renameError: "Could not rename Message Tag",
  deleteError: "Could not delete Message Tag",
};

/**
 * What the toolbar menu calls one kind of name. The words it shares with the
 * sidebar come from the sidebar's copy.
 */
export type GroupsMenuCopy = Pick<
  NavEntityCopy,
  "title" | "addLabel" | "createTitle" | "namePlaceholder" | "createError"
> & {
  searchPlaceholder: string;
  emptyText: string;
  noMatchText: string;
};

export const CONTACT_GROUP_MENU_COPY: GroupsMenuCopy = {
  ...CONTACT_GROUP_COPY,
  searchPlaceholder: "Search Contact Groups…",
  emptyText: "No Contact Groups",
  noMatchText: "No matching Contact Groups",
};

export const MESSAGE_TAG_MENU_COPY: GroupsMenuCopy = {
  ...MESSAGE_TAG_COPY,
  searchPlaceholder: "Search Message Tags…",
  emptyText: "No Message Tags",
  noMatchText: "No matching Message Tags",
};

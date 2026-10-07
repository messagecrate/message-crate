"use client";

import type { ContactDetail, GroupParticipant } from "@/lib/types";
import { inferHandleType } from "@/lib/handleKind";
import { useRouter } from "next/navigation";
import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import {
  draftHasName,
  emptyContactEditDraft,
  handlesForSave,
  seedContactEditDraft,
  type ContactEditDraft,
} from "./contactEdit";
import {
  contactFormAnchorFromRect,
  type ContactFormAnchor,
} from "./ContactFormOverlay";
import type { LabelCheckState } from "./LabelsMenu";

export type ParticipantContactSavedResult = {
  kind: "edit" | "create";
  contactId: number;
  contact?: ContactDetail;
};

export type UseParticipantContactFormOptions = {
  setStatus?: (message: string | null) => void;
  /** Extra label names always listed in the draft Labels menu (e.g. browse allLabels). */
  knownLabels?: string[];
  /** Defaults applied when creating a contact from a handle. */
  createDefaults?: { labels: string[] };
  /** Return true to ignore Escape (e.g. another modal is open). */
  shouldIgnoreEscape?: () => boolean;
  /** After successful create/edit (default: router.refresh). */
  onSaved?: (result: ParticipantContactSavedResult) => void;
};

export type ParticipantContactFormState = {
  formOpen: boolean;
  editDraft: ContactEditDraft | null;
  setEditDraft: Dispatch<SetStateAction<ContactEditDraft | null>>;
  formAnchor: ContactFormAnchor | null;
  contactCreating: boolean;
  editContactId: number | null;
  contactSaving: boolean;
  canSaveForm: boolean;
  draftMenuLabels: string[];
  draftLabelChecks: Record<string, LabelCheckState>;
  cancelContactForm: () => void;
  saveContactEdit: () => Promise<void>;
  saveContactCreate: () => Promise<void>;
  toggleDraftLabel: (name: string) => void;
  createAndAssignDraftLabel: (name: string) => void;
  clearDraftLabels: () => void;
  openEditContact: (id: number, anchor: ContactFormAnchor) => Promise<void>;
  /** Open edit with an already-seeded draft (e.g. browse detail card). */
  openEditFromDraft: (
    id: number,
    draft: ContactEditDraft,
    anchor: ContactFormAnchor | null,
  ) => void;
  openCreateContactWithHandle: (
    handle: string,
    anchor: ContactFormAnchor,
  ) => void;
  onParticipantClick: (
    participant: GroupParticipant,
    anchorRect: DOMRect,
  ) => void;
};

export function useParticipantContactForm(
  options: UseParticipantContactFormOptions,
): ParticipantContactFormState {
  const {
    setStatus,
    knownLabels = [],
    createDefaults,
    shouldIgnoreEscape,
    onSaved,
  } = options;

  const router = useRouter();
  const [editContactId, setEditContactId] = useState<number | null>(null);
  const [contactCreating, setContactCreating] = useState(false);
  const [editDraft, setEditDraft] = useState<ContactEditDraft | null>(null);
  const [formAnchor, setFormAnchor] = useState<ContactFormAnchor | null>(null);
  const [extraDraftLabels, setExtraDraftLabels] = useState<string[]>([]);
  const [contactSaving, setContactSaving] = useState(false);

  const formOpen = (editContactId != null || contactCreating) && !!editDraft;
  const canSaveForm =
    !!editDraft &&
    draftHasName(editDraft) &&
    handlesForSave(editDraft.handles).length > 0;

  const draftMenuLabels = useMemo(() => {
    const names = new Set([...knownLabels, ...extraDraftLabels]);
    for (const g of editDraft?.labels ?? []) names.add(g);
    return [...names].sort((a, b) =>
      a.localeCompare(b, undefined, { sensitivity: "base" }),
    );
  }, [knownLabels, extraDraftLabels, editDraft?.labels]);

  const draftLabelChecks = useMemo(() => {
    const result: Record<string, LabelCheckState> = {};
    const groups = editDraft?.labels ?? [];
    for (const name of draftMenuLabels) {
      result[name] = groups.includes(name) ? "on" : "off";
    }
    return result;
  }, [draftMenuLabels, editDraft?.labels]);

  const cancelContactForm = useCallback(() => {
    setEditContactId(null);
    setContactCreating(false);
    setEditDraft(null);
    setFormAnchor(null);
    setExtraDraftLabels([]);
  }, []);

  useEffect(() => {
    if (!formOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (shouldIgnoreEscape?.()) return;
      e.preventDefault();
      if (!contactSaving) cancelContactForm();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [formOpen, contactSaving, cancelContactForm, shouldIgnoreEscape]);

  const toggleDraftLabel = useCallback((name: string) => {
    setEditDraft((prev) => {
      if (!prev) return prev;
      const has = prev.labels.includes(name);
      const labels = has
        ? prev.labels.filter((g) => g !== name)
        : [...prev.labels, name].sort((a, b) =>
            a.localeCompare(b, undefined, { sensitivity: "base" }),
          );
      return { ...prev, labels };
    });
  }, []);

  const createAndAssignDraftLabel = useCallback((name: string) => {
    setExtraDraftLabels((prev) =>
      prev.includes(name) ? prev : [...prev, name],
    );
    setEditDraft((prev) => {
      if (!prev) return prev;
      if (prev.labels.includes(name)) return prev;
      return {
        ...prev,
        labels: [...prev.labels, name].sort((a, b) =>
          a.localeCompare(b, undefined, { sensitivity: "base" }),
        ),
      };
    });
  }, []);

  const clearDraftLabels = useCallback(() => {
    setEditDraft((prev) => (prev ? { ...prev, labels: [] } : prev));
  }, []);

  const finishSaved = useCallback(
    (result: ParticipantContactSavedResult) => {
      cancelContactForm();
      if (onSaved) onSaved(result);
      else router.refresh();
    },
    [cancelContactForm, onSaved, router],
  );

  const openEditContact = useCallback(
    async (id: number, anchor: ContactFormAnchor) => {
      setFormAnchor(anchor);
      setContactSaving(true);
      try {
        const res = await fetch(`/api/contacts/${id}`);
        const data = await res.json();
        if (!res.ok) throw new Error(data.error ?? "load failed");
        setExtraDraftLabels([]);
        setEditDraft(seedContactEditDraft(data.contact));
        setEditContactId(id);
        setContactCreating(false);
      } catch (err) {
        console.error(err);
        setFormAnchor(null);
        setStatus?.(
          err instanceof Error ? err.message : "Failed to load contact",
        );
      } finally {
        setContactSaving(false);
      }
    },
    [setStatus],
  );

  const openEditFromDraft = useCallback(
    (
      id: number,
      draft: ContactEditDraft,
      anchor: ContactFormAnchor | null,
    ) => {
      setFormAnchor(anchor);
      setExtraDraftLabels([]);
      setEditDraft(draft);
      setEditContactId(id);
      setContactCreating(false);
    },
    [],
  );

  const openCreateContactWithHandle = useCallback(
    (handle: string, anchor: ContactFormAnchor) => {
      setFormAnchor(anchor);
      setExtraDraftLabels([]);
      setEditContactId(null);
      setContactCreating(true);
      const draft = emptyContactEditDraft(createDefaults);
      const raw = handle.trim();
      setEditDraft({
        ...draft,
        handles: [
          { raw, handle_type: inferHandleType(raw) },
          { raw: "", handle_type: "phone" },
        ],
      });
    },
    [createDefaults],
  );

  const onParticipantClick = useCallback(
    (participant: GroupParticipant, anchorRect: DOMRect) => {
      if (contactSaving || formOpen) return;
      const anchor = contactFormAnchorFromRect(anchorRect);
      if (participant.contactId != null) {
        void openEditContact(participant.contactId, anchor);
        return;
      }
      openCreateContactWithHandle(participant.handle, anchor);
    },
    [
      contactSaving,
      formOpen,
      openEditContact,
      openCreateContactWithHandle,
    ],
  );

  const saveContactEdit = useCallback(async () => {
    if (!editDraft || editContactId == null) return;
    const id = editContactId;
    setContactSaving(true);
    try {
      const res = await fetch(`/api/contacts/${id}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          preferredName: editDraft.preferredName.trim() || null,
          handles: handlesForSave(editDraft.handles),
          labels: editDraft.labels,
        }),
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error ?? "save failed");
      finishSaved({
        kind: "edit",
        contactId: id,
        contact: data.contact as ContactDetail | undefined,
      });
    } catch (err) {
      console.error(err);
      setStatus?.(err instanceof Error ? err.message : "Save failed");
    } finally {
      setContactSaving(false);
    }
  }, [editDraft, editContactId, finishSaved, setStatus]);

  const saveContactCreate = useCallback(async () => {
    if (!editDraft || !draftHasName(editDraft)) return;
    setContactSaving(true);
    try {
      const res = await fetch("/api/contacts", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          preferredName: editDraft.preferredName.trim() || null,
          handles: handlesForSave(editDraft.handles),
          labels: editDraft.labels,
        }),
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error ?? "create failed");
      const contact = data.contact as ContactDetail | undefined;
      finishSaved({
        kind: "create",
        contactId: contact?.id ?? -1,
        contact,
      });
    } catch (err) {
      console.error(err);
      setStatus?.(err instanceof Error ? err.message : "Create failed");
    } finally {
      setContactSaving(false);
    }
  }, [editDraft, finishSaved, setStatus]);

  return {
    formOpen,
    editDraft,
    setEditDraft,
    formAnchor,
    contactCreating,
    editContactId,
    contactSaving,
    canSaveForm,
    draftMenuLabels,
    draftLabelChecks,
    cancelContactForm,
    saveContactEdit,
    saveContactCreate,
    toggleDraftLabel,
    createAndAssignDraftLabel,
    clearDraftLabels,
    openEditContact,
    openEditFromDraft,
    openCreateContactWithHandle,
    onParticipantClick,
  };
}

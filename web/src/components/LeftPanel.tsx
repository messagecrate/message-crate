import { useEffect, useLayoutEffect, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { apiErrorMessage } from "../lib/apiErrorMessage";
import { canUseImportExportWithProfile } from "../lib/desktopFeatures";
import { type SavedSearch, useSavedSearchActions, useSavedSearches } from "../lib/savedSearches";
import { isTauri } from "../lib/tauri-check";
import { resizeHandleGutterClass } from "../lib/tw";
import { useAccountProfile } from "../lib/useAccountProfile";
import { useContactGroups } from "../lib/useContactGroups";
import { useMessageTags } from "../lib/useMessageTags";
import { useWindowWidth } from "../lib/useWindowWidth";
import { type ImportAttention, useImportAttention } from "../screens/import/useImportAttention";
import type { NavItem } from "./appLayoutContext";
import ColumnResizeHandle from "./ColumnResizeHandle";
import { COLUMN_DIVIDER_CLASS } from "./columnDivider";
import { useReportColumnResizing } from "./columnResizeState";
import GroupsNav from "./GroupsNav";
import {
  ContactsIcon,
  ConversationsIcon,
  EllipsisIcon,
  ExportIcon,
  ImportIcon,
  SearchIcon,
  TrashIcon,
} from "./icons";
import { LIST_TOOLBAR_CLASS } from "./ListRangeHeader";
import {
  LEFT_PANEL_DEFAULT_WIDTH,
  LEFT_PANEL_MAX_WIDTH,
  LEFT_PANEL_MIN_WIDTH,
  LEFT_PANEL_STORAGE_KEY,
  LEFT_PANEL_WIDTH_VAR,
  leftPanelWindowMaxWidth,
} from "./leftPanelWidth";
import MessageTagsNav from "./MessageTagsNav";
import NavCollapsibleSection from "./NavCollapsibleSection";
import NavGlyphButton from "./NavGlyphButton";
import {
  NAV_LEADING_GLYPH_CLASS,
  NAV_LEADING_ROW_CLASS,
  NAV_NESTED_ROW_CLASS,
  navGlyphRowClass,
} from "./navSectionLayout";
import PlainButton from "./PlainButton";
import PopupMenu from "./PopupMenu";
import SavedSearchForm from "./SavedSearchForm";
import { useColumnResize } from "./useColumnResize";

/** The Import entry's badge: its word, and the sentence its tooltip reads. */
const IMPORT_BADGE: Record<ImportAttention, { label: string; title: string }> = {
  waiting: { label: "Waiting", title: "An import is waiting for your approval" },
  paused: { label: "Paused", title: "An import is paused and can be resumed" },
  failed: { label: "Failed", title: "The last import failed" },
};

/** Browse rows: same leading slot as section headings (no extra row padding). */
function browseLinkClass(active: boolean): string {
  return `${NAV_LEADING_ROW_CLASS} box-border w-full cursor-pointer rounded border-none px-0 py-1.5 text-left text-[0.875rem] text-text hover:bg-hover ${
    active ? "bg-hover font-semibold" : "bg-transparent font-normal"
  }`;
}

export default function LeftPanel({
  browseQuery,
  navItem,
  besideMinWidth,
}: {
  /**
   * The query the conversation list is showing right now, or "" when the
   * person is not looking at conversations. Export opens with it prefilled
   * and told it is a Conversations list query, so exporting the current view
   * is one click and the file holds the conversations that list showed.
   */
  browseQuery: string;
  /**
   * The item the route is on, which the panel highlights, as the route
   * declared it to the layout; null on a route with none, such as Settings.
   */
  navItem: NavItem | null;
  /**
   * The width the columns beside the panel need, dividers included, on a
   * screen with a list; not given on a screen without one. A narrow window
   * narrows the panel first, down to `LEFT_PANEL_MIN_WIDTH`, and below that
   * the area under the header scrolls sideways (#1722).
   */
  besideMinWidth?: number;
}) {
  const location = useLocation();
  const navigate = useNavigate();
  const { profile } = useAccountProfile();
  const canImport = canUseImportExportWithProfile(isTauri(), profile);
  // An account without the import permission has no Import Run to ask the server about.
  const importAttention = useImportAttention(canImport && profile?.can_import === true);
  const onDraggingChange = useReportColumnResizing();
  // A narrow window caps the panel below its stored width, which comes back
  // when the window is wide enough again.
  const windowMaxWidth = leftPanelWindowMaxWidth(useWindowWidth(), besideMinWidth);
  const { width, dragging, handleHover, handleProps } = useColumnResize({
    storageKey: LEFT_PANEL_STORAGE_KEY,
    defaultWidth: LEFT_PANEL_DEFAULT_WIDTH,
    minWidth: LEFT_PANEL_MIN_WIDTH,
    maxWidth: LEFT_PANEL_MAX_WIDTH,
    windowMaxWidth,
    onDraggingChange,
  });

  // Keep the header brand slot aligned with the nav while it resizes. Set
  // before the browser paints, so the slot's fallback, which knows nothing of
  // the panel's cap beside a list, never shows for a frame.
  useLayoutEffect(() => {
    document.documentElement.style.setProperty(LEFT_PANEL_WIDTH_VAR, `${width}px`);
  }, [width]);

  useEffect(() => {
    return () => {
      document.documentElement.style.removeProperty(LEFT_PANEL_WIDTH_VAR);
    };
  }, []);

  /** Whether `item` is the one the route is on; `aria-current` marks it for a screen reader. */
  const current = (item: NavItem) => navItem === item;
  const ariaCurrent = (item: NavItem) => (current(item) ? "page" : undefined);

  const { savedSearches: groups } = useSavedSearches();
  const savedSearchActions = useSavedSearchActions();
  const [showGroupForm, setShowGroupForm] = useState(false);
  const [editFor, setEditFor] = useState<SavedSearch | null>(null);
  // Why the open form's last save was refused, and why the last delete failed.
  // A delete has no dialog to show it in, so it is shown under the list.
  const [formError, setFormError] = useState<string | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const { groups: contactGroups } = useContactGroups();
  const { tags: messageTags } = useMessageTags();

  const createSavedSearch = async (name: string, query: string) => {
    setFormError(null);
    try {
      await savedSearchActions.create(name, query);
      setShowGroupForm(false);
    } catch (err) {
      setFormError(apiErrorMessage(err, "Could not create saved search"));
    }
  };

  const updateSavedSearch = async (id: number, name: string, query: string) => {
    setFormError(null);
    try {
      await savedSearchActions.update(id, name, query);
      setEditFor(null);
    } catch (err) {
      setFormError(apiErrorMessage(err, "Could not save saved search"));
    }
  };

  const removeSavedSearch = async (id: number) => {
    setDeleteError(null);
    try {
      await savedSearchActions.remove(id);
    } catch (err) {
      setDeleteError(apiErrorMessage(err, "Could not delete saved search"));
    }
  };

  return (
    <div
      style={{ flex: `0 0 ${width}px`, width: `${width}px` }}
      className={`relative flex h-full shrink-0 flex-col overflow-hidden ${COLUMN_DIVIDER_CLASS} bg-panel text-text`}
    >
      <div className={LIST_TOOLBAR_CLASS} aria-hidden />
      <div className={`min-h-0 flex-1 overflow-auto ${resizeHandleGutterClass}`}>
        {/* Browse */}
        <div className="px-3 py-2">
          <PlainButton
            className={browseLinkClass(current("messages"))}
            aria-current={ariaCurrent("messages")}
            onPress={() => navigate("/")}
          >
            <span className={NAV_LEADING_GLYPH_CLASS}>
              <ConversationsIcon size={15} />
            </span>
            Messages
          </PlainButton>
          <PlainButton
            className={browseLinkClass(current("contacts"))}
            aria-current={ariaCurrent("contacts")}
            onPress={() => navigate("/contacts")}
          >
            <span className={NAV_LEADING_GLYPH_CLASS}>
              <ContactsIcon size={15} />
            </span>
            Contacts
          </PlainButton>
          <PlainButton
            className={browseLinkClass(current("trash"))}
            aria-current={ariaCurrent("trash")}
            onPress={() => navigate("/trash")}
          >
            <span className={NAV_LEADING_GLYPH_CLASS}>
              <TrashIcon size={15} />
            </span>
            Trash
          </PlainButton>
        </div>

        {/* Import/Export — desktop app only */}
        {canImport && (
          <NavCollapsibleSection
            id="messages-import-export"
            title="Messages"
            headingActive={current("import") || current("export")}
          >
            <PlainButton
              onPress={() => navigate("/import")}
              aria-current={ariaCurrent("import")}
              className={`${navGlyphRowClass(current("import"))} cursor-pointer`}
            >
              <span className={NAV_NESTED_ROW_CLASS}>
                <span className={NAV_LEADING_GLYPH_CLASS}>
                  <ImportIcon size={15} />
                </span>
                <span className="truncate">Import</span>
                {importAttention ? (
                  <span
                    className={`ml-auto shrink-0 rounded-full px-1.5 text-[0.688rem] font-semibold leading-4 ${
                      importAttention === "failed"
                        ? "bg-danger text-sent-text"
                        : "bg-accent text-sent-text"
                    }`}
                    title={IMPORT_BADGE[importAttention].title}
                  >
                    {IMPORT_BADGE[importAttention].label}
                  </span>
                ) : null}
              </span>
            </PlainButton>
            <PlainButton
              onPress={() =>
                navigate(
                  browseQuery
                    ? `/export?q=${encodeURIComponent(browseQuery)}&list=conversations`
                    : "/export",
                )
              }
              aria-current={ariaCurrent("export")}
              className={`${navGlyphRowClass(current("export"))} cursor-pointer`}
            >
              <span className={NAV_NESTED_ROW_CLASS}>
                <span className={NAV_LEADING_GLYPH_CLASS}>
                  <ExportIcon size={15} />
                </span>
                <span className="truncate">Export</span>
              </span>
            </PlainButton>
          </NavCollapsibleSection>
        )}

        <GroupsNav groups={contactGroups} />

        {/* Named search queries stored on the server. Not contact membership. */}
        <NavCollapsibleSection
          id="saved-searches"
          title="Saved Searches"
          addLabel="Create saved search"
          onAdd={() => {
            setEditFor(null);
            setFormError(null);
            setShowGroupForm(true);
          }}
        >
          {groups.length === 0 ? (
            <div className={`${NAV_LEADING_ROW_CLASS} py-1.5 text-[0.813rem] text-muted`}>
              <span className={NAV_LEADING_GLYPH_CLASS} aria-hidden />
              <span>No saved searches</span>
            </div>
          ) : (
            groups.map((g) => {
              const active =
                location.pathname === "/" &&
                location.search === `?q=${encodeURIComponent(g.query)}`;
              return (
                <div key={g.id} className="relative w-full">
                  <div className={navGlyphRowClass(active)}>
                    <PlainButton
                      onPress={() => navigate(`/?q=${encodeURIComponent(g.query)}`)}
                      className={`${NAV_NESTED_ROW_CLASS} cursor-pointer border-none bg-transparent p-0 text-left text-inherit`}
                    >
                      <span className={NAV_LEADING_GLYPH_CLASS}>
                        <SearchIcon size={15} />
                      </span>
                      <span className="min-w-0 truncate">{g.name}</span>
                    </PlainButton>
                    <PopupMenu
                      trigger={
                        <NavGlyphButton
                          aria-label={`Saved search options for ${g.name}`}
                          onPress={() => setDeleteError(null)}
                          className={
                            active
                              ? ""
                              : "opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                          }
                        >
                          <EllipsisIcon size={15} />
                        </NavGlyphButton>
                      }
                      label={`Saved search options for ${g.name}`}
                      items={[
                        {
                          label: "Rename…",
                          onSelect: () => {
                            setShowGroupForm(false);
                            setFormError(null);
                            setEditFor(g);
                          },
                        },
                        {
                          label: "Delete",
                          onSelect: () => void removeSavedSearch(g.id),
                        },
                      ]}
                    />
                  </div>
                </div>
              );
            })
          )}
          {deleteError ? (
            <p role="alert" className="py-1.5 text-[0.813rem] text-danger">
              {deleteError}
            </p>
          ) : null}
        </NavCollapsibleSection>

        <MessageTagsNav tags={messageTags} />
      </div>

      {showGroupForm ? (
        <SavedSearchForm
          error={formError}
          busy={savedSearchActions.pending}
          onSave={createSavedSearch}
          onCancel={() => setShowGroupForm(false)}
        />
      ) : null}
      {editFor ? (
        <SavedSearchForm
          key={editFor.id}
          initial={{ name: editFor.name, query: editFor.query }}
          error={formError}
          busy={savedSearchActions.pending}
          onSave={(name, query) => updateSavedSearch(editFor.id, name, query)}
          onCancel={() => setEditFor(null)}
        />
      ) : null}

      <ColumnResizeHandle
        ariaLabel="Resize navigation panel"
        width={width}
        minWidth={LEFT_PANEL_MIN_WIDTH}
        maxWidth={Math.min(LEFT_PANEL_MAX_WIDTH, windowMaxWidth)}
        dragging={dragging}
        handleHover={handleHover}
        handleProps={handleProps}
      />
    </div>
  );
}

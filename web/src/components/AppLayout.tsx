import { useState } from "react";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import { type ContactBrowseTarget, contactBrowseQuery } from "../lib/contactBrowseQuery";
import { asMessagesLocationState } from "../lib/messagesLocationState";
import AppHeader from "./AppHeader";
import { AppLayoutContext, type AppLayoutContextValue, useSectionSlot } from "./appLayoutContext";
import { ColumnResizeProvider } from "./ColumnResizeContext";
import ContactDrawer from "./ContactDrawer";
import { COLUMN_DIVIDER_WIDTH } from "./columnDivider";
import type { ContactPreview } from "./contactDrawer/contactDrawerTypes";
import LeftPanel from "./LeftPanel";
import { LIST_COLUMN_MIN_WIDTH } from "./ListColumn";
import { RIGHT_PANE_MIN_WIDTH } from "./RightPane";
import { RightToolbarProvider } from "./RightToolbarContext";

/**
 * The width the list column and the right pane need beside the navigation
 * panel at their minimums, the list's divider included.
 */
const BESIDE_PANEL_MIN_WIDTH = LIST_COLUMN_MIN_WIDTH + COLUMN_DIVIDER_WIDTH + RIGHT_PANE_MIN_WIDTH;

/**
 * The frame around every route of the message shell: the header, the
 * navigation panel, and the contact panel over a conversation. Each route's
 * element in `App.tsx` (`BrowseRoutes.tsx`) renders the columns beside the
 * panel and declares what the header searches.
 */
export default function AppLayout() {
  const location = useLocation();
  const navigate = useNavigate();
  const section = useSectionSlot();
  const [selectedContact, setSelectedContact] = useState<ContactPreview | null>(null);

  const locationState = asMessagesLocationState(location.state);
  const openContactId = locationState?.openContactId ?? null;
  const openContactPreview = locationState?.openContactPreview ?? null;

  const closeContactDrawer = () => {
    setSelectedContact(null);
    if (!openContactId || !locationState) return;
    const { openContactId: _closed, openContactPreview: _preview, ...rest } = locationState;
    navigate(`${location.pathname}${location.search}`, {
      replace: true,
      state: Object.keys(rest).length > 0 ? rest : null,
    });
  };

  const browseContactConversations = (target: ContactBrowseTarget) => {
    const query = contactBrowseQuery(target);
    setSelectedContact(null);
    navigate(`/?q=${encodeURIComponent(query)}&f=${encodeURIComponent(query)}`);
  };

  const context: AppLayoutContextValue = {
    declareSection: section.declareSection,
    declareNavItem: section.declareNavItem,
    selectedContact,
    selectContact: setSelectedContact,
    closeContactDrawer,
    browseContactConversations,
  };

  // A route that declares no section has no list: the header offers no
  // search, and the navigation panel fits the window.
  const shown = section.shown;

  return (
    <RightToolbarProvider>
      <div className="flex h-screen flex-col bg-bg font-sans text-text">
        <AppHeader
          search={shown?.search ?? null}
          onSearchChange={section.onSearchChange}
          onSearch={section.onSearch}
        />
        <ColumnResizeProvider>
          {/* Each column keeps its own minimum width, so a window narrower
              than the three scrolls this row sideways rather than squeezing
              the list (#1722). A screen with no list fits the window.
              The header stays outside the row and does not scroll with it,
              so the search box stays in view; once the row has scrolled, the
              header's name slot no longer lines up with the navigation
              panel. That is accepted until the phone layout (#1722), because
              scrolling the header too would need the row's whole width
              summed for it and would move the search box out of view. */}
          <div className="flex min-h-0 flex-1 overflow-x-auto overflow-y-hidden">
            <LeftPanel
              browseQuery={shown?.browseQuery ?? ""}
              navItem={section.navItem}
              besideMinWidth={shown ? BESIDE_PANEL_MIN_WIDTH : undefined}
            />

            {/* The route's own columns, from its element in App.tsx. */}
            <AppLayoutContext.Provider value={context}>
              <Outlet />
            </AppLayoutContext.Provider>

            {/* Overlay contact panel (e.g. opened from a conversation). */}
            {openContactId ? (
              <ContactDrawer
                variant="overlay"
                contactId={openContactId}
                preview={openContactPreview}
                onClose={closeContactDrawer}
                onBrowseConversations={browseContactConversations}
              />
            ) : null}
          </div>
        </ColumnResizeProvider>
      </div>
    </RightToolbarProvider>
  );
}

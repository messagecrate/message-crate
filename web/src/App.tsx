import { Suspense } from "react";
import { Navigate, Route, Routes } from "react-router-dom";
import { ApiTokenRevealProvider } from "./components/ApiTokenRevealDialog";
import AppLayout from "./components/AppLayout";
import { AuthGuard } from "./components/AuthGuard";
import AuthShell from "./components/AuthShell";
import {
  ContactsRoute,
  ConversationsRoute,
  NoListRoute,
  OpenConversationRoute,
  TrashRoute,
} from "./components/BrowseRoutes";
import ImportExportRoute from "./components/ImportExportRoute";
import { useIsOwner } from "./hooks/useIsOwner";
import { useNeedsProfileSetup } from "./hooks/useNeedsProfileSetup";
import { useAuth } from "./lib/authContext";
import { lazyChunk } from "./lib/chunkLoad";
import { ThemeProvider } from "./lib/ThemeProvider";
import { TimeZoneProvider } from "./lib/TimeZoneProvider";
import LoginScreen from "./screens/LoginScreen";
import { SettingsScreen } from "./screens/lazySettingsScreen";
import OnboardingScreen from "./screens/OnboardingScreen";
import OwnerHome from "./screens/OwnerHome";
import { useMouseHistoryNavigation } from "./useMouseHistoryNavigation";

/**
 * Import and export only ever run in the desktop app, so their code — the
 * importer forms, the job runner and the Tauri bridge behind them — is split out
 * and never downloaded by a browser visiting the website build.
 */
const ImportScreen = lazyChunk("ImportScreen", () => import("./screens/ImportScreen"));
const ExportScreen = lazyChunk("ExportScreen", () => import("./screens/ExportScreen"));

/**
 * Trash is its own route and is not on the first paint path. Neither is
 * Settings, which Owner Home opens too (`lazySettingsScreen`).
 */
const TrashScreen = lazyChunk("TrashScreen", () => import("./screens/TrashScreen"));

/** Every route of the app; exported so tests drive the real route table. */
export function AppRoutes() {
  const { isAuthenticated } = useAuth();
  const { isOwner } = useIsOwner();
  const { needsSetup: needsOnboarding } = useNeedsProfileSetup();
  useMouseHistoryNavigation();

  // Where a logged-in visitor to the login screen should go next. Same order
  // the AuthGuard uses.
  const loggedInDestination = (
    <Navigate to={isOwner ? "/owner" : needsOnboarding ? "/onboarding" : "/"} replace />
  );

  return (
    <Routes>
      {/* Public routes — an authenticated visitor goes to loggedInDestination */}
      <Route path="/login" element={isAuthenticated ? loggedInDestination : <LoginScreen />} />
      {/* Registration is now the second tab of the login card, not its own screen. */}
      <Route path="/register" element={<Navigate to="/login" replace />} />
      {/* Owner Home, outside the AuthGuard's message shell: the owner holds
          no messages, so none of what that shell frames exists for them. */}
      <Route
        path="/owner/:section?/:accountId?"
        element={isAuthenticated && isOwner ? <OwnerHome /> : <Navigate to="/" replace />}
      />
      <Route
        path="/onboarding"
        element={
          isAuthenticated && needsOnboarding ? <OnboardingScreen /> : <Navigate to="/" replace />
        }
      />

      {/* Protected routes — AuthGuard redirects to /login or /onboarding */}
      <Route element={<AuthGuard />}>
        <Route
          element={
            <TimeZoneProvider>
              {/* Above Settings, so a new token's secret is shown even after
                  Settings is left; below the AuthGuard, so logging out drops it. */}
              <ApiTokenRevealProvider>
                <AppLayout />
              </ApiTokenRevealProvider>
            </TimeZoneProvider>
          }
        >
          {/* Each route's element renders the columns beside the navigation
              panel and tells the layout what the header searches and
              which navigation item to highlight. */}
          <Route index element={<ConversationsRoute />} />
          <Route path="tag/:slug" element={<ConversationsRoute />} />
          <Route path="no-tag" element={<ConversationsRoute tag="none" />} />
          <Route path="contacts" element={<ContactsRoute />} />
          <Route path="group/:slug" element={<ContactsRoute />} />
          <Route path="no-group" element={<ContactsRoute group="none" />} />
          <Route path="unknown" element={<ContactsRoute group="unknown" />} />
          <Route
            path="trash"
            element={
              <TrashRoute>
                <Suspense fallback={null}>
                  <TrashScreen />
                </Suspense>
              </TrashRoute>
            }
          />
          <Route
            path="import"
            element={
              <NoListRoute navItem="import">
                <ImportExportRoute feature="import">
                  <ImportScreen />
                </ImportExportRoute>
              </NoListRoute>
            }
          />
          <Route
            path="export"
            element={
              <NoListRoute navItem="export">
                <ImportExportRoute feature="export">
                  <ExportScreen />
                </ImportExportRoute>
              </NoListRoute>
            }
          />
          <Route
            path="settings"
            element={
              <NoListRoute>
                <Suspense fallback={null}>
                  <SettingsScreen />
                </Suspense>
              </NoListRoute>
            }
          />
          <Route path="messages/:conversationId" element={<OpenConversationRoute />} />
        </Route>
      </Route>

      {/* Catch-all */}
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}

export default function App() {
  return (
    <ThemeProvider>
      <AuthShell>
        <AppRoutes />
      </AuthShell>
    </ThemeProvider>
  );
}

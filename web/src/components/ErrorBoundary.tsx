import { Component, type ReactNode } from "react";
import { errorLine } from "../lib/apiErrorMessage";
import { reloadPage } from "../lib/pageReload";
import { reloadOnceForScreen, ScreenLoadError } from "../lib/screenLoad";
import Button from "./Button";

type Scope = "page" | "screen" | "panel";

type Props = {
  children: ReactNode;
  /**
   * What failed: the whole `page`; one `screen` beside the navigation panel,
   * which stays usable; or a `panel` that opens over a screen, such as
   * Advanced search.
   */
  scope: Scope;
  /**
   * The boundary shows its children again when this changes, such as the
   * address once the person picks another screen.
   */
  resetKey?: string;
};

type State = { failed: false } | { failed: true; error: unknown; reloading: boolean };

/**
 * What the person sees in place of a screen that threw while it rendered, or
 * whose code didn't load: what went wrong and a Reload button, rather than
 * the blank page React leaves when nothing catches the error.
 *
 * A screen that didn't load (`ScreenLoadError`) reloads the page once by
 * itself (`reloadOnceForScreen`), because the usual cause is a new release
 * that replaced the app's files under an open tab, and a reload fetches the
 * new ones. Nothing shows while that is decided.
 */
export default class ErrorBoundary extends Component<Props, State> {
  state: State = { failed: false };

  static getDerivedStateFromError(error: unknown): State {
    return { failed: true, error, reloading: error instanceof ScreenLoadError };
  }

  componentDidCatch(error: unknown): void {
    if (!(error instanceof ScreenLoadError)) return;
    void reloadOnceForScreen().then((reloaded) => {
      if (reloaded) return;
      this.setState((state) =>
        state.failed && state.error === error ? { ...state, reloading: false } : state,
      );
    });
  }

  componentDidUpdate(previous: Props): void {
    if (this.state.failed && previous.resetKey !== this.props.resetKey) {
      this.setState({ failed: false });
    }
  }

  render(): ReactNode {
    const { state } = this;
    if (!state.failed) return this.props.children;
    // The page may be reloading, so a message would only flash.
    if (state.reloading) return null;
    return <ErrorPanel error={state.error} scope={this.props.scope} />;
  }
}

const HEADING: Record<Scope, { failed: string; notLoaded: string }> = {
  page: { failed: "Something went wrong", notLoaded: "This page didn't load" },
  screen: { failed: "Something went wrong on this screen", notLoaded: "This screen didn't load" },
  panel: { failed: "Something went wrong in this panel", notLoaded: "This panel didn't load" },
};

const NEXT: Record<Scope, string> = {
  page: "Reload to try again.",
  screen: "Reload to try again, or pick another screen from the panel on the left.",
  panel: "Reload to try again.",
};

const FRAME: Record<Scope, string> = {
  page: "max-w-[700px] flex-1 p-6",
  screen: "max-w-[700px] flex-1 p-6",
  panel: "rounded-md border border-border bg-popover p-4",
};

function ErrorPanel({ error, scope }: { error: unknown; scope: Scope }) {
  const heading = HEADING[scope];
  return (
    <div role="alert" className={`${FRAME[scope]} font-sans text-text`}>
      <h2 className="m-0 mb-3">
        {error instanceof ScreenLoadError ? heading.notLoaded : heading.failed}
      </h2>
      <p className="m-0 mb-3 text-[0.875rem] text-danger">{errorLine(error)}</p>
      <p className="m-0 text-[0.875rem] text-muted">{NEXT[scope]}</p>
      <div className="mt-6">
        <Button variant="primary" size="wide" onPress={reloadPage}>
          Reload
        </Button>
      </div>
    </div>
  );
}

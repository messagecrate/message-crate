import { Component, type ReactNode } from "react";
import { errorLine } from "../lib/apiErrorMessage";
import { reloadPage } from "../lib/pageReload";
import { reloadOnceForScreen, ScreenLoadError } from "../lib/screenLoad";
import Button from "./Button";

type Props = {
  children: ReactNode;
  /**
   * What failed: one `screen` beside the navigation panel, which stays usable,
   * or the whole `page`.
   */
  scope: "screen" | "page";
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
 * itself, because the usual cause is a new release that replaced the app's
 * files under an open tab, and a reload fetches the new ones.
 */
export default class ErrorBoundary extends Component<Props, State> {
  state: State = { failed: false };

  static getDerivedStateFromError(error: unknown): State {
    return { failed: true, error, reloading: false };
  }

  componentDidCatch(error: unknown): void {
    if (error instanceof ScreenLoadError && reloadOnceForScreen()) {
      this.setState({ failed: true, error, reloading: true });
    }
  }

  componentDidUpdate(previous: Props): void {
    if (this.state.failed && previous.resetKey !== this.props.resetKey) {
      this.setState({ failed: false });
    }
  }

  render(): ReactNode {
    const { state } = this;
    if (!state.failed) return this.props.children;
    // The page is on its way out; a message would only flash.
    if (state.reloading) return null;
    return <ErrorPanel error={state.error} scope={this.props.scope} />;
  }
}

function ErrorPanel({ error, scope }: { error: unknown; scope: Props["scope"] }) {
  const heading =
    error instanceof ScreenLoadError
      ? "This screen didn't load"
      : scope === "screen"
        ? "Something went wrong on this screen"
        : "Something went wrong";
  const next =
    scope === "screen"
      ? "Reload to try again, or pick another screen from the panel on the left."
      : "Reload to try again.";
  return (
    <div role="alert" className="max-w-[700px] flex-1 p-6 font-sans text-text">
      <h2 className="m-0 mb-3">{heading}</h2>
      <p className="m-0 mb-3 text-[0.875rem] text-danger">{errorLine(error)}</p>
      <p className="m-0 text-[0.875rem] text-muted">{next}</p>
      <div className="mt-6">
        <Button variant="primary" size="wide" onPress={reloadPage}>
          Reload
        </Button>
      </div>
    </div>
  );
}

import React from "react";
import { I18nProvider } from "react-aria-components";
import ReactDOM from "react-dom/client";
import { HashRouter } from "react-router-dom";
import App from "./App";
import ErrorBoundary from "./components/ErrorBoundary";
import "./theme.css";

const rootEl = document.getElementById("root");
if (!rootEl) {
  throw new Error("Missing #root element");
}

ReactDOM.createRoot(rootEl).render(
  <React.StrictMode>
    {/* The last catch: an error that no screen's own boundary (AppLayout)
        caught shows what went wrong and a Reload button, not a blank page. */}
    <ErrorBoundary scope="page">
      <I18nProvider locale="en-US">
        <HashRouter>
          <App />
        </HashRouter>
      </I18nProvider>
    </ErrorBoundary>
  </React.StrictMode>,
);

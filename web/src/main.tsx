import React from "react";
import { I18nProvider } from "react-aria-components";
import ReactDOM from "react-dom/client";
import { HashRouter } from "react-router-dom";
import App from "./App";
import "./theme.css";

const rootEl = document.getElementById("root");
if (!rootEl) {
  throw new Error("Missing #root element");
}

ReactDOM.createRoot(rootEl).render(
  <React.StrictMode>
    <I18nProvider locale="en-US">
      <HashRouter>
        <App />
      </HashRouter>
    </I18nProvider>
  </React.StrictMode>,
);

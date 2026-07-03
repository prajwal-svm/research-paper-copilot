// Web entry (v5 platform-parity): the IDENTICAL component tree as desktop —
// only the platform adapter differs. The WASM core bridge installs itself
// via setWebInvoke; until a command is bridged, features degrade through
// the capability matrix, never silently.
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

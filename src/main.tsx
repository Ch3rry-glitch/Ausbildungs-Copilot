import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";
import "./themes.css";
import { startDarstellung } from "./theme";

// Farbthema/Glasstufe vor dem ersten Zeichnen setzen
startDarstellung();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

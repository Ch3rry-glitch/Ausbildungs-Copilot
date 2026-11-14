// Schlichte Linien-Icons (24er Raster), damit keine Icon-Bibliothek nötig ist.
import type { ReactElement } from "react";

const p = (d: string | string[]) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    {(Array.isArray(d) ? d : [d]).map((x, i) => (
      <path key={i} d={x} />
    ))}
  </svg>
);

export const Icon: Record<string, ReactElement> = {
  heute: p(["M12 3v2", "M12 19v2", "M4.2 4.2l1.4 1.4", "M18.4 18.4l1.4 1.4", "M3 12h2", "M19 12h2", "M12 8a4 4 0 1 0 0 8a4 4 0 0 0 0-8z"]),
  unterricht: p(["M4 19.5V5a2 2 0 0 1 2-2h12v16H6a2 2 0 0 0-2 2.5z", "M8 7h6", "M8 11h4"]),
  logbuch: p(["M9 3h6a1 1 0 0 1 1 1v2H8V4a1 1 0 0 1 1-1z", "M8 5H6a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2h-2", "M9 13l2 2l4-4"]),
  aufgaben: p(["M4 6h2", "M4 12h2", "M4 18h2", "M10 6h10", "M10 12h10", "M10 18h10"]),
  fragen: p(["M9.1 9a3 3 0 0 1 5.8 1c0 2-3 3-3 3", "M12 17h.01", "M12 3a9 9 0 1 0 0 18a9 9 0 0 0 0-18z"]),
  berichtsheft: p(["M3 7a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z", "M3 10h18", "M8 3v4", "M16 3v4"]),
  inbox: p(["M22 12h-6l-2 3h-4l-2-3H2", "M5.45 5.11L2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"]),
  pruefungen: p(["M12 2l3 7h7l-5.5 4.5L18.5 21L12 16.5L5.5 21l2-7.5L2 9h7z"]),
  notizen: p(["M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"]),
  einstellungen: p(["M12 15a3 3 0 1 0 0-6a3 3 0 0 0 0 6z", "M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33a1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"]),
  ordner: p(["M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"]),
  obsidian: p(["M12 2l7 5l-2 11l-5 4l-5-4L5 7z", "M12 2v20"]),
  refresh: p(["M21 12a9 9 0 1 1-2.64-6.36", "M21 3v6h-6"]),
  plus: p(["M12 5v14", "M5 12h14"]),
  ordnerOeffnen: p(["M5 19a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h4l2 2h7a2 2 0 0 1 2 2v1", "M3 19l3-8h16l-3 8z"]),
  datei: p(["M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z", "M14 3v5h5"]),
  sparkle: p(["M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8z"]),
};

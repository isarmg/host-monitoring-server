import { startAfterFonts } from "@xcss/web-fonts";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import "@xcss/design-tokens/tokens.css";
import "@xcss/design-tokens/tokens.dark.css";
import "@xcss/web-fonts/fonts.css";
import "@xcss/admin-ui/styles.css";
import "@xcss/design-tokens/reset.css";
import "@xcss/design-tokens/accessibility.css";
import "./host.css";

import App from "./App";

void startAfterFonts(() => {
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
});

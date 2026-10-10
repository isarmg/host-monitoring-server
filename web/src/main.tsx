import { startAfterFonts } from "@xcss/web/web-fonts";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import "@xcss/web/design-tokens/tokens.css";
import "@xcss/web/design-tokens/tokens.dark.css";
import "@xcss/web/web-fonts/fonts.css";
import "@xcss/web/admin-ui/styles.css";
import "@xcss/web/design-tokens/reset.css";
import "@xcss/web/design-tokens/accessibility.css";
import "./host.css";

import App from "./App";

void startAfterFonts(() => {
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
});

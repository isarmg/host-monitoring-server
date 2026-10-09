import { createXcssReactViteConfig } from "@xcss/web-toolchain/vite";
import { mergeConfig } from "vite";
import { foundationFontLicenses } from "./font-licenses.mjs";

export default mergeConfig(createXcssReactViteConfig(), { plugins: [foundationFontLicenses()] });

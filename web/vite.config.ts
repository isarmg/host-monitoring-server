import { createXcssReactViteConfig } from "@xcss/web/web-toolchain/vite";
import { mergeConfig } from "vite";
import { xcssFontLicenses } from "./font-licenses.mjs";

export default mergeConfig(createXcssReactViteConfig(), { plugins: [xcssFontLicenses()] });

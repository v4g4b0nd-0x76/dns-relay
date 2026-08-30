import { defineConfig, chromium } from "@playwright/test";
import { existsSync } from "node:fs";

const localChrome = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const useLocalChrome = !existsSync(chromium.executablePath()) && existsSync(localChrome);

export default defineConfig({
  webServer: {
    command: "npm run dev",
    url: "http://127.0.0.1:1420",
    reuseExistingServer: true,
  },
  use: useLocalChrome ? { launchOptions: { executablePath: localChrome } } : {},
});

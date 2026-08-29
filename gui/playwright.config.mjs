import { defineConfig, chromium } from "@playwright/test";
import { existsSync } from "node:fs";

const localChrome = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const useLocalChrome = !existsSync(chromium.executablePath()) && existsSync(localChrome);

export default defineConfig({
  use: useLocalChrome ? { launchOptions: { executablePath: localChrome } } : {},
});

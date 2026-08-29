import { copyFile, mkdir } from "node:fs/promises";

await mkdir("prototype/vendor", { recursive: true });
await copyFile(
  "node_modules/lucide/dist/umd/lucide.js",
  "prototype/vendor/lucide.min.js",
);

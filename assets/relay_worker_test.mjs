import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const source = readFileSync(new URL("./relay_worker.js", import.meta.url), "utf8");
const worker = await import(
  `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`
);

assert.equal(worker.subnetForIp("8.8.8.42"), "8.8.8.0/24");
assert.equal(worker.subnetForIp("192.168.1.2"), null);
assert.equal(worker.subnetForIp("2001:db8::1"), null);
assert.equal(worker.subnetForIp("not-an-ip"), null);

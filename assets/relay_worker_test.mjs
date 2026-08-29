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

function aResponse(...addresses) {
  const packet = [
    0x12, 0x34, 0x81, 0x80, 0, 1, 0, addresses.length, 0, 0, 0, 0,
    1, 0x61, 0, 0, 1, 0, 1,
  ];
  for (const address of addresses) {
    packet.push(
      0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4,
      ...address.split(".").map(Number),
    );
  }
  return new Uint8Array(packet);
}

function noDataResponse() {
  return aResponse();
}

assert.equal(worker.hasOnlyUnspecifiedAddresses(aResponse("0.0.0.0")), true);
assert.equal(
  worker.hasOnlyUnspecifiedAddresses(aResponse("0.0.0.0", "8.8.8.8")),
  false,
);
assert.equal(worker.hasOnlyUnspecifiedAddresses(aResponse("192.168.1.1")), false);
assert.equal(worker.hasOnlyUnspecifiedAddresses(noDataResponse()), false);

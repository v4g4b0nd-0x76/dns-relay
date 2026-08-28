const DOH_ENDPOINTS = [
  "https://cloudflare-dns.com/dns-query",
  "https://dns.google/dns-query",
  "https://dns.quad9.net/dns-query",
];
const MAX_DNS_PACKET_BYTES = 4096;
const CACHE_TTL_SECONDS = 60;
const UPSTREAM_TIMEOUT_MS = 3000;
let nextEndpoint = 0;

export default {
  async fetch(request, env) {
    if (request.method === "GET") {
      const url = new URL(request.url);
      if (url.searchParams.get("subnet") !== "1")
        return new Response("not found", { status: 404 });
      const subnet = subnetForIp(request.headers.get("cf-connecting-ip"));
      if (!subnet) return new Response("subnet unavailable", { status: 503 });
      return new Response(`${subnet}\n`, {
        headers: {
          "content-type": "text/plain; charset=utf-8",
          "cache-control": "no-store",
        },
      });
    }
    if (request.method !== "POST")
      return new Response("not found", { status: 404 });
    if (!env.RELAY_KEY) {
      console.error("RELAY_KEY secret is not configured");
      return new Response("relay unavailable", { status: 503 });
    }

    try {
      const encryptedQuery = new Uint8Array(await request.arrayBuffer());
      if (encryptedQuery.length > MAX_DNS_PACKET_BYTES + 28) {
        return new Response("payload too large", { status: 413 });
      }

      const relayKey = base64ToBytes(env.RELAY_KEY);
      const dnsQuery = await decodeFromRelay(encryptedQuery, relayKey);
      if (!isValidDnsQuery(dnsQuery))
        return new Response("bad request", { status: 400 });

      const cache = caches.default;
      const cacheRequest = await cacheRequestFor(dnsQuery, relayKey);
      let reply = await readCachedReply(cache, cacheRequest);
      if (!reply) {
        reply = await resolveUpstream(dnsQuery);
        // Caching is an optimization. A Cache API failure must not fail DNS.
        await cacheReply(cache, cacheRequest, reply).catch((err) => {
          console.warn("relay cache write failed", err);
        });
      }

      // The key excludes the DNS transaction ID, allowing cache reuse. Restore
      // the caller's ID before encryption so the response remains valid.
      const encryptedReply = await encodeForRelay(
        withTransactionId(reply, dnsQuery),
        relayKey,
      );
      return new Response(encryptedReply, {
        headers: { "content-type": "application/octet-stream" },
      });
    } catch (err) {
      console.error("worker relay failure", err);
      return new Response("upstream failed", { status: 502 });
    }
  },
};

export function subnetForIp(value) {
  if (!value) return null;
  const parts = value.split(".");
  if (parts.length !== 4) return null;
  const octets = parts.map(Number);
  if (
    octets.some(
      (octet, index) =>
        !Number.isInteger(octet) ||
        octet < 0 ||
        octet > 255 ||
        String(octet) !== parts[index],
    )
  )
    return null;

  const [a, b, c] = octets;
  if (
    a === 0 ||
    a === 10 ||
    a === 127 ||
    (a === 100 && (b & 0xc0) === 0x40) ||
    (a === 169 && b === 254) ||
    (a === 172 && b >= 16 && b <= 31) ||
    (a === 192 && b === 0) ||
    (a === 192 && b === 168) ||
    (a === 198 && (b & 0xfe) === 18) ||
    (a === 192 && b === 0 && c === 2) ||
    (a === 198 && b === 51 && c === 100) ||
    (a === 203 && b === 0 && c === 113) ||
    a >= 224
  )
    return null;
  return `${a}.${b}.${c}.0/24`;
}

function isValidDnsQuery(packet) {
  return packet && packet.length >= 12 && !(packet[2] & 0x80);
}

async function resolveUpstream(dnsQuery) {
  // A two-provider hedge replaces the old three-round, three-provider fan-out
  // (nine upstream requests per miss). The third provider is only a fallback.
  const start = nextEndpoint;
  nextEndpoint = (nextEndpoint + 1) % DOH_ENDPOINTS.length;
  const first = DOH_ENDPOINTS[start];
  const second = DOH_ENDPOINTS[(start + 1) % DOH_ENDPOINTS.length];
  const third = DOH_ENDPOINTS[(start + 2) % DOH_ENDPOINTS.length];

  try {
    return await Promise.any([
      queryDoh(first, dnsQuery),
      queryDoh(second, dnsQuery),
    ]);
  } catch {
    return queryDoh(third, dnsQuery);
  }
}

async function queryDoh(url, dnsQuery) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), UPSTREAM_TIMEOUT_MS);
  try {
    const response = await fetch(url, {
      method: "POST",
      headers: {
        "content-type": "application/dns-message",
        accept: "application/dns-message",
      },
      body: dnsQuery,
      signal: controller.signal,
    });
    if (!response.ok) throw new Error(`upstream returned ${response.status}`);

    const reply = new Uint8Array(await response.arrayBuffer());
    if (!isCacheableReply(reply))
      throw new Error("invalid upstream DNS response");
    return reply;
  } finally {
    clearTimeout(timeout);
  }
}

function isCacheableReply(packet) {
  if (!packet || packet.length < 12) return false;
  const flags = (packet[2] << 8) | packet[3];
  const isResponse = (flags & 0x8000) !== 0;
  const isTruncated = (flags & 0x0200) !== 0;
  const rcode = flags & 0x000f;
  return isResponse && !isTruncated && rcode !== 2; // SERVFAIL is transient.
}

async function cacheRequestFor(query, relayKey) {
  const canonical = query.slice();
  canonical[0] = 0;
  canonical[1] = 0;
  const key = await crypto.subtle.importKey(
    "raw",
    relayKey,
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const digest = new Uint8Array(
    await crypto.subtle.sign("HMAC", key, canonical),
  );
  return new Request(`https://relay-cache.invalid/v1/${bytesToHex(digest)}`);
}

async function readCachedReply(cache, request) {
  const cached = await cache.match(request);
  return cached ? new Uint8Array(await cached.arrayBuffer()) : null;
}

async function cacheReply(cache, request, reply) {
  const canonical = reply.slice();
  canonical[0] = 0;
  canonical[1] = 0;
  await cache.put(
    request,
    new Response(canonical, {
      headers: {
        "cache-control": `public, max-age=${CACHE_TTL_SECONDS}`,
        "content-type": "application/dns-message",
      },
    }),
  );
}

function withTransactionId(reply, query) {
  const out = reply.slice();
  out[0] = query[0];
  out[1] = query[1];
  return out;
}

async function importAesKey(rawKeyBytes) {
  return crypto.subtle.importKey("raw", rawKeyBytes, "AES-GCM", false, [
    "encrypt",
    "decrypt",
  ]);
}

async function decodeFromRelay(packet, rawKeyBytes) {
  if (packet.length < 28) return null;
  try {
    const key = await importAesKey(rawKeyBytes);
    const plaintext = await crypto.subtle.decrypt(
      { name: "AES-GCM", iv: packet.slice(0, 12) },
      key,
      packet.slice(12),
    );
    return new Uint8Array(plaintext);
  } catch {
    return null;
  }
}

async function encodeForRelay(plaintext, rawKeyBytes) {
  const key = await importAesKey(rawKeyBytes);
  const nonce = crypto.getRandomValues(new Uint8Array(12));
  const ciphertext = await crypto.subtle.encrypt(
    { name: "AES-GCM", iv: nonce },
    key,
    plaintext,
  );
  const out = new Uint8Array(12 + ciphertext.byteLength);
  out.set(nonce, 0);
  out.set(new Uint8Array(ciphertext), 12);
  return out;
}

function base64ToBytes(b64) {
  return Uint8Array.from(atob(b64), (char) => char.charCodeAt(0));
}

function bytesToHex(bytes) {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(
    "",
  );
}

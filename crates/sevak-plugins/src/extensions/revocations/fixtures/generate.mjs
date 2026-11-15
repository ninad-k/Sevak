// Deterministic TEST ONLY key. Never use it for production signing.
// Regenerate with: node crates/sevak-plugins/src/extensions/revocations/fixtures/generate.mjs
import { createHash, createPrivateKey, createPublicKey, sign } from "node:crypto";
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
const key = createPrivateKey({ format: "der", type: "pkcs8", key: Buffer.concat([
  Buffer.from("302e020100300506032b657004220420", "hex"), Buffer.alloc(32, 7),
]) });
const publicKey = createPublicKey(key).export({ format: "der", type: "spki" }).subarray(-32);
const keyId = Buffer.from("0102030405060708", "hex");
const encode = (value) => Buffer.from(value).toString("base64");
const write = (name, body) => writeFileSync(fileURLToPath(new URL(name, import.meta.url)), body);
write("public-key.txt", encode(`untrusted comment: INSECURE TEST KEY\n${encode(Buffer.concat([Buffer.from("Ed"), keyId, publicKey]))}\n`) + "\n");
const base = { format: 1, issued_at: 1700000000, expires_at: 1700086400, revocations: [
  { id: "docs", versions: ["1.0"], reason: "Test advisory: remove this version.", advisory_url: "https://github.com/ninad-k/Sevak/security/advisories/GHSA-test-only" },
  { id: "tool", versions: ["0.2.0"], reason: "Test revoked native version." },
  { kind: "theme", id: "nord", versions: ["*"], reason: "Test revoked theme." },
] };
for (const [name, data] of Object.entries({ valid: base, older: { ...base, issued_at: base.issued_at - 10 }, invalid: { ...base, unexpected: true } })) {
  const body = Buffer.from(JSON.stringify(data, null, 2) + "\n");
  const sig = sign(null, createHash("blake2b512").update(body).digest(), key);
  const comment = "timestamp:1700000000\tfile:fixture.json\tprehashed";
  const global = sign(null, Buffer.concat([sig, Buffer.from(comment)]), key);
  const packet = Buffer.concat([Buffer.from("ED"), keyId, sig]);
  const text = `untrusted comment: INSECURE TEST SIGNATURE\n${encode(packet)}\ntrusted comment: ${comment}\n${encode(global)}\n`;
  write(`${name}.json`, body);
  write(`${name}.json.sig`, encode(text) + "\n");
}

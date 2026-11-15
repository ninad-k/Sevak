// Deterministic TEST ONLY keys. Never use them for production signing.
// Regenerate with: node crates/sevak-plugins/src/index_signature/fixtures/generate.mjs
//
// Writes signed gallery indexes for release v1.2.3, as the release workflow
// signs them: the exact bytes, staged as `<tag>-<file>` so the Tauri signer's
// trusted comment says `file:v1.2.3-index.json`. The key is the same TEST key
// as ../../extensions/revocations/fixtures (so a test can show an advisory
// signature is not accepted as an index signature); `other-key` is a second
// TEST key the app's test trust does not know.
import { createHash, createPrivateKey, createPublicKey, sign } from "node:crypto";
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const testKey = (seed, id) => ({
  key: createPrivateKey({ format: "der", type: "pkcs8", key: Buffer.concat([
    Buffer.from("302e020100300506032b657004220420", "hex"), Buffer.alloc(32, seed),
  ]) }),
  keyId: Buffer.from(id, "hex"),
});
const trusted = testKey(7, "0102030405060708");
const other = testKey(9, "0909090909090909");
const encode = (value) => Buffer.from(value).toString("base64");
const write = (name, body) => writeFileSync(fileURLToPath(new URL(name, import.meta.url)), body);

const publicKey = createPublicKey(trusted.key).export({ format: "der", type: "spki" }).subarray(-32);
write("public-key.txt", encode(`untrusted comment: INSECURE TEST KEY\n${encode(Buffer.concat([Buffer.from("Ed"), trusted.keyId, publicKey]))}\n`) + "\n");

/** A Tauri-signer `.sig`: base64 of a prehashed minisign signature box. */
function signature(body, signedName, { key, keyId } = trusted) {
  const sig = sign(null, createHash("blake2b512").update(body).digest(), key);
  const comment = `timestamp:1700000000\tfile:${signedName}`;
  const global = sign(null, Buffer.concat([sig, Buffer.from(comment)]), key);
  const packet = Buffer.concat([Buffer.from("ED"), keyId, sig]);
  return encode(`untrusted comment: signature from tauri secret key\n${encode(packet)}\ntrusted comment: ${comment}\n${encode(global)}\n`);
}

const index = Buffer.from(JSON.stringify({
  format: 2,
  name: "Sevak gallery",
  entries: [{
    id: "fixture-search",
    kind: "workflow",
    name: "Fixture search",
    description: "A TEST entry of a signed index.",
    author: "Sevak",
    version: "1.0",
    source: "gallery/packages/fixture-search.zip",
    sha256: "15b9f3726c402ec97fe731952a651ad5054af2977636c36884b2186e5413e09f",
  }],
}, null, 2) + "\n");
const themes = Buffer.from(JSON.stringify({
  version: 2,
  themes: [{
    id: "fixture-dark",
    name: "Fixture Dark",
    author: "Sevak",
    description: "A TEST entry of a signed theme index.",
    mode: "dark",
    url: "gallery/themes/Fixture-Dark.toml",
    sha256: "43c6ac42334ad4774fc8a5c67a4cfee3886410bba30ceb9d22d763aec0f1718f",
  }],
}, null, 2) + "\n");

write("index.json", index);
write("themes.json", themes);
// What the release publishes.
write("index.json.sig", signature(index, "v1.2.3-index.json"));
write("themes.json.sig", signature(themes, "v1.2.3-themes.json"));
// Authentic bytes, but signed for something else: must all be refused.
write("index.json.other-key.sig", signature(index, "v1.2.3-index.json", other));
write("index.json.other-tag.sig", signature(index, "v1.2.2-index.json"));
write("index.json.as-themes.sig", signature(index, "v1.2.3-themes.json"));
write("index.json.unbound.sig", signature(index, "index.json"));

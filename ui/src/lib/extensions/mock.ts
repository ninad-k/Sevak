// In-memory data for `npm run dev` in a plain browser (no Rust backend). Mirrors
// what the commands in src-tauri/src/extensions.rs return. Never shipped: the
// production build drops the dynamic import in extensions-ipc.ts.

import type {
  Catalog,
  CatalogItem,
  InstalledItem,
  InstalledStatus,
  ItemKind,
  Overview,
  Result,
} from "./extensions-ipc";

const HASH = "9f2c4be1a07d63c85e1f4a7b2d90c3e8a6b5f41d7c2e9083b1a4d6f58e07c2b9";

function entry(over: Partial<CatalogItem> & Pick<CatalogItem, "id" | "kind" | "name">): CatalogItem {
  return {
    description: "",
    author: "Sevak",
    version: "1.0",
    tags: [],
    homepage: null,
    repository: null,
    license: null,
    permissions: [],
    min_sevak: null,
    source: `https://raw.githubusercontent.com/ninad-k/Sevak/v0.1.0/gallery/packages/${over.id}.zip`,
    sha256: HASH,
    platforms: [],
    theme_mode: null,
    state: "available",
    installed_version: null,
    unavailable: null,
    removable: false,
    ...over,
  };
}

const items: CatalogItem[] = [
  entry({
    id: "duckduckgo",
    kind: "workflow",
    name: "DuckDuckGo search",
    description: "Type ddg and some words to search DuckDuckGo. Runs no code.",
    tags: ["search", "no-code"],
  }),
  entry({
    id: "dev-search",
    kind: "workflow",
    name: "Developer search",
    description: "Keywords so, mdn, crate, docsrs, npm and pypi search the developer sites.",
    tags: ["search", "developer", "no-code"],
    version: "1.1",
  }),
  entry({
    id: "password-generator",
    kind: "plugin",
    name: "Password generator",
    description: "Random passwords, PINs and tokens (pw 24). Needs Python 3.",
    tags: ["needs-python"],
  }),
  entry({
    id: "rust-hello",
    kind: "native",
    name: "Rust hello",
    description: "Type rh and a name for greetings. An example written with sevak-extension-sdk.",
    author: "Sevak project",
    version: "0.1.0",
    license: "Apache-2.0",
    repository: "https://github.com/ninad-k/Sevak/tree/main/examples/rust-hello",
    homepage: "https://github.com/ninad-k/Sevak/tree/main/examples/rust-hello",
    permissions: [],
    min_sevak: "0.1.0",
    platforms: ["windows-x86_64", "macos-aarch64", "linux-x86_64"],
    source:
      "https://raw.githubusercontent.com/ninad-k/Sevak/v0.1.0/gallery/extensions/rust-hello/rust-hello-0.1.0-windows-x86_64.sevakext",
    tags: ["native"],
  }),
  entry({
    id: "weather-now",
    kind: "native",
    name: "Weather now",
    description: "Current weather for a city. Fetches from a public weather service.",
    author: "Example Author",
    version: "0.4.2",
    license: "MIT",
    repository: "https://github.com/example/weather-now",
    permissions: ["network"],
    min_sevak: "0.1.0",
    platforms: ["windows-x86_64", "linux-x86_64"],
    source:
      "https://raw.githubusercontent.com/ninad-k/Sevak/v0.1.0/gallery/extensions/weather-now/weather-now-0.4.2-windows-x86_64.sevakext",
    tags: ["native", "network"],
  }),
  entry({
    id: "future-tool",
    kind: "native",
    name: "Future tool",
    description: "Needs a newer Sevak than this one.",
    author: "Example Author",
    version: "2.0.0",
    license: "MIT",
    min_sevak: "9.0.0",
    platforms: ["windows-x86_64"],
    state: "unavailable",
    unavailable: "Needs Sevak 9.0.0 or newer; this is Sevak 0.1.0.",
    tags: ["native"],
  }),
  entry({
    id: "nord",
    kind: "theme",
    name: "Nord",
    description: "An arctic, north-bluish palette.",
    author: "Arctic Ice Studio",
    version: "",
    theme_mode: "dark",
    source: "https://raw.githubusercontent.com/ninad-k/Sevak/v0.1.0/gallery/themes/Nord.toml",
    tags: ["dark"],
  }),
  entry({
    id: "solarized-light",
    kind: "theme",
    name: "Solarized Light",
    description: "Precision colors for machines and people.",
    author: "Ethan Schoonover",
    version: "",
    theme_mode: "light",
    source:
      "https://raw.githubusercontent.com/ninad-k/Sevak/v0.1.0/gallery/themes/Solarized-Light.toml",
    tags: ["light"],
  }),
];

let installed: InstalledItem[] = [
  {
    id: "dev-search",
    kind: "workflow",
    name: "Developer search",
    folder: "dev-search",
    version: "1.0",
    author: "Sevak",
    installed_at: Math.floor(Date.now() / 1000) - 86400 * 3,
    update_to: "1.1",
    program_sha256: null,
    enabled: true,
    status: "ready",
    problem: null,
    keywords: ["so", "mdn", "crate"],
    can_review: false,
  },
];

let loaded = false;
let loadedAt = 0;

function catalogItems(): CatalogItem[] {
  return items.map((item) => {
    const mine = installed.find((i) => i.id === item.id);
    if (!mine) return { ...item };
    const newer = mine.update_to !== null;
    return {
      ...item,
      state: newer ? "update_available" : "installed",
      installed_version: mine.version,
      removable: true,
    };
  });
}

function catalog(): Catalog | null {
  if (!loaded) return null;
  return {
    source: "https://raw.githubusercontent.com/ninad-k/Sevak/v0.1.0/gallery/index.json",
    note: null,
    themes_error: null,
    skipped: 0,
    fetched_at: loadedAt,
    from_cache: false,
    items: catalogItems(),
  };
}

function snapshot(): Overview {
  return {
    installed: installed.map((i) => ({ ...i })),
    catalog: catalog(),
    platform: "windows-x86_64",
    plugins_folder: "C:\\Users\\you\\AppData\\Roaming\\sevak\\plugins",
  };
}

const wait = (ms = 250) => new Promise((resolve) => setTimeout(resolve, ms));

export async function overview(): Promise<Result<Overview>> {
  return { ok: true, value: snapshot() };
}

export async function refresh(): Promise<Result<Overview>> {
  await wait(500);
  loaded = true;
  loadedAt = Math.floor(Date.now() / 1000);
  return { ok: true, value: snapshot() };
}

function find(id: string): CatalogItem | undefined {
  return items.find((item) => item.id === id);
}

function statusFor(kind: ItemKind): InstalledStatus {
  return kind === "theme" ? "theme" : kind === "workflow" ? "ready" : "waiting";
}

export async function install(id: string): Promise<Result<Overview>> {
  await wait();
  const item = find(id);
  if (!item) return { ok: false, error: "That item is not in the loaded list. Load the list again." };
  if (item.state === "unavailable") return { ok: false, error: item.unavailable ?? "Not available." };
  installed = [
    {
      id: item.id,
      kind: item.kind,
      name: item.name,
      folder: item.id,
      version: item.version,
      author: item.author,
      installed_at: Math.floor(Date.now() / 1000),
      update_to: null,
      program_sha256: item.kind === "native" ? HASH : null,
      enabled: true,
      status: statusFor(item.kind),
      problem: null,
      keywords: item.kind === "theme" ? [] : [item.id.split("-")[0]],
      can_review: item.kind === "native" || item.kind === "plugin",
    },
    ...installed.filter((i) => i.id !== id),
  ];
  return { ok: true, value: snapshot() };
}

export async function update(id: string): Promise<Result<Overview>> {
  await wait();
  const item = find(id);
  installed = installed.map((i) =>
    i.id === id
      ? {
          ...i,
          version: item?.version ?? i.version,
          update_to: null,
          status: i.kind === "native" ? "waiting" : i.status,
          can_review: i.kind === "native",
        }
      : i,
  );
  return { ok: true, value: snapshot() };
}

export async function uninstall(id: string): Promise<Result<Overview | null>> {
  await wait(150);
  installed = installed.filter((i) => i.id !== id);
  return { ok: true, value: snapshot() };
}

export async function setEnabled(id: string, enabled: boolean): Promise<Result<Overview>> {
  installed = installed.map((i) =>
    i.id === id ? { ...i, enabled, status: enabled ? "ready" : "disabled" } : i,
  );
  return { ok: true, value: snapshot() };
}

export async function review(id: string): Promise<Result<boolean>> {
  installed = installed.map((i) =>
    i.id === id ? { ...i, status: "ready", can_review: false } : i,
  );
  return { ok: true, value: true };
}

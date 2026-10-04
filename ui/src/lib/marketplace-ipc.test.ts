import { afterEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

import { openMarketplace } from "./marketplace-ipc";

afterEach(() => {
  vi.unstubAllGlobals();
  invoke.mockReset();
});

describe("openMarketplace", () => {
  it("sends only the page word to the backend", async () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    invoke.mockResolvedValue(undefined);

    expect(await openMarketplace("submit")).toBeNull();

    expect(invoke).toHaveBeenCalledWith("open_marketplace", { target: "submit" });
  });

  it("returns the backend's error text", async () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    invoke.mockRejectedValue("could not open the browser");

    expect(await openMarketplace("browse")).toBe("could not open the browser");
  });

  it("does nothing in a plain browser during development", async () => {
    expect(await openMarketplace("guide")).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });
});

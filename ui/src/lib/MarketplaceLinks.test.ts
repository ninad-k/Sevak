// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("./marketplace-ipc", () => ({ openMarketplace: vi.fn() }));

import * as ipc from "./marketplace-ipc";
import MarketplaceLinks from "./MarketplaceLinks.svelte";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("MarketplaceLinks", () => {
  it("opens the marketplace and the submission form by name, never by address", async () => {
    vi.mocked(ipc.openMarketplace).mockResolvedValue(null);
    render(MarketplaceLinks);

    await fireEvent.click(screen.getByRole("button", { name: "Browse the marketplace" }));
    await fireEvent.click(screen.getByRole("button", { name: "Submit your extension" }));

    expect(ipc.openMarketplace).toHaveBeenNthCalledWith(1, "browse");
    expect(ipc.openMarketplace).toHaveBeenNthCalledWith(2, "submit");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("says so when the browser could not be opened", async () => {
    vi.mocked(ipc.openMarketplace).mockResolvedValue("no browser");
    render(MarketplaceLinks);

    await fireEvent.click(screen.getByRole("button", { name: "Browse the marketplace" }));

    await waitFor(() => expect(screen.getByRole("alert").textContent).toBe("no browser"));
  });
});

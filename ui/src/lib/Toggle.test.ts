// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import Toggle from "./Toggle.svelte";

afterEach(cleanup);

describe("Toggle", () => {
  it("is a switch named by its label and reflects `checked`", () => {
    render(Toggle, { label: "Launch at login", checked: true });
    const toggle = screen.getByRole("switch", { name: "Launch at login" });
    expect(toggle.getAttribute("aria-checked")).toBe("true");
  });

  it("flips and reports the new value on click", async () => {
    const onchange = vi.fn();
    render(Toggle, { label: "Blur", checked: false, onchange });
    const toggle = screen.getByRole("switch", { name: "Blur" });
    await fireEvent.click(toggle);
    expect(onchange).toHaveBeenLastCalledWith(true);
    expect(toggle.getAttribute("aria-checked")).toBe("true");
    await fireEvent.click(toggle);
    expect(onchange).toHaveBeenLastCalledWith(false);
    expect(toggle.getAttribute("aria-checked")).toBe("false");
  });

  it("does nothing while disabled", async () => {
    const onchange = vi.fn();
    render(Toggle, { label: "Locked", checked: false, disabled: true, onchange });
    const toggle = screen.getByRole("switch", { name: "Locked" });
    await fireEvent.click(toggle);
    expect(onchange).not.toHaveBeenCalled();
    expect(toggle.getAttribute("aria-checked")).toBe("false");
  });
});

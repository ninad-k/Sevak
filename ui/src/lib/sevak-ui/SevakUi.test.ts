// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import SevakUi from "./SevakUi.svelte";
import type { UiEventMessage, UiPayload, UiUpdate } from "./types";

afterEach(cleanup);

function payload(root: UiPayload["definition"]["root"], extra: Partial<UiPayload> = {}): UiPayload {
  return {
    id: "1",
    target: "launcher",
    workflow: "Demo",
    title: "Demo",
    width: 400,
    height: 300,
    definition: { state: { name: "Ada" }, root },
    images: {},
    vars: { who: "Bob" },
    query: "seed",
    scripts: ["up"],
    ...extra,
  };
}

const update = (patch: Partial<UiUpdate> = {}): UiUpdate => ({ set: {}, images: {}, close: false, ...patch });

describe("SevakUi", () => {
  it("fills placeholders from state, variables and the query", () => {
    render(SevakUi, {
      payload: payload({
        type: "column",
        children: [{ type: "label", text: "{state:name} / {var:who} / {query}" }],
      }),
      send: vi.fn(),
      onend: vi.fn(),
    });
    expect(screen.getByText("Ada / Bob / seed")).toBeTruthy();
  });

  it("an output action sends the state and the templated argument, then ends", async () => {
    const send = vi.fn(async (_event: UiEventMessage) => null);
    const onend = vi.fn();
    render(SevakUi, {
      payload: payload({
        type: "column",
        children: [
          { type: "textfield", id: "name", placeholder: "Name" },
          {
            type: "button",
            label: "Save",
            events: { click: [{ set: { done: "yes {state:name}" } }, { output: "save", arg: "{state:name}!" }] },
          },
        ],
      }),
      send,
      onend,
    });
    await fireEvent.input(screen.getByLabelText("Name"), { target: { value: "Eve" } });
    await fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(onend).toHaveBeenCalled());
    expect(send).toHaveBeenCalledWith({
      kind: "output",
      port: "save",
      arg: "Eve!",
      state: { name: "Eve", done: "yes Eve" },
    });
  });

  it("a run action sends the event and applies the script's update live", async () => {
    const send = vi.fn(async (event: UiEventMessage) =>
      event.kind === "run"
        ? update({ set: { out: "HELLO" }, root: { type: "label", text: "new {state:out}" } })
        : null,
    );
    render(SevakUi, {
      payload: payload({
        type: "column",
        children: [{ type: "button", id: "go", label: "Go", events: { click: [{ run: "up" }] } }],
      }),
      send,
      onend: vi.fn(),
    });
    await fireEvent.click(screen.getByRole("button", { name: "Go" }));
    await waitFor(() => expect(screen.getByText("new HELLO")).toBeTruthy());
    expect(send).toHaveBeenCalledWith({
      kind: "run",
      script: "up",
      input: { event: "click", component: "go", state: { name: "Ada" }, vars: { who: "Bob" } },
    });
  });

  it("shows a failing script as an alert and stays open", async () => {
    const send = vi.fn(async () => {
      throw "the program exited with code 3";
    });
    const onend = vi.fn();
    render(SevakUi, {
      payload: payload({ type: "button", label: "Go", events: { click: [{ run: "up" }] } }),
      send,
      onend,
    });
    await fireEvent.click(screen.getByRole("button", { name: "Go" }));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("exited with code 3"));
    expect(onend).not.toHaveBeenCalled();
  });

  it("filters a list by a textfield and Esc dismisses as closed", async () => {
    const send = vi.fn(async (_event: UiEventMessage) => null);
    const onend = vi.fn();
    render(SevakUi, {
      payload: payload({
        type: "column",
        children: [
          { type: "textfield", id: "q", placeholder: "Filter" },
          { type: "list", id: "l", filter_by: "q", items: [{ title: "Apple" }, { title: "Banana" }] },
        ],
      }),
      send,
      onend,
    });
    await fireEvent.input(screen.getByLabelText("Filter"), { target: { value: "bnn" } });
    expect(screen.queryByText("Apple")).toBeNull();
    expect(screen.getByText("Banana")).toBeTruthy();
    await fireEvent.keyDown(screen.getByLabelText("Filter"), { key: "Escape" });
    await waitFor(() => expect(onend).toHaveBeenCalled());
    expect(send.mock.calls[0][0].kind).toBe("close");
  });

  it("shows only the data: image the host loaded, never a remote one", () => {
    const { container } = render(SevakUi, {
      payload: payload(
        { type: "column", children: [{ type: "image", path: "a.png", alt: "A" }, { type: "image", path: "b.png", alt: "B" }] },
        { images: { "a.png": "data:image/png;base64,AAAA" } },
      ),
      send: vi.fn(),
      onend: vi.fn(),
    });
    const img = container.querySelector("img");
    expect(img?.getAttribute("src")).toBe("data:image/png;base64,AAAA");
    expect(container.querySelectorAll("img").length).toBe(1);
    expect(screen.getByText("B")).toBeTruthy();
  });
});

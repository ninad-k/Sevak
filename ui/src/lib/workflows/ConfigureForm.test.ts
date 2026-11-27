// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { WorkflowConfigForm } from "./ipc";
vi.mock("./ipc", () => ({ getWorkflowConfig: vi.fn(), setWorkflowConfig: vi.fn() }));
vi.mock("../settings-ipc", () => ({ pickFile: vi.fn(), pickDirectory: vi.fn() }));
import * as ipc from "./ipc";
import * as settings from "../settings-ipc";
import ConfigureForm from "./ConfigureForm.svelte";

const form = (over: Partial<WorkflowConfigForm> = {}): WorkflowConfigForm => ({
  folder: "docs",
  name: "Docs",
  fields: [
    { variable: "host", kind: "textfield", label: "Server", required: true },
    {
      variable: "units",
      kind: "popup",
      label: "Units",
      default: "metric",
      options: [
        { label: "Metric", value: "metric" },
        { label: "Imperial", value: "imperial" },
      ],
    },
    { variable: "volume", kind: "slider", label: "Volume", default: "40", min: 0, max: 100 },
    { variable: "loud", kind: "checkbox", label: "Shout" },
    { variable: "root", kind: "filepicker", label: "Folder", mode: "folder" },
  ],
  values: {},
  missing: ["Server"],
  ...over,
});

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(ipc.setWorkflowConfig).mockResolvedValue({ ok: true, value: null });
});
afterEach(cleanup);

async function open(value: WorkflowConfigForm, onclose = vi.fn()) {
  vi.mocked(ipc.getWorkflowConfig).mockResolvedValue({ ok: true, value });
  render(ConfigureForm, { folder: "docs", onclose });
  await screen.findByRole("heading", { name: "Configure Docs" });
  return onclose;
}

it("renders a control per field and starts from the defaults", async () => {
  await open(form());
  expect(screen.getByLabelText(/Server/)).toBeTruthy();
  expect((screen.getByLabelText("Units") as HTMLSelectElement).value).toBe("metric");
  expect((screen.getByLabelText("Volume") as HTMLInputElement).value).toBe("40");
  expect(screen.getByRole("switch", { name: "Shout" }).getAttribute("aria-checked")).toBe("false");
  expect(screen.getByLabelText("Folder")).toBeTruthy();
});

it("cannot save until the required fields are filled in", async () => {
  await open(form());
  const save = screen.getByRole("button", { name: "Save" });
  expect(save.hasAttribute("disabled")).toBe(true);
  expect(screen.getByText("Fill in Server to save.")).toBeTruthy();
  await fireEvent.input(screen.getByLabelText(/Server/), { target: { value: "h.example" } });
  await waitFor(() => expect(save.hasAttribute("disabled")).toBe(false));
});

it("saves typed values and changed controls, not the untouched defaults", async () => {
  const onclose = await open(form());
  await fireEvent.input(screen.getByLabelText(/Server/), { target: { value: "h.example" } });
  await fireEvent.click(screen.getByRole("switch", { name: "Shout" }));
  await fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(onclose).toHaveBeenCalledWith(true));
  expect(ipc.setWorkflowConfig).toHaveBeenCalledWith("docs", { host: "h.example", loud: true });
});

it("keeps what was saved before and sends a slider as a number", async () => {
  const onclose = await open(form({ values: { host: "old", volume: "55" }, missing: [] }));
  expect((screen.getByLabelText("Volume") as HTMLInputElement).value).toBe("55");
  await fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(onclose).toHaveBeenCalledWith(true));
  expect(ipc.setWorkflowConfig).toHaveBeenCalledWith("docs", { host: "old", volume: 55 });
});

it("shows the backend's refusal and stays open", async () => {
  vi.mocked(ipc.setWorkflowConfig).mockResolvedValue({ ok: false, error: "\"Volume\" must be between 0 and 100" });
  const onclose = await open(form({ missing: [], values: { host: "h" } }));
  await fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect((await screen.findByRole("alert")).textContent).toContain("must be between");
  expect(onclose).not.toHaveBeenCalled();
});

it("fills a file picker from the native dialog and closes on Cancel without saving", async () => {
  vi.mocked(settings.pickDirectory).mockResolvedValue("~/Projects");
  const onclose = await open(form({ missing: [], values: { host: "h" } }));
  await fireEvent.click(screen.getByRole("button", { name: "Browse…" }));
  await waitFor(() => expect((screen.getByLabelText("Folder") as HTMLInputElement).value).toBe("~/Projects"));
  await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(onclose).toHaveBeenCalledWith(false);
  expect(ipc.setWorkflowConfig).not.toHaveBeenCalled();
});

it("says so when the form cannot be loaded", async () => {
  vi.mocked(ipc.getWorkflowConfig).mockResolvedValue({ ok: false, error: "cannot read the workflow" });
  render(ConfigureForm, { folder: "docs", onclose: vi.fn() });
  expect((await screen.findByRole("alert")).textContent).toContain("cannot read the workflow");
});

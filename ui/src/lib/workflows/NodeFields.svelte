<script lang="ts">
  // The property form of one node: a field per entry of its kind in the
  // catalog. It never changes the node itself; every edit goes to `onset`, so
  // the editor that owns the workflow does the mutation.
  import "./workflows.css";
  import { kindOf, type FieldDef, type WfNode } from "./model";
  import { validateUiDefinition, type UiCheck } from "../sevak-ui/ipc";
  import { t, tHtml } from "../i18n.svelte";

  let {
    node,
    onset,
    folder = null,
  }: {
    node: WfNode;
    onset: (key: string, value: unknown) => void;
    /** The workflow's folder (for a UI's `file`), when it is saved. */
    folder?: string | null;
  } = $props();

  const def = $derived(kindOf(node.type));

  // Multi-line fields keep their own text while typing: "FOO" without an "="
  // yet is not a variable, and turning it into a value and back would erase it.
  // The editor remounts this form (`{#key}`) when another node is selected.
  // svelte-ignore state_referenced_locally
  const local = $state<Record<string, string>>(
    Object.fromEntries(
      kindOf(node.type)
        .fields.filter((f) => f.kind === "lines" || f.kind === "env" || f.kind === "json")
        .map((f) => [f.key, initialText(f, node)]),
    ),
  );

  function initialText(field: FieldDef, source: WfNode): string {
    const value = source[field.key];
    if (field.kind === "lines") return Array.isArray(value) ? value.join("\n") : "";
    if (field.kind === "json") {
      return value && typeof value === "object" && Object.keys(value).length > 0
        ? JSON.stringify(value, null, 2)
        : "";
    }
    if (value && typeof value === "object") {
      return Object.entries(value as Record<string, string>)
        .map(([name, text]) => `${name}=${text}`)
        .join("\n");
    }
    return "";
  }

  function setLines(field: FieldDef, text: string) {
    local[field.key] = text;
    onset(field.key, text === "" ? [] : text.split("\n"));
  }

  // A field that holds a JSON object (a UI's scripts): kept as typed, and
  // only a complete object is passed on.
  let jsonError = $state<Record<string, boolean>>({});
  function setJson(field: FieldDef, text: string) {
    local[field.key] = text;
    if (text.trim() === "") {
      jsonError[field.key] = false;
      onset(field.key, {});
      return;
    }
    try {
      const value = JSON.parse(text);
      if (value && typeof value === "object" && !Array.isArray(value)) {
        jsonError[field.key] = false;
        onset(field.key, value);
        return;
      }
    } catch {
      // fall through
    }
    jsonError[field.key] = true;
  }

  // The Validate button of a show_ui node.
  let check = $state<{ ok: true; value: UiCheck } | { ok: false; error: string } | null>(null);
  let checking = $state(false);
  async function validateUi() {
    checking = true;
    check = await validateUiDefinition({
      definition: typeof node.definition === "string" && node.definition.trim() ? node.definition : null,
      file: typeof node.file === "string" && node.file.trim() ? node.file : null,
      scripts: Object.keys((node.scripts as Record<string, unknown> | undefined) ?? {}),
      folder,
    });
    checking = false;
  }

  function setEnv(field: FieldDef, text: string) {
    local[field.key] = text;
    const env: Record<string, string> = {};
    for (const line of text.split("\n")) {
      const at = line.indexOf("=");
      if (at > 0) env[line.slice(0, at).trim()] = line.slice(at + 1);
    }
    onset(field.key, env);
  }

  function setNumber(field: FieldDef, text: string) {
    if (text.trim() === "") {
      onset(field.key, undefined);
      return;
    }
    const value = Number(text);
    if (Number.isFinite(value)) onset(field.key, Math.trunc(value));
  }

  // The rows of a list filter: each edit replaces the whole list, so the
  // editor that owns the workflow sees one change.
  type Row = { title: string; subtitle?: string; arg?: string };
  const rows = $derived<Row[]>(Array.isArray(node.items) ? (node.items as Row[]) : []);

  function setRow(index: number, key: keyof Row, value: string) {
    onset(
      "items",
      rows.map((row, i) => (i === index ? { ...row, [key]: value } : row)),
    );
  }

  function addRow() {
    onset("items", [...rows, { title: "", subtitle: "", arg: "" }]);
  }

  function removeRow(index: number) {
    onset(
      "items",
      rows.filter((_, i) => i !== index),
    );
  }

  const ACCEPTS = $derived([
    { value: "text", label: t("workflows.fields.acceptText") },
    { value: "url", label: t("workflows.fields.acceptUrl") },
    { value: "file", label: t("workflows.fields.acceptFile") },
  ]);

  function toggleAccepts(kind: string, on: boolean) {
    const current: string[] = Array.isArray(node.accepts) ? node.accepts : [];
    const next = on ? [...current.filter((k) => k !== kind), kind] : current.filter((k) => k !== kind);
    // Keep the order of the list above.
    onset(
      "accepts",
      ACCEPTS.map((a) => a.value).filter((k) => next.includes(k)),
    );
  }

  const id = (key: string) => `wf-field-${node.id}-${key}`;
</script>

{#each def.fields as field (field.key)}
  <div class="wf-field">
    {#if field.kind === "checkbox"}
      <label class="wf-check">
        <input
          type="checkbox"
          checked={node[field.key] === true}
          onchange={(e) => onset(field.key, e.currentTarget.checked)}
        />
        {field.label}
      </label>
    {:else if field.kind === "items"}
      <span class="wf-label" id={id(field.key)}>{field.label}</span>
      <div role="group" aria-labelledby={id(field.key)}>
        {#each rows as row, index (index)}
          <div class="wf-item">
            <input
              class="wf-input"
              type="text"
              spellcheck="false"
              autocomplete="off"
              aria-label={t("workflows.fields.itemTitle")}
              placeholder={t("workflows.fields.itemTitle")}
              value={row.title}
              oninput={(e) => setRow(index, "title", e.currentTarget.value)}
            />
            <input
              class="wf-input"
              type="text"
              spellcheck="false"
              autocomplete="off"
              aria-label={t("workflows.fields.itemSubtitle")}
              placeholder={t("workflows.fields.itemSubtitle")}
              value={row.subtitle ?? ""}
              oninput={(e) => setRow(index, "subtitle", e.currentTarget.value)}
            />
            <input
              class="wf-input mono"
              type="text"
              spellcheck="false"
              autocomplete="off"
              aria-label={t("workflows.fields.itemArg")}
              placeholder={t("workflows.fields.itemArg")}
              value={row.arg ?? ""}
              oninput={(e) => setRow(index, "arg", e.currentTarget.value)}
            />
            <button
              type="button"
              class="wf-btn small danger"
              aria-label={t("workflows.fields.removeItem", { n: index + 1 })}
              onclick={() => removeRow(index)}>{t("common.remove")}</button
            >
          </div>
        {/each}
        <button type="button" class="wf-btn small" onclick={addRow}>{t("workflows.fields.addItem")}</button>
      </div>
    {:else if field.kind === "accepts"}
      <span class="wf-label" id={id(field.key)}>{field.label}</span>
      <div role="group" aria-labelledby={id(field.key)}>
        {#each ACCEPTS as option (option.value)}
          <label class="wf-check">
            <input
              type="checkbox"
              checked={Array.isArray(node.accepts) && node.accepts.includes(option.value)}
              onchange={(e) => toggleAccepts(option.value, e.currentTarget.checked)}
            />
            {option.label}
          </label>
        {/each}
      </div>
    {:else}
      <label for={id(field.key)}>{field.label}</label>
      {#if field.kind === "text"}
        <input
          id={id(field.key)}
          class="wf-input"
          class:mono={field.mono}
          type="text"
          spellcheck="false"
          autocomplete="off"
          placeholder={field.placeholder ?? ""}
          value={node[field.key] ?? ""}
          oninput={(e) => onset(field.key, e.currentTarget.value)}
        />
      {:else if field.kind === "textarea"}
        <textarea
          id={id(field.key)}
          class="wf-input"
          class:mono={field.mono}
          rows="3"
          spellcheck="false"
          placeholder={field.placeholder ?? ""}
          value={node[field.key] ?? ""}
          oninput={(e) => onset(field.key, e.currentTarget.value)}
        ></textarea>
      {:else if field.kind === "number"}
        <input
          id={id(field.key)}
          class="wf-input"
          type="number"
          step="1"
          min={field.min}
          max={field.max}
          placeholder={field.placeholder ?? ""}
          value={node[field.key] ?? ""}
          oninput={(e) => setNumber(field, e.currentTarget.value)}
        />
      {:else if field.kind === "select"}
        <select
          id={id(field.key)}
          class="wf-input"
          value={node[field.key] ?? field.options?.[0]?.value}
          onchange={(e) => onset(field.key, e.currentTarget.value)}
        >
          {#each field.options ?? [] as option (option.value)}
            <option value={option.value}>{option.label}</option>
          {/each}
        </select>
      {:else if field.kind === "tristate"}
        <select
          id={id(field.key)}
          class="wf-input"
          value={node[field.key] === undefined ? "default" : String(node[field.key])}
          onchange={(e) => {
            const v = e.currentTarget.value;
            onset(field.key, v === "default" ? undefined : v === "true");
          }}
        >
          <option value="default">{t("workflows.fields.pasteDefault")}</option>
          <option value="true">{t("workflows.fields.yes")}</option>
          <option value="false">{t("workflows.fields.no")}</option>
        </select>
      {:else if field.kind === "lines"}
        <textarea
          id={id(field.key)}
          class="wf-input"
          class:mono={field.mono}
          rows="3"
          spellcheck="false"
          placeholder={field.placeholder ?? ""}
          value={local[field.key] ?? ""}
          oninput={(e) => setLines(field, e.currentTarget.value)}
        ></textarea>
      {:else if field.kind === "json"}
        <textarea
          id={id(field.key)}
          class="wf-input mono"
          rows="6"
          spellcheck="false"
          aria-invalid={jsonError[field.key] === true}
          placeholder={'{"up": {"language": "shell", "script": "echo {}"}}'}
          value={local[field.key] ?? ""}
          oninput={(e) => setJson(field, e.currentTarget.value)}
        ></textarea>
        {#if jsonError[field.key]}<p class="wf-hint" role="alert">{t("workflows.fields.jsonInvalid")}</p>{/if}
      {:else if field.kind === "env"}
        <textarea
          id={id(field.key)}
          class="wf-input mono"
          rows="3"
          spellcheck="false"
          placeholder="NAME=value"
          value={local[field.key] ?? ""}
          oninput={(e) => setEnv(field, e.currentTarget.value)}
        ></textarea>
      {/if}
    {/if}
    {#if field.hint}<p class="wf-hint">{field.hint}</p>{/if}
  </div>
{/each}
{#if node.type === "show_ui"}
  <div class="wf-field">
    <button type="button" class="wf-btn" disabled={checking} onclick={validateUi}>
      {t("workflows.fields.validateUi")}
    </button>
    {#if check?.ok}
      <p class="wf-hint" role="status">
        {t("workflows.fields.uiValid", {
          components: check.value.components,
          ports: check.value.ports.join(", "),
        })}
        {#each check.value.warnings as warning, i (i)}<br />{warning}{/each}
      </p>
    {:else if check}
      <p class="wf-hint" role="alert">{check.error}</p>
    {/if}
  </div>
{/if}
{#if def.fields.length === 0}
  <p class="wf-hint">{@html tHtml("workflows.fields.noSettings", { id: node.id })}</p>
{/if}

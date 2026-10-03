<script lang="ts">
  // The property form of one node: a field per entry of its kind in the
  // catalog. It never changes the node itself; every edit goes to `onset`, so
  // the editor that owns the workflow does the mutation.
  import "./workflows.css";
  import { kindOf, type FieldDef, type WfNode } from "./model";

  let {
    node,
    onset,
  }: {
    node: WfNode;
    onset: (key: string, value: unknown) => void;
  } = $props();

  const def = $derived(kindOf(node.type));

  // Multi-line fields keep their own text while typing: "FOO" without an "="
  // yet is not a variable, and turning it into a value and back would erase it.
  // The editor remounts this form (`{#key}`) when another node is selected.
  // svelte-ignore state_referenced_locally
  const local = $state<Record<string, string>>(
    Object.fromEntries(
      kindOf(node.type)
        .fields.filter((f) => f.kind === "lines" || f.kind === "env")
        .map((f) => [f.key, initialText(f, node)]),
    ),
  );

  function initialText(field: FieldDef, source: WfNode): string {
    const value = source[field.key];
    if (field.kind === "lines") return Array.isArray(value) ? value.join("\n") : "";
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

  const ACCEPTS = [
    { value: "text", label: "Text" },
    { value: "url", label: "Links" },
    { value: "file", label: "Files and folders" },
  ];

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
          <option value="default">Use the [paste] setting</option>
          <option value="true">Yes</option>
          <option value="false">No</option>
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
{#if def.fields.length === 0}
  <p class="wf-hint">
    This trigger has no settings. Start it with <code>sevak --trigger &lt;folder&gt;/{node.id}</code>
    (add some text after it to pass it in as <code>{"{query}"}</code>).
  </p>
{/if}

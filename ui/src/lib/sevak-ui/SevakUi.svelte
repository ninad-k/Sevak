<script lang="ts">
  // Renders a Sevak UI (see docs/sevak-ui.md). The definition was parsed,
  // expanded and validated by Rust; this draws it and runs its events.
  //
  // Security: the only `{@html}` in the UI is inside Markdown.svelte (the
  // sanitising renderer). Images use the `data:` URLs the host loaded. There is
  // no web view, no navigation and no remote content here by design.
  import { tick } from "svelte";
  import Markdown from "../Markdown.svelte";
  import { t } from "../i18n.svelte";
  import {
    filterItems,
    initialState,
    itemValue,
    progressValue,
    resolveValue,
    show,
    substitute,
    type Scope,
    type UiState,
  } from "./runtime";
  import type {
    EventName,
    UiAction,
    UiComponent,
    UiEventMessage,
    UiEvents,
    UiPayload,
    UiUpdate,
  } from "./types";

  let {
    payload,
    send,
    onend,
  }: {
    payload: UiPayload;
    /** Delivers an event to the shell; `run` answers with the script's update. */
    send: (event: UiEventMessage) => Promise<UiUpdate | null>;
    /** The UI is over (it closed itself or continued the workflow). */
    onend: () => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let root = $state<UiComponent>(payload.definition.root);
  // svelte-ignore state_referenced_locally
  let images = $state<Record<string, string>>({ ...payload.images });
  // svelte-ignore state_referenced_locally
  let vals = $state<UiState>(
    Object.fromEntries(
      // Starting text may use {query} and {var:name}.
      Object.entries(initialState(payload.definition)).map(([id, value]) => [
        id,
        typeof value === "string" ? substitute(value, { state: {}, vars: payload.vars, query: payload.query }) : value,
      ]),
    ),
  );
  let busy = $state(0);
  let problem = $state<string | null>(null);
  let ended = false;
  let host: HTMLElement | undefined = $state();

  const scope = (): Scope => ({ state: vals, vars: payload.vars, query: payload.query });
  const text = (value: string) => substitute(value, scope());

  /** The state as plain JSON data. */
  const snapshot = (): Record<string, unknown> => $state.snapshot(vals) as Record<string, unknown>;

  $effect(() => {
    // Focus the first control so the keyboard works at once.
    void tick().then(() => {
      host?.querySelector<HTMLElement>("input, textarea, select, button, [tabindex='0']")?.focus();
    });
  });

  async function finish(message: UiEventMessage) {
    if (ended) return;
    ended = true;
    try {
      await send(message);
    } catch (err) {
      console.warn("[sevak-ui] could not send:", err);
    }
    onend();
  }

  /** Esc, or leaving: the workflow continues from `closed`. */
  export function dismiss() {
    void finish({ kind: "close", state: snapshot() });
  }

  async function apply(update: UiUpdate) {
    for (const [id, value] of Object.entries(update.set ?? {})) vals[id] = value;
    if (update.root) {
      root = update.root;
      images = { ...images, ...update.images };
      // Inputs the new tree adds start from their defaults.
      const fresh = initialState({ root: update.root });
      for (const [id, value] of Object.entries(fresh)) if (!(id in vals)) vals[id] = value;
    }
    if (update.notify) await send({ kind: "notify", text: update.notify }).catch(() => null);
    if (update.close) dismiss();
  }

  async function perform(action: UiAction, event: EventName, component: string) {
    if (action.set) {
      for (const [id, value] of Object.entries(action.set)) vals[id] = resolveValue(value, scope());
    } else if (action.close) {
      dismiss();
    } else if (action.output) {
      await finish({
        kind: "output",
        port: action.output,
        arg: action.arg === undefined ? null : text(action.arg),
        state: snapshot(),
      });
    } else if (action.run) {
      busy += 1;
      problem = null;
      try {
        const update = await send({
          kind: "run",
          script: action.run,
          input: { event, component, state: snapshot(), vars: payload.vars },
        });
        if (update) await apply(update);
      } catch (err) {
        problem = t("sevakUi.scriptFailed", { message: typeof err === "string" ? err : String(err) });
      } finally {
        busy -= 1;
      }
    } else if (action.notify !== undefined) {
      await send({ kind: "notify", text: text(action.notify) }).catch(() => null);
    } else if (action.copy !== undefined) {
      await send({ kind: "copy", text: text(action.copy) }).catch(() => null);
    }
  }

  async function fire(events: UiEvents | undefined, event: EventName, component: string) {
    const actions = events?.[event] ?? [];
    for (const action of actions) {
      if (ended) return;
      await perform(action, event, component);
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape" && !e.defaultPrevented) {
      e.preventDefault();
      e.stopPropagation();
      dismiss();
    }
  }

  const gapOf = (c: { gap?: number }) => `${c.gap ?? 8}px`;
  const padOf = (c: { padding?: number }) => `${c.padding ?? 0}px`;
  const alignOf = (c: { align?: string }) =>
    ({ start: "flex-start", center: "center", end: "flex-end", stretch: "stretch" })[c.align ?? "stretch"];

  function toggleListItem(c: Extract<UiComponent, { type: "list" }>, value: string) {
    if (c.multi) {
      const current = Array.isArray(vals[c.id]) ? (vals[c.id] as string[]) : [];
      vals[c.id] = current.includes(value) ? current.filter((v) => v !== value) : [...current, value];
    } else {
      vals[c.id] = value;
    }
    void fire(c.events, "select", c.id);
  }

  const isSelected = (id: string, value: string) =>
    Array.isArray(vals[id]) ? (vals[id] as string[]).includes(value) : vals[id] === value;
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section
  class="sui"
  class:window={payload.target === "window"}
  
  aria-label={payload.title}
  aria-busy={busy > 0}
  bind:this={host}
  onkeydown={onKey}
>
  {#snippet node(c: UiComponent)}
    {#if c.type === "column" || c.type === "row" || c.type === "grid"}
      <div
        class="box {c.type}"
        style:gap={gapOf(c)}
        style:padding={padOf(c)}
        style:align-items={c.type === "grid" ? undefined : alignOf(c)}
        style:grid-template-columns={c.type === "grid"
          ? `repeat(${c.columns ?? 1}, minmax(0, 1fr))`
          : undefined}
      >
        {#each c.children as child, i (i)}{@render node(child)}{/each}
      </div>
    {:else if c.type === "label"}
      <p class="label {c.style ?? 'body'}">{text(c.text)}</p>
    {:else if c.type === "markdown"}
      <Markdown source={text(c.text)} compact />
    {:else if c.type === "button"}
      <button
        type="button"
        class="btn {c.style ?? 'secondary'}"
        disabled={c.disabled === true}
        onclick={() => fire(c.events, "click", c.id ?? "")}>{text(c.label)}</button
      >
    {:else if c.type === "checkbox"}
      <label class="check">
        <input
          type="checkbox"
          checked={vals[c.id] === true}
          onchange={(e) => {
            vals[c.id] = e.currentTarget.checked;
            void fire(c.events, "change", c.id);
          }}
        />
        <span>{text(c.label)}</span>
      </label>
    {:else if c.type === "textfield" || c.type === "passwordfield"}
      <input
        class="field"
        type={c.type === "passwordfield" ? "password" : "text"}
        aria-label={c.placeholder || c.id}
        placeholder={c.placeholder ? text(c.placeholder) : ""}
        autocomplete="off"
        spellcheck="false"
        value={show(vals[c.id])}
        oninput={(e) => {
          vals[c.id] = e.currentTarget.value;
          void fire(c.events, "change", c.id);
        }}
        onkeydown={(e) => {
          if (e.key === "Enter" && !e.isComposing) {
            e.preventDefault();
            void fire(c.events, "submit", c.id);
          }
        }}
      />
    {:else if c.type === "textarea"}
      <textarea
        class="field"
        rows={c.rows ?? 4}
        aria-label={c.placeholder || c.id}
        placeholder={c.placeholder ? text(c.placeholder) : ""}
        spellcheck="false"
        value={show(vals[c.id])}
        oninput={(e) => {
          vals[c.id] = e.currentTarget.value;
          void fire(c.events, "change", c.id);
        }}
      ></textarea>
    {:else if c.type === "picker"}
      <select
        class="field"
        aria-label={c.id}
        value={show(vals[c.id])}
        onchange={(e) => {
          vals[c.id] = e.currentTarget.value;
          void fire(c.events, "change", c.id);
        }}
      >
        {#each c.options as option (option.value)}
          <option value={option.value}>{text(option.label)}</option>
        {/each}
      </select>
    {:else if c.type === "slider"}
      <input
        class="slider"
        type="range"
        aria-label={c.id}
        min={c.min}
        max={c.max}
        step={c.step ?? 1}
        value={Number(vals[c.id] ?? c.min)}
        oninput={(e) => {
          vals[c.id] = e.currentTarget.valueAsNumber;
          void fire(c.events, "change", c.id);
        }}
      />
    {:else if c.type === "progress"}
      {@const fraction = c.indeterminate ? null : progressValue(c.value, scope())}
      {#if fraction === null}
        <progress class="bar" aria-label={t("sevakUi.progress")}></progress>
      {:else}
        <progress class="bar" max="1" value={fraction} aria-label={t("sevakUi.progress")}></progress>
      {/if}
    {:else if c.type === "image"}
      {#if images[c.path]}
        <img class="img" src={images[c.path]} alt={c.alt ?? ""} />
      {:else}
        <p class="label caption">{c.alt || t("sevakUi.imageMissing")}</p>
      {/if}
    {:else if c.type === "list"}
      {@const filter = c.filter_by ? show(vals[c.filter_by]) : ""}
      <ul class="list" role="listbox" aria-label={c.id} aria-multiselectable={c.multi === true}>
        {#each filterItems(c.items, filter) as item (itemValue(item))}
          {@const value = itemValue(item)}
          <li role="option" aria-selected={isSelected(c.id, value)}>
            <button
              type="button"
              class="row-item"
              class:picked={isSelected(c.id, value)}
              onclick={() => toggleListItem(c, value)}
            >
              <span>{text(item.title)}</span>
              {#if item.subtitle}<span class="sub">{text(item.subtitle)}</span>{/if}
            </button>
          </li>
        {/each}
      </ul>
    {:else if c.type === "separator"}
      <hr class="sep" />
    {:else if c.type === "spacer"}
      <div class="spacer"></div>
    {/if}
  {/snippet}

  {@render node(root)}
  {#if busy > 0}<p class="label caption" role="status">{t("sevakUi.running")}</p>{/if}
  {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
</section>

<style>
  .sui {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 18px 14px;
    color: var(--fg);
    font-size: calc(14px * var(--font-scale, 1));
    max-height: 420px;
    overflow-y: auto;
    min-width: 0;
  }
  .sui.window {
    max-height: none;
    height: 100vh;
    box-sizing: border-box;
    background: var(--bg);
  }
  .box {
    display: flex;
    min-width: 0;
  }
  .column {
    flex-direction: column;
  }
  .row {
    flex-direction: row;
    flex-wrap: wrap;
  }
  .grid {
    display: grid;
  }
  .label {
    margin: 0;
    line-height: 1.45;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .heading {
    font-size: calc(17px * var(--font-scale, 1));
    font-weight: 650;
  }
  .caption {
    color: var(--muted);
    font-size: calc(12px * var(--font-scale, 1));
  }
  .mono {
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
  }
  .field {
    box-sizing: border-box;
    width: 100%;
    padding: 6px 9px;
    border: 1px solid var(--input-border);
    border-radius: 6px;
    background: var(--input-bg);
    color: var(--fg);
    font: inherit;
  }
  textarea.field {
    resize: vertical;
  }
  .btn {
    padding: 6px 14px;
    border: 1px solid var(--input-border);
    border-radius: 6px;
    background: var(--surface);
    color: var(--fg);
    font: inherit;
    cursor: pointer;
  }
  .btn.primary {
    border-color: var(--accent-strong);
    background: var(--accent);
    color: var(--on-accent);
    font-weight: 600;
  }
  .btn.danger {
    border-color: var(--error);
    color: var(--error);
  }
  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .check {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .slider {
    width: 100%;
  }
  .bar {
    width: 100%;
    accent-color: var(--accent-strong);
  }
  .img {
    max-width: 100%;
    height: auto;
    border-radius: 6px;
  }
  .list {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .row-item {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    width: 100%;
    padding: 6px 10px;
    border: 0;
    background: transparent;
    color: var(--fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .row-item.picked {
    background: var(--selected);
  }
  .sub {
    color: var(--muted);
    font-size: calc(12px * var(--font-scale, 1));
  }
  .sep {
    width: 100%;
    margin: 4px 0;
    border: 0;
    border-top: 1px solid var(--border);
  }
  .spacer {
    flex: 1 1 auto;
    min-height: 8px;
  }
  .problem {
    margin: 0;
    color: var(--error);
    font-size: calc(12.5px * var(--font-scale, 1));
  }
  :where(.sui button, .sui input, .sui textarea, .sui select):focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: 1px;
  }
</style>

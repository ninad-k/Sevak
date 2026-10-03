<script lang="ts">
  // Settings > Workflows: the installed workflows, "New from template", and the
  // visual builder (WorkflowEditor) for the one being edited.
  import "./workflows.css";
  import { onMount } from "svelte";
  import Toggle from "../Toggle.svelte";
  import WorkflowEditor from "./WorkflowEditor.svelte";
  import {
    createWorkflow,
    deleteWorkflow,
    listWorkflows,
    loadWorkflow,
    openWorkflowsFolder,
    reviewWorkflow,
    setWorkflowEnabled,
    workflowTemplates,
    type TemplateInfo,
    type WorkflowList,
    type WorkflowSummary,
  } from "./ipc";
  import { newWorkflow, normalize, type Workflow } from "./model";

  let list = $state<WorkflowList | null>(null);
  let templates = $state<TemplateInfo[]>([]);
  let error = $state<string | null>(null);
  let busy = $state<string | null>(null);
  let showTemplates = $state(false);
  /** The workflow open in the builder (`folder` null: a new one). */
  let editing = $state<{ folder: string | null; workflow: Workflow } | null>(null);

  async function refresh() {
    const result = await listWorkflows();
    if (result.ok) {
      list = result.value;
      error = null;
    } else {
      error = result.error;
    }
  }

  onMount(() => {
    void refresh();
    void workflowTemplates().then((result) => {
      if (result.ok) templates = result.value;
    });
  });

  async function edit(folder: string) {
    busy = folder;
    const result = await loadWorkflow(folder);
    busy = null;
    if (!result.ok) {
      error = result.error;
      return;
    }
    editing = { folder, workflow: normalize(result.value.workflow) };
  }

  function startBlank() {
    showTemplates = false;
    editing = { folder: null, workflow: newWorkflow() };
  }

  async function fromTemplate(id: string) {
    busy = id;
    const created = await createWorkflow(id);
    busy = null;
    showTemplates = false;
    if (!created.ok) {
      error = created.error;
      return;
    }
    await refresh();
    await edit(created.value.folder);
  }

  async function remove(row: WorkflowSummary) {
    busy = row.folder;
    const result = await deleteWorkflow(row.folder);
    busy = null;
    if (!result.ok) error = result.error;
    await refresh();
  }

  async function toggle(row: WorkflowSummary, enabled: boolean) {
    const result = await setWorkflowEnabled(row.folder, enabled);
    if (!result.ok) error = result.error;
    await refresh();
  }

  async function review(row: WorkflowSummary) {
    busy = row.folder;
    const result = await reviewWorkflow(row.folder);
    busy = null;
    if (!result.ok) error = result.error;
    await refresh();
  }

  async function closeEditor(changed: boolean) {
    editing = null;
    if (changed) await refresh();
  }
</script>

<!-- The list stays out of reach (keyboard included) while the builder covers it. -->
<div class="wf-page" inert={editing !== null}>
  <h1>Workflows</h1>
  <p class="wf-lead">
    A workflow chains a trigger (a keyword, a shortcut, Universal Actions or a command) to actions
    and outputs. Build one here, or drop a folder with a <code>workflow.toml</code> into the
    workflows folder. A workflow that runs scripts or commands asks for your permission first.
  </p>

  <div class="wf-toolbar">
    <button type="button" class="wf-btn primary" onclick={startBlank}>New workflow</button>
    <button
      type="button"
      class="wf-btn"
      aria-expanded={showTemplates}
      onclick={() => (showTemplates = !showTemplates)}
    >
      New from template…
    </button>
    <button type="button" class="wf-btn" onclick={() => void openWorkflowsFolder()}>
      Open workflows folder
    </button>
  </div>

  {#if showTemplates}
    <section class="wf-card templates" aria-label="Templates">
      {#each templates as template (template.id)}
        <div class="wf-row tpl">
          <div class="wf-grow">
            <div class="wf-title">{template.name}</div>
            <div class="wf-sub">{template.description}</div>
          </div>
          <button
            type="button"
            class="wf-btn small"
            disabled={busy !== null}
            onclick={() => fromTemplate(template.id)}
          >
            {busy === template.id ? "Creating…" : "Use this"}
          </button>
        </div>
      {/each}
    </section>
  {/if}

  {#if error}
    <p class="wf-msg error" role="alert">{error}</p>
  {/if}

  {#if list === null}
    <p class="wf-sub" role="status">Loading…</p>
  {:else if list.workflows.length === 0}
    <p class="wf-sub">
      No workflows yet. Start with a template, or press New workflow. They live in
      <code class="wf-code">{list.folder}</code>.
    </p>
  {:else}
    <ul class="rows" aria-label="Installed workflows">
      {#each list.workflows as row (row.folder)}
        <li class="wf-card">
          <div class="wf-row">
            <div class="wf-grow">
              <div class="wf-title">{row.name}</div>
              {#if row.description}<div class="wf-sub">{row.description}</div>{/if}
              <div class="chips">
                <span class="wf-chip" title="The folder">{row.folder}</span>
                {#each row.keywords as keyword (keyword)}
                  <span class="wf-chip">keyword: {keyword}</span>
                {/each}
                {#if row.nodes > 0}<span class="wf-chip">{row.nodes} nodes</span>{/if}
                {#if row.warnings > 0}
                  <span class="wf-chip warn">{row.warnings} warning{row.warnings === 1 ? "" : "s"}</span>
                {/if}
                {#if row.error}
                  <span class="wf-chip error">Not loaded</span>
                {:else if row.needs_approval && !row.approved}
                  <span class="wf-chip warn">Waiting for your permission</span>
                {:else if row.needs_approval}
                  <span class="wf-chip ok">Allowed to run code</span>
                {/if}
              </div>
              {#if row.error}<p class="wf-msg error">{row.error}</p>{/if}
              {#each row.keyword_warnings as warning (warning)}
                <p class="wf-msg warn">{warning}</p>
              {/each}
              {#each row.hotkey_errors as problem (problem.key)}
                <p class="wf-msg warn">The shortcut {problem.key} is not active: {problem.error}</p>
              {/each}
            </div>
            <div class="actions">
              {#if row.needs_approval && !row.approved && !row.error}
                <button
                  type="button"
                  class="wf-btn small primary"
                  disabled={busy !== null}
                  onclick={() => review(row)}>Review…</button
                >
              {/if}
              {#if !row.error}
                <Toggle
                  checked={row.enabled}
                  label="Enable {row.name}"
                  onchange={(on) => toggle(row, on)}
                />
              {/if}
              <button
                type="button"
                class="wf-btn small"
                disabled={busy !== null}
                onclick={() => edit(row.folder)}>{row.error ? "Fix…" : "Edit"}</button
              >
              <button
                type="button"
                class="wf-btn small danger"
                disabled={busy !== null}
                aria-label="Delete {row.name}"
                onclick={() => remove(row)}>Delete</button
              >
            </div>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

{#if editing}
  {#key editing.folder ?? "new"}
    <WorkflowEditor folder={editing.folder} initial={editing.workflow} onclose={closeEditor} />
  {/key}
{/if}

<style>
  .rows {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    margin-top: 6px;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .templates {
    margin-bottom: 12px;
  }

  .tpl + .tpl {
    margin-top: 10px;
    padding-top: 10px;
    border-top: 1px solid var(--border);
  }
</style>

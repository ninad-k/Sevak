<script lang="ts">
  // Settings > Workflows: the installed workflows, "New from template", and the
  // visual builder (WorkflowEditor) for the one being edited.
  import "./workflows.css";
  import { onMount, tick } from "svelte";
  import Toggle from "../Toggle.svelte";
  import ConfigureForm from "./ConfigureForm.svelte";
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
  import { t, tHtml } from "../i18n.svelte";

  let list = $state<WorkflowList | null>(null);
  let templates = $state<TemplateInfo[]>([]);
  let error = $state<string | null>(null);
  let busy = $state<string | null>(null);
  let showTemplates = $state(false);
  /** The workflow open in the builder (`folder` null: a new one). */
  let editing = $state<{ folder: string | null; workflow: Workflow } | null>(null);
  /** The workflow whose settings form is open. */
  let configuring = $state<string | null>(null);

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

  async function closeConfigure(saved: boolean) {
    configuring = null;
    if (saved) await refresh();
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

  /** What had focus when the builder opened, to give focus back when it closes. */
  let opener: HTMLElement | null = null;
  let page: HTMLElement | undefined = $state();
  $effect.pre(() => {
    if (editing && !opener) opener = document.activeElement as HTMLElement | null;
  });

  async function closeEditor(changed: boolean) {
    editing = null;
    const back = opener;
    opener = null;
    if (changed) await refresh();
    await tick();
    if (back?.isConnected && back !== document.body) back.focus();
    else page?.querySelector<HTMLElement>("button:not(:disabled)")?.focus();
  }
</script>

<!-- The list stays out of reach (keyboard included) while the builder covers it. -->
<div class="wf-page" inert={editing !== null || configuring !== null} bind:this={page}>
  <h1>{t("workflows.page.title")}</h1>
  <p class="wf-lead">{@html tHtml("workflows.page.lead")}</p>

  <div class="wf-toolbar">
    <button type="button" class="wf-btn primary" onclick={startBlank}>{t("workflows.page.newWorkflow")}</button>
    <button
      type="button"
      class="wf-btn"
      aria-expanded={showTemplates}
      onclick={() => (showTemplates = !showTemplates)}
    >
      {t("workflows.page.newFromTemplate")}
    </button>
    <button type="button" class="wf-btn" onclick={() => void openWorkflowsFolder()}>
      {t("workflows.page.openFolder")}
    </button>
  </div>

  {#if showTemplates}
    <section class="wf-card templates" aria-label={t("workflows.page.templates")}>
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
            aria-label={t("workflows.page.useTemplate", { name: template.name })}
            onclick={() => fromTemplate(template.id)}
          >
            {busy === template.id ? t("workflows.page.creating") : t("workflows.page.useThis")}
          </button>
        </div>
      {/each}
    </section>
  {/if}

  {#if error}
    <p class="wf-msg error" role="alert">{error}</p>
  {/if}

  {#if list === null}
    <p class="wf-sub" role="status">{t("common.loading")}</p>
  {:else if list.workflows.length === 0}
    <p class="wf-sub">{@html tHtml("workflows.page.empty", { folder: list.folder })}</p>
  {:else}
    <ul class="rows" aria-label={t("workflows.page.installed")}>
      {#each list.workflows as row (row.folder)}
        <li class="wf-card">
          <div class="wf-row">
            <div class="wf-grow">
              <div class="wf-title">{row.name}</div>
              {#if row.description}<div class="wf-sub">{row.description}</div>{/if}
              <div class="chips">
                <span class="wf-chip" title={t("workflows.page.folderTitle")}>{row.folder}</span>
                {#each row.keywords as keyword (keyword)}
                  <span class="wf-chip">{t("workflows.page.keyword", { keyword })}</span>
                {/each}
                {#if row.nodes > 0}<span class="wf-chip">{t("workflows.page.nodes", { count: row.nodes })}</span>{/if}
                {#if row.warnings > 0}
                  <span class="wf-chip warn">{t("workflows.page.warnings", { count: row.warnings })}</span>
                {/if}
                {#if row.missing_config.length > 0 && !row.error}
                  <span class="wf-chip warn">
                    {t("workflows.page.needsSettings", { fields: row.missing_config.join(", ") })}
                  </span>
                {/if}
                {#if row.error}
                  <span class="wf-chip error">{t("workflows.page.notLoaded")}</span>
                {:else if row.needs_approval && !row.approved}
                  <span class="wf-chip warn">{t("workflows.page.waiting")}</span>
                {:else if row.needs_approval}
                  <span class="wf-chip ok">{t("workflows.page.allowed")}</span>
                {/if}
              </div>
              {#if row.error}<p class="wf-msg error">{row.error}</p>{/if}
              {#each row.keyword_warnings as warning (warning)}
                <p class="wf-msg warn">{warning}</p>
              {/each}
              {#each row.hotkey_errors as problem (problem.key)}
                <p class="wf-msg warn">{t("workflows.page.hotkeyInactive", { key: problem.key, error: problem.error })}</p>
              {/each}
            </div>
            <div class="actions">
              {#if row.needs_approval && !row.approved && !row.error}
                <button
                  type="button"
                  class="wf-btn small primary"
                  disabled={busy !== null}
                  aria-label={t("workflows.page.reviewLabel", { name: row.name })}
                  onclick={() => review(row)}>{t("workflows.page.review")}</button
                >
              {/if}
              {#if row.configurable && !row.error}
                <button
                  type="button"
                  class="wf-btn small"
                  class:primary={row.missing_config.length > 0}
                  disabled={busy !== null}
                  aria-label={t("workflows.page.configureLabel", { name: row.name })}
                  onclick={() => (configuring = row.folder)}>{t("workflows.page.configure")}</button
                >
              {/if}
              {#if !row.error}
                <Toggle
                  checked={row.enabled}
                  label={t("workflows.page.enable", { name: row.name })}
                  onchange={(on) => toggle(row, on)}
                />
              {/if}
              <button
                type="button"
                class="wf-btn small"
                disabled={busy !== null}
                aria-label={row.error
                  ? t("workflows.page.fixLabel", { name: row.name })
                  : t("workflows.page.editLabel", { name: row.name })}
                onclick={() => edit(row.folder)}
                >{row.error ? t("workflows.page.fix") : t("workflows.page.edit")}</button
              >
              <button
                type="button"
                class="wf-btn small danger"
                disabled={busy !== null}
                aria-label={t("workflows.page.deleteLabel", { name: row.name })}
                onclick={() => remove(row)}>{t("workflows.page.delete")}</button
              >
            </div>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

{#if configuring}
  <ConfigureForm folder={configuring} onclose={closeConfigure} />
{/if}

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

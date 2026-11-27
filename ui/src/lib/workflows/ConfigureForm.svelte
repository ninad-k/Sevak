<script lang="ts">
  // The settings form of one workflow ("Configure…"): a control per `[[config]]`
  // field the author declared. The values are the user's own and are kept
  // outside the workflow folder (see `workflow/config.rs`); the backend checks
  // every value against its field again before saving.
  import "./workflows.css";
  import { onMount, tick } from "svelte";
  import Toggle from "../Toggle.svelte";
  import { pickDirectory, pickFile } from "../settings-ipc";
  import { t } from "../i18n.svelte";
  import { getWorkflowConfig, setWorkflowConfig, type ConfigValues, type WorkflowConfigForm } from "./ipc";
  import { DEFAULT_SLIDER_MAX, DEFAULT_SLIDER_MIN, type ConfigField } from "./model";

  let {
    folder,
    onclose,
  }: {
    folder: string;
    /** Leaves the form; `saved` is true when the values were saved. */
    onclose: (saved: boolean) => void;
  } = $props();

  let form = $state<WorkflowConfigForm | null>(null);
  /** What each control shows, as text (a checkbox "true"/"false"). */
  let values = $state<Record<string, string>>({});
  let error = $state<string | null>(null);
  let saving = $state(false);
  let dialog: HTMLElement | undefined = $state();

  const label = (field: ConfigField) => field.label?.trim() || field.variable;
  const sliderMin = (field: ConfigField) => field.min ?? DEFAULT_SLIDER_MIN;
  const sliderMax = (field: ConfigField) => field.max ?? DEFAULT_SLIDER_MAX;

  /** The text a control starts with when nothing is saved for it. */
  function fallback(field: ConfigField): string {
    if (field.default) return field.default;
    if (field.kind === "checkbox") return "false";
    if (field.kind === "slider") return String(sliderMin(field));
    return "";
  }

  onMount(async () => {
    const result = await getWorkflowConfig(folder);
    if (!result.ok) {
      error = result.error;
      return;
    }
    form = result.value;
    values = Object.fromEntries(
      result.value.fields.map((field) => [field.variable, result.value.values[field.variable] ?? fallback(field)]),
    );
    await tick();
    dialog?.querySelector<HTMLElement>("input:not([type=range]), select, textarea, button.cf-primary")?.focus();
  });

  /** The required fields still empty (a checkbox always has a value). */
  const missing = $derived(
    (form?.fields ?? [])
      .filter((field) => field.required && field.kind !== "checkbox")
      .filter((field) => (values[field.variable] ?? "").trim() === "")
      .map(label),
  );

  /**
   * What to save: every field that already has a saved value, plus those the
   * user changed. A field left at its default stays unsaved, so a newer default
   * from a workflow update still applies.
   */
  function payload(): ConfigValues {
    const out: ConfigValues = {};
    for (const field of form?.fields ?? []) {
      const text = values[field.variable] ?? "";
      const saved = field.variable in (form?.values ?? {});
      if (!saved && text === fallback(field)) continue;
      out[field.variable] =
        field.kind === "checkbox" ? text === "true" : field.kind === "slider" ? Number(text) : text;
    }
    return out;
  }

  async function save() {
    if (!form || saving || missing.length > 0) return;
    saving = true;
    error = null;
    const result = await setWorkflowConfig(folder, payload());
    saving = false;
    if (result.ok) onclose(true);
    else error = result.error;
  }

  async function browse(field: ConfigField) {
    const picked =
      field.mode === "folder"
        ? await pickDirectory(t("workflows.config.chooseFolder"))
        : await pickFile(t("workflows.config.chooseFile"));
    if (picked) values[field.variable] = picked;
  }

  function onkey(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.stopPropagation();
      onclose(false);
    }
  }

  const id = (field: ConfigField) => `cf-${field.variable}`;
</script>

<div class="cf-backdrop" role="presentation">
  <div
    class="cf-dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="cf-title"
    tabindex="-1"
    bind:this={dialog}
    onkeydown={onkey}
  >
    <h2 id="cf-title">{t("workflows.config.title", { name: form?.name ?? folder })}</h2>
    <p class="wf-hint lead">{t("workflows.config.lead")}</p>

    {#if error}
      <p class="wf-msg error" role="alert">{error}</p>
    {/if}

    {#if form === null}
      {#if !error}<p class="wf-sub" role="status">{t("common.loading")}</p>{/if}
    {:else if form.fields.length === 0}
      <p class="wf-sub">{t("workflows.config.empty")}</p>
    {:else}
      <div class="cf-fields">
        {#each form.fields as field (field.variable)}
          <div class="wf-field">
            {#if field.kind === "checkbox"}
              <div class="cf-switch">
                <span class="wf-label" id={id(field)}>{label(field)}</span>
                <Toggle
                  checked={values[field.variable] === "true"}
                  label={label(field)}
                  onchange={(on) => (values[field.variable] = on ? "true" : "false")}
                />
              </div>
            {:else}
              <label for={id(field)}>
                {label(field)}
                {#if field.required}<span class="cf-req" title={t("workflows.config.required")}>*</span>{/if}
              </label>
              {#if field.kind === "textarea"}
                <textarea
                  id={id(field)}
                  class="wf-input"
                  rows="3"
                  spellcheck="false"
                  placeholder={field.placeholder ?? ""}
                  bind:value={values[field.variable]}
                ></textarea>
              {:else if field.kind === "popup"}
                <select id={id(field)} class="wf-input" bind:value={values[field.variable]}>
                  {#if !field.default}<option value="">{t("workflows.config.choose")}</option>{/if}
                  {#each field.options ?? [] as option (option.value)}
                    <option value={option.value}>{option.label || option.value}</option>
                  {/each}
                </select>
              {:else if field.kind === "slider"}
                <div class="cf-slider">
                  <input
                    id={id(field)}
                    type="range"
                    min={sliderMin(field)}
                    max={sliderMax(field)}
                    step={field.step ?? 1}
                    bind:value={values[field.variable]}
                  />
                  <output for={id(field)}>{values[field.variable]}</output>
                </div>
              {:else if field.kind === "filepicker"}
                <div class="cf-pick">
                  <input
                    id={id(field)}
                    class="wf-input mono"
                    type="text"
                    spellcheck="false"
                    autocomplete="off"
                    bind:value={values[field.variable]}
                  />
                  <button type="button" class="wf-btn" onclick={() => browse(field)}>
                    {t("workflows.config.browse")}
                  </button>
                </div>
              {:else}
                <input
                  id={id(field)}
                  class="wf-input"
                  type="text"
                  spellcheck="false"
                  autocomplete="off"
                  placeholder={field.placeholder ?? ""}
                  bind:value={values[field.variable]}
                />
              {/if}
            {/if}
            {#if field.description}<p class="wf-hint">{field.description}</p>{/if}
          </div>
        {/each}
      </div>
    {/if}

    {#if missing.length > 0}
      <p class="wf-hint" role="status">{t("workflows.config.fillIn", { fields: missing.join(", ") })}</p>
    {/if}

    <div class="cf-buttons">
      <button type="button" class="wf-btn" onclick={() => onclose(false)}>{t("common.cancel")}</button>
      <button
        type="button"
        class="wf-btn primary cf-primary"
        disabled={!form || form.fields.length === 0 || saving || missing.length > 0}
        onclick={save}>{saving ? t("common.saving") : t("common.save")}</button
      >
    </div>
  </div>
</div>

<style>
  .cf-backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 16px;
    background: rgba(0, 0, 0, 0.4);
  }

  .cf-dialog {
    width: min(480px, 100%);
    max-height: 100%;
    overflow-y: auto;
    padding: 16px 18px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--bg);
    color: var(--fg);
    box-shadow: 0 10px 40px rgba(0, 0, 0, 0.35);
  }

  h2 {
    margin: 0 0 4px;
    font-size: 16px;
    font-weight: 650;
  }

  .lead {
    margin-bottom: 12px;
  }

  .cf-switch {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .cf-switch .wf-label {
    margin: 0;
  }

  .cf-req {
    color: var(--error);
  }

  .cf-slider,
  .cf-pick {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .cf-slider input {
    flex: 1;
  }

  .cf-slider output {
    min-width: 3ch;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .cf-buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 14px;
  }
</style>

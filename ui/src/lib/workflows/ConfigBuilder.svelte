<script lang="ts">
  // The author's side of the settings form: add, edit, reorder and remove the
  // `[[config]]` fields of a workflow. Every edit hands the whole new list to
  // `onchange`, so the editor that owns the workflow does the mutation.
  import "./workflows.css";
  import { tHtml, t, type MessageKey } from "../i18n.svelte";
  import {
    CONFIG_KINDS,
    DEFAULT_SLIDER_MAX,
    DEFAULT_SLIDER_MIN,
    isConfigVariable,
    newConfigField,
    type ConfigField,
    type ConfigKind,
  } from "./model";

  let {
    fields,
    onchange,
  }: {
    fields: ConfigField[];
    onchange: (next: ConfigField[]) => void;
  } = $props();

  function patch(index: number, change: Partial<ConfigField>) {
    onchange(fields.map((field, i) => (i === index ? { ...field, ...change } : field)));
  }

  function add() {
    onchange([...fields, newConfigField(fields)]);
  }

  function remove(index: number) {
    onchange(fields.filter((_, i) => i !== index));
  }

  function move(index: number, by: -1 | 1) {
    const to = index + by;
    if (to < 0 || to >= fields.length) return;
    const next = [...fields];
    [next[index], next[to]] = [next[to], next[index]];
    onchange(next);
  }

  function setKind(index: number, kind: ConfigKind) {
    // The default of one kind means nothing to another.
    patch(index, { kind, default: "" });
  }

  function setOption(index: number, at: number, key: "label" | "value", text: string) {
    const options = (fields[index].options ?? []).map((option, i) =>
      i === at ? { ...option, [key]: text } : option,
    );
    patch(index, { options });
  }

  function addOption(index: number) {
    patch(index, { options: [...(fields[index].options ?? []), { label: "", value: "" }] });
  }

  function removeOption(index: number, at: number) {
    patch(index, { options: (fields[index].options ?? []).filter((_, i) => i !== at) });
  }

  function setNumber(index: number, key: "min" | "max" | "step", text: string) {
    const value = text.trim() === "" ? null : Number(text);
    patch(index, { [key]: value !== null && Number.isFinite(value) ? value : null });
  }

  const name = (field: ConfigField) => field.label?.trim() || field.variable;
  const id = (index: number, key: string) => `wf-config-${index}-${key}`;
</script>

<div class="wf-field">
  <span class="wf-label">{t("workflows.builder.title")}</span>
  {#each fields as field, index (index)}
    <div class="wf-card cb-card">
      <div class="cb-head">
        <strong>{name(field)}</strong>
        <span class="cb-tools">
          <button
            type="button"
            class="wf-btn small"
            disabled={index === 0}
            aria-label={t("workflows.builder.moveUp", { name: name(field) })}
            onclick={() => move(index, -1)}>↑</button
          >
          <button
            type="button"
            class="wf-btn small"
            disabled={index === fields.length - 1}
            aria-label={t("workflows.builder.moveDown", { name: name(field) })}
            onclick={() => move(index, 1)}>↓</button
          >
          <button
            type="button"
            class="wf-btn small danger"
            aria-label={t("workflows.builder.remove", { name: name(field) })}
            onclick={() => remove(index)}>×</button
          >
        </span>
      </div>

      <div class="wf-field">
        <label for={id(index, "label")}>{t("workflows.builder.label")}</label>
        <input
          id={id(index, "label")}
          class="wf-input"
          type="text"
          value={field.label ?? ""}
          oninput={(e) => patch(index, { label: e.currentTarget.value })}
        />
      </div>
      <div class="wf-field">
        <label for={id(index, "variable")}>{t("workflows.builder.variable")}</label>
        <input
          id={id(index, "variable")}
          class="wf-input mono"
          class:invalid={!isConfigVariable(field.variable)}
          type="text"
          spellcheck="false"
          autocomplete="off"
          value={field.variable}
          oninput={(e) => patch(index, { variable: e.currentTarget.value })}
        />
      </div>
      <div class="wf-field">
        <label for={id(index, "kind")}>{t("workflows.builder.kind")}</label>
        <select
          id={id(index, "kind")}
          class="wf-input"
          value={field.kind}
          onchange={(e) => setKind(index, e.currentTarget.value as ConfigKind)}
        >
          {#each CONFIG_KINDS as kind (kind)}
            <option value={kind}>{t(`workflows.builder.kinds.${kind}` as MessageKey)}</option>
          {/each}
        </select>
      </div>
      <label class="wf-check">
        <input
          type="checkbox"
          checked={field.required === true}
          disabled={field.kind === "checkbox"}
          onchange={(e) => patch(index, { required: e.currentTarget.checked })}
        />
        {t("workflows.builder.required")}
      </label>
      <div class="wf-field">
        <label for={id(index, "description")}>{t("workflows.builder.description")}</label>
        <input
          id={id(index, "description")}
          class="wf-input"
          type="text"
          value={field.description ?? ""}
          oninput={(e) => patch(index, { description: e.currentTarget.value })}
        />
      </div>

      {#if field.kind === "popup"}
        <span class="wf-label">{t("workflows.builder.options")}</span>
        {#each field.options ?? [] as option, at (at)}
          <div class="wf-item">
            <input
              class="wf-input"
              type="text"
              aria-label={t("workflows.builder.optionLabel", { n: at + 1 })}
              placeholder={t("workflows.builder.optionLabelPlaceholder")}
              value={option.label}
              oninput={(e) => setOption(index, at, "label", e.currentTarget.value)}
            />
            <input
              class="wf-input mono"
              type="text"
              spellcheck="false"
              aria-label={t("workflows.builder.optionValue", { n: at + 1 })}
              placeholder={t("workflows.builder.optionValuePlaceholder")}
              value={option.value}
              oninput={(e) => setOption(index, at, "value", e.currentTarget.value)}
            />
            <button
              type="button"
              class="wf-btn small danger"
              aria-label={t("workflows.builder.removeOption", { n: at + 1 })}
              onclick={() => removeOption(index, at)}>×</button
            >
          </div>
        {/each}
        <button type="button" class="wf-btn small" onclick={() => addOption(index)}>
          {t("workflows.builder.addOption")}
        </button>
        <div class="wf-field">
          <label for={id(index, "default")}>{t("workflows.builder.default")}</label>
          <select
            id={id(index, "default")}
            class="wf-input"
            value={field.default ?? ""}
            onchange={(e) => patch(index, { default: e.currentTarget.value })}
          >
            <option value="">{t("workflows.builder.noDefault")}</option>
            {#each (field.options ?? []).filter((o) => o.value !== "") as option (option.value)}
              <option value={option.value}>{option.label || option.value}</option>
            {/each}
          </select>
        </div>
      {:else if field.kind === "checkbox"}
        <div class="wf-field">
          <label for={id(index, "default")}>{t("workflows.builder.default")}</label>
          <select
            id={id(index, "default")}
            class="wf-input"
            value={field.default === "true" ? "true" : "false"}
            onchange={(e) => patch(index, { default: e.currentTarget.value === "true" ? "true" : "" })}
          >
            <option value="false">{t("workflows.builder.off")}</option>
            <option value="true">{t("workflows.builder.on")}</option>
          </select>
        </div>
      {:else if field.kind === "slider"}
        <div class="cb-numbers">
          {#each ["min", "max", "step"] as const as key (key)}
            <div class="wf-field">
              <label for={id(index, key)}>{t(`workflows.builder.${key}` as MessageKey)}</label>
              <input
                id={id(index, key)}
                class="wf-input"
                type="number"
                placeholder={key === "min"
                  ? String(DEFAULT_SLIDER_MIN)
                  : key === "max"
                    ? String(DEFAULT_SLIDER_MAX)
                    : "1"}
                value={field[key] ?? ""}
                oninput={(e) => setNumber(index, key, e.currentTarget.value)}
              />
            </div>
          {/each}
        </div>
        <div class="wf-field">
          <label for={id(index, "default")}>{t("workflows.builder.default")}</label>
          <input
            id={id(index, "default")}
            class="wf-input"
            type="number"
            value={field.default ?? ""}
            oninput={(e) => patch(index, { default: e.currentTarget.value })}
          />
        </div>
      {:else}
        {#if field.kind === "filepicker"}
          <div class="wf-field">
            <label for={id(index, "mode")}>{t("workflows.builder.mode")}</label>
            <select
              id={id(index, "mode")}
              class="wf-input"
              value={field.mode ?? "file"}
              onchange={(e) => patch(index, { mode: e.currentTarget.value === "folder" ? "folder" : "file" })}
            >
              <option value="file">{t("workflows.builder.modeFile")}</option>
              <option value="folder">{t("workflows.builder.modeFolder")}</option>
            </select>
          </div>
        {:else}
          <div class="wf-field">
            <label for={id(index, "placeholder")}>{t("workflows.builder.placeholder")}</label>
            <input
              id={id(index, "placeholder")}
              class="wf-input"
              type="text"
              value={field.placeholder ?? ""}
              oninput={(e) => patch(index, { placeholder: e.currentTarget.value })}
            />
          </div>
        {/if}
        <div class="wf-field">
          <label for={id(index, "default")}>{t("workflows.builder.default")}</label>
          <input
            id={id(index, "default")}
            class="wf-input"
            type="text"
            spellcheck="false"
            value={field.default ?? ""}
            oninput={(e) => patch(index, { default: e.currentTarget.value })}
          />
        </div>
      {/if}
    </div>
  {/each}
  <button type="button" class="wf-btn small" onclick={add}>{t("workflows.builder.add")}</button>
  <p class="wf-hint">{@html tHtml("workflows.builder.hint")}</p>
</div>

<style>
  .cb-card {
    margin-bottom: 8px;
  }

  .cb-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 8px;
  }

  .cb-tools {
    display: flex;
    gap: 4px;
  }

  .cb-numbers {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 8px;
  }
</style>

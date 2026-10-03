<script lang="ts">
  // The visual builder: nodes as boxes on an SVG canvas, connectors between
  // output and input ports, a side panel for the selected node. It edits a
  // copy of the workflow and writes it with `save_workflow` (workflow.toml).
  //
  // Mouse: drag a box to move it, drag from an output dot to another box to
  // connect, click a box or a line to select it, Delete removes the selection.
  // Keyboard: Tab to a box, arrows move it, Enter selects it; the side panel
  // can add and remove connections without dragging.
  import "./workflows.css";
  import { onMount } from "svelte";
  import NodeFields from "./NodeFields.svelte";
  import { checkWorkflow, saveWorkflow } from "./ipc";
  import {
    CATEGORIES,
    CATEGORY_LABELS,
    ELSE,
    KINDS,
    NODE_H,
    NODE_W,
    OUT,
    PORT_R,
    THEN,
    autoLayout,
    canConnect,
    curve,
    inPoint,
    isStart,
    kindOf,
    makeNode,
    needsLayout,
    normalize,
    outPoint,
    outputPorts,
    portOf,
    removeNode,
    renameNode,
    toSave,
    type NodeType,
    type Point,
    type Problem,
    type WfConnection,
    type WfNode,
    type Workflow,
  } from "./model";

  let {
    folder,
    initial,
    onclose,
  }: {
    /** The workflow's folder, or `null` for one that has not been saved yet. */
    folder: string | null;
    initial: Workflow;
    /** Leaves the editor; `changed` is true when something was saved. */
    onclose: (changed: boolean) => void;
  } = $props();

  // The editor is mounted afresh for every workflow, so the props are read once.
  // svelte-ignore state_referenced_locally
  let workflow = $state<Workflow>(prepare(initial));
  // svelte-ignore state_referenced_locally
  let currentFolder = $state<string | null>(folder);
  let baseline = $state(snapshot(workflow));
  let changed = false;

  type Selection = { kind: "node"; id: string } | { kind: "edge"; key: string } | null;
  let selection = $state<Selection>(null);
  let problems = $state<Problem[]>([]);
  let notice = $state<{ text: string; kind: "ok" | "warn" | "error" } | null>(null);
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;
  let saving = $state(false);
  let confirmingDiscard = $state(false);
  let svg: SVGSVGElement | undefined = $state();
  let wrap: HTMLElement | undefined = $state();
  let root: HTMLElement | undefined = $state();
  let addValue = $state("");
  // svelte-ignore state_referenced_locally
  let variableRows = $state(Object.entries(workflow.variables).map(([name, value]) => ({ name, value })));

  /** A node being dragged. */
  let drag: { id: string; dx: number; dy: number } | null = null;
  /** A connection being drawn from an output port. */
  let link = $state<{ from: string; port: string; x: number; y: number; hover: string | null } | null>(
    null,
  );

  function prepare(source: Workflow): Workflow {
    const copy = normalize(JSON.parse(JSON.stringify(source)) as Workflow);
    if (needsLayout(copy)) autoLayout(copy);
    return copy;
  }

  function snapshot(value: Workflow): string {
    return JSON.stringify(toSave(value));
  }

  const dirty = $derived(snapshot(workflow) !== baseline);
  const errors = $derived(problems.filter((p) => p.severity === "error"));
  const canSave = $derived(!saving && errors.length === 0 && (dirty || currentFolder === null));
  const needsAllow = $derived(workflow.node.some((node) => kindOf(node.type).needsApproval));

  const canvasW = $derived(Math.max(900, ...workflow.node.map((n) => n.x + NODE_W + 120)));
  const canvasH = $derived(Math.max(520, ...workflow.node.map((n) => n.y + NODE_H + 120)));

  const selectedNode = $derived.by(() => {
    const picked = selection;
    return picked?.kind === "node" ? workflow.node.find((n) => n.id === picked.id) : undefined;
  });
  const selectedEdge = $derived.by(() => {
    const picked = selection;
    return picked?.kind === "edge"
      ? workflow.connection.find((c) => edgeKey(c) === picked.key)
      : undefined;
  });

  const byNode = $derived.by(() => {
    const map = new Map<string, Problem[]>();
    for (const problem of problems) {
      if (problem.node) map.set(problem.node, [...(map.get(problem.node) ?? []), problem]);
    }
    return map;
  });

  const edgeKey = (c: WfConnection) => `${c.from}|${portOf(c)}|${c.to}`;
  const nodeById = (id: string) => workflow.node.find((n) => n.id === id);

  function say(text: string, kind: "ok" | "warn" | "error" = "warn") {
    notice = { text, kind };
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => (notice = null), 4000);
  }

  // Validation by Rust, shortly after every change.
  let checkSeq = 0;
  $effect(() => {
    const body = snapshot(workflow);
    const where = currentFolder;
    const mine = ++checkSeq;
    const timer = setTimeout(async () => {
      const result = await checkWorkflow(JSON.parse(body) as Workflow, where);
      if (mine === checkSeq && result.ok) problems = result.value;
    }, 120);
    return () => clearTimeout(timer);
  });

  onMount(() => {
    root?.focus();
    return () => clearTimeout(noticeTimer);
  });

  // ---- text on the boxes -------------------------------------------------------

  function clip(text: string, max: number): string {
    const flat = text.replace(/\s+/g, " ").trim();
    return flat.length > max ? `${flat.slice(0, max - 1)}…` : flat;
  }

  function detail(node: WfNode): string {
    const def = kindOf(node.type);
    for (const field of def.fields) {
      const value = node[field.key];
      if (typeof value === "string" && value.trim() !== "" && field.kind !== "select") return value;
      if (Array.isArray(value) && field.kind === "lines" && value.length > 0) return value.join(" ");
    }
    return "";
  }

  function nodeLabel(node: WfNode): string {
    return node.title?.trim() || kindOf(node.type).label;
  }

  // ---- geometry helpers ------------------------------------------------------

  function toCanvas(e: PointerEvent): Point {
    const rect = svg!.getBoundingClientRect();
    return { x: e.clientX - rect.left, y: e.clientY - rect.top };
  }

  const snap = (value: number) => Math.max(0, Math.round(value / 10) * 10);

  function nodeAt(p: Point): WfNode | undefined {
    return [...workflow.node]
      .reverse()
      .find(
        (n) =>
          p.x >= n.x - PORT_R &&
          p.x <= n.x + NODE_W + PORT_R &&
          p.y >= n.y - 4 &&
          p.y <= n.y + NODE_H + 4,
      );
  }

  // ---- pointer interaction ------------------------------------------------------

  function onNodeDown(e: PointerEvent, node: WfNode) {
    if (e.button !== 0) return;
    e.stopPropagation();
    selection = { kind: "node", id: node.id };
    const p = toCanvas(e);
    drag = { id: node.id, dx: p.x - node.x, dy: p.y - node.y };
    svg!.setPointerCapture(e.pointerId);
    root?.focus();
  }

  function onPortDown(e: PointerEvent, node: WfNode, port: string) {
    if (e.button !== 0) return;
    e.stopPropagation();
    const p = toCanvas(e);
    selection = { kind: "node", id: node.id };
    link = { from: node.id, port, x: p.x, y: p.y, hover: null };
    svg!.setPointerCapture(e.pointerId);
    root?.focus();
  }

  function onBackgroundDown() {
    selection = null;
    confirmingDiscard = false;
    root?.focus();
  }

  function onMove(e: PointerEvent) {
    if (drag) {
      const node = nodeById(drag.id);
      if (!node) return;
      const p = toCanvas(e);
      node.x = snap(p.x - drag.dx);
      node.y = snap(p.y - drag.dy);
    } else if (link) {
      const p = toCanvas(e);
      link.x = p.x;
      link.y = p.y;
      link.hover = nodeAt(p)?.id ?? null;
    }
  }

  function onUp(e: PointerEvent) {
    if (svg?.hasPointerCapture(e.pointerId)) svg.releasePointerCapture(e.pointerId);
    if (drag) {
      drag = null;
    } else if (link) {
      const target = nodeAt(toCanvas(e));
      const from = link;
      link = null;
      if (target && target.id !== from.from) connect(from.from, from.port, target.id);
    }
  }

  function onCancel() {
    drag = null;
    link = null;
  }

  function connect(from: string, port: string, to: string) {
    const verdict = canConnect(workflow, from, port, to);
    if (!verdict.ok) {
      say(verdict.reason);
      return;
    }
    workflow.connection.push({ from, port, to });
    selection = { kind: "edge", key: `${from}|${port}|${to}` };
  }

  // ---- keyboard ---------------------------------------------------------------

  function onKeydown(e: KeyboardEvent) {
    const target = e.target as HTMLElement;
    const typing = ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "s") {
      // Not the settings form's save.
      e.preventDefault();
      e.stopPropagation();
      void save();
      return;
    }
    if (typing) return;
    if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      deleteSelection();
    } else if (e.key === "Escape") {
      if (link) link = null;
      else selection = null;
    }
  }

  function onNodeKey(e: KeyboardEvent, node: WfNode) {
    const step = e.shiftKey ? 40 : 10;
    const moves: Record<string, [number, number]> = {
      ArrowLeft: [-step, 0],
      ArrowRight: [step, 0],
      ArrowUp: [0, -step],
      ArrowDown: [0, step],
    };
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      selection = { kind: "node", id: node.id };
    } else if (moves[e.key]) {
      e.preventDefault();
      node.x = Math.max(0, node.x + moves[e.key][0]);
      node.y = Math.max(0, node.y + moves[e.key][1]);
    }
  }

  function deleteSelection() {
    if (selection?.kind === "node") {
      const id = selection.id;
      removeNode(workflow, id);
    } else if (selection?.kind === "edge") {
      const key = selection.key;
      workflow.connection = workflow.connection.filter((c) => edgeKey(c) !== key);
    }
    selection = null;
  }

  // ---- adding and editing -----------------------------------------------------------

  function addNode(type: NodeType) {
    // The first free spot at the top-left of what is visible.
    const left = (wrap?.scrollLeft ?? 0) + 40;
    const top = (wrap?.scrollTop ?? 0) + 40;
    const taken = (x: number, y: number) =>
      workflow.node.some(
        (other) => Math.abs(other.x - x) < NODE_W + 20 && Math.abs(other.y - y) < NODE_H + 20,
      );
    let x = left;
    let y = top;
    for (let tries = 0; taken(x, y) && tries < 40; tries++) {
      y += NODE_H + 30;
      if (y > top + 5 * (NODE_H + 30)) {
        y = top;
        x += NODE_W + 40;
      }
    }
    const node = makeNode(workflow, type, snap(x), snap(y));
    workflow.node.push(node);
    selection = { kind: "node", id: node.id };
  }

  function onAdd(e: Event & { currentTarget: HTMLSelectElement }) {
    const type = e.currentTarget.value as NodeType;
    addValue = "";
    if (type) addNode(type);
  }

  function setField(node: WfNode, key: string, value: unknown) {
    if (value === undefined) delete node[key];
    else node[key] = value;
  }

  function setId(node: WfNode, raw: string, input: HTMLInputElement) {
    const next = raw.trim();
    if (next === node.id) return;
    if (!/^[A-Za-z0-9_-]{1,40}$/.test(next)) {
      say("An id uses 1 to 40 letters, digits, dashes or underscores.");
      input.value = node.id;
      return;
    }
    if (workflow.node.some((n) => n.id === next)) {
      say("Another node already has that id.");
      input.value = node.id;
      return;
    }
    const old = node.id;
    renameNode(workflow, old, next);
    selection = { kind: "node", id: next };
  }

  function syncVariables() {
    const variables: Record<string, string> = {};
    for (const row of variableRows) {
      if (row.name.trim() !== "") variables[row.name.trim()] = row.value;
    }
    workflow.variables = variables;
  }

  function addVariable() {
    variableRows.push({ name: "", value: "" });
  }

  function removeVariable(index: number) {
    variableRows.splice(index, 1);
    syncVariables();
  }

  function tidy() {
    autoLayout(workflow);
  }

  const portLabel = (port: string) =>
    port === THEN ? "When true, go to" : port === ELSE ? "Otherwise, go to" : "Next";

  function targetsFor(node: WfNode, port: string): WfNode[] {
    return workflow.node.filter((other) => canConnect(workflow, node.id, port, other.id).ok);
  }

  function addFromPanel(node: WfNode, port: string, to: string) {
    if (to) connect(node.id, port, to);
  }

  function removeConnection(conn: WfConnection) {
    const key = edgeKey(conn);
    workflow.connection = workflow.connection.filter((c) => edgeKey(c) !== key);
    if (selection?.kind === "edge" && selection.key === key) selection = null;
  }

  function focusProblem(problem: Problem) {
    if (!problem.node) return;
    const node = nodeById(problem.node);
    if (!node) return;
    selection = { kind: "node", id: node.id };
    wrap?.scrollTo({
      left: Math.max(0, node.x - 80),
      top: Math.max(0, node.y - 80),
      behavior: "smooth",
    });
  }

  // ---- saving and leaving ---------------------------------------------------------------

  async function save() {
    if (!canSave) return;
    saving = true;
    const result = await saveWorkflow(currentFolder, toSave(workflow));
    saving = false;
    if (!result.ok) {
      say(result.error, "error");
      return;
    }
    currentFolder = result.value.folder;
    baseline = snapshot(workflow);
    changed = true;
    say(
      needsAllow
        ? "Saved. Sevak will ask you to allow it before it runs anything."
        : "Saved.",
      "ok",
    );
  }

  function back() {
    if (dirty && !confirmingDiscard) {
      confirmingDiscard = true;
      return;
    }
    onclose(changed);
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="editor"
  role="region"
  aria-label="Workflow builder"
  tabindex="0"
  bind:this={root}
  onkeydown={onKeydown}
>
  <header class="bar">
    <button type="button" class="wf-btn" onclick={back}>← Workflows</button>
    <div class="wf-grow name">
      <strong>{workflow.name || "Untitled"}</strong>
      <span class="wf-sub">
        {currentFolder ? `folder: ${currentFolder}` : "not saved yet"}{dirty ? " · unsaved changes" : ""}
      </span>
    </div>
    {#if errors.length > 0}
      <span class="wf-chip error" role="status">{errors.length} problem{errors.length === 1 ? "" : "s"} to fix</span>
    {/if}
    <label class="add">
      <span class="visually-hidden">Add a node</span>
      <select class="wf-input" bind:value={addValue} onchange={onAdd} aria-label="Add a node">
        <option value="">Add a node…</option>
        {#each CATEGORIES as category (category)}
          <optgroup label={CATEGORY_LABELS[category]}>
            {#each KINDS.filter((k) => k.category === category) as kind (kind.type)}
              <option value={kind.type}>{kind.label}</option>
            {/each}
          </optgroup>
        {/each}
      </select>
    </label>
    <button type="button" class="wf-btn" onclick={tidy} title="Arrange the boxes in columns">
      Tidy
    </button>
    <button type="button" class="wf-btn primary" disabled={!canSave} onclick={save}>
      {saving ? "Saving…" : "Save"}
    </button>
  </header>

  {#if confirmingDiscard}
    <div class="banner" role="alert">
      <span>You have unsaved changes.</span>
      <button type="button" class="wf-btn small danger" onclick={() => onclose(changed)}>
        Discard them
      </button>
      <button type="button" class="wf-btn small" onclick={() => (confirmingDiscard = false)}>
        Keep editing
      </button>
    </div>
  {/if}

  <div class="body">
    <div class="canvas" bind:this={wrap}>
      <svg
        bind:this={svg}
        width={canvasW}
        height={canvasH}
        role="application"
        aria-label="Workflow canvas. Boxes are nodes; drag from a dot on the right edge of a box to another box to connect them."
        onpointerdown={onBackgroundDown}
        onpointermove={onMove}
        onpointerup={onUp}
        onpointercancel={onCancel}
      >
        <defs>
          <pattern id="wf-grid" width="20" height="20" patternUnits="userSpaceOnUse">
            <circle cx="1" cy="1" r="1" class="grid-dot" />
          </pattern>
          <marker
            id="wf-arrow"
            viewBox="0 0 10 10"
            refX="9"
            refY="5"
            markerWidth="7"
            markerHeight="7"
            orient="auto-start-reverse"
          >
            <path d="M 0 0 L 10 5 L 0 10 z" class="arrow" />
          </marker>
        </defs>
        <rect width="100%" height="100%" fill="url(#wf-grid)" />

        {#each workflow.connection as conn (edgeKey(conn))}
          {@const from = nodeById(conn.from)}
          {@const to = nodeById(conn.to)}
          {#if from && to}
            {@const d = curve(outPoint(from, portOf(conn)), inPoint(to))}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <g
              class="edge"
              class:selected={selection?.kind === "edge" && selection.key === edgeKey(conn)}
              onpointerdown={(e) => {
                e.stopPropagation();
                selection = { kind: "edge", key: edgeKey(conn) };
                root?.focus();
              }}
            >
              <path {d} class="edge-hit" />
              <path {d} class="edge-line" marker-end="url(#wf-arrow)" />
            </g>
          {/if}
        {/each}

        {#if link}
          {@const from = nodeById(link.from)}
          {#if from}
            <path
              d={curve(outPoint(from, link.port), { x: link.x, y: link.y })}
              class="edge-line temp"
            />
          {/if}
        {/if}

        {#each workflow.node as node (node.id)}
          {@const def = kindOf(node.type)}
          {@const nodeProblems = byNode.get(node.id) ?? []}
          {@const hasError = nodeProblems.some((p) => p.severity === "error")}
          {@const target =
            link !== null && link.hover === node.id
              ? canConnect(workflow, link.from, link.port, node.id).ok
              : null}
          <g
            class="node {def.category}"
            class:selected={selection?.kind === "node" && selection.id === node.id}
            class:bad={hasError}
            class:ok-target={target === true}
            class:no-target={target === false}
            transform="translate({node.x} {node.y})"
            role="button"
            tabindex="0"
            aria-label="{nodeLabel(node)}, {def.label}{nodeProblems.length > 0
              ? `, ${nodeProblems.length} problem${nodeProblems.length === 1 ? '' : 's'}`
              : ''}"
            onpointerdown={(e) => onNodeDown(e, node)}
            onkeydown={(e) => onNodeKey(e, node)}
          >
            <rect class="box" width={NODE_W} height={NODE_H} rx="10" />
            <rect class="stripe" width="6" height={NODE_H} rx="3" />
            <text class="t1" x="16" y="26">{clip(nodeLabel(node), 24)}</text>
            <text class="t2" x="16" y="46">
              {clip(detail(node) ? `${def.label} · ${detail(node)}` : def.label, 30)}
            </text>
            {#if nodeProblems.length > 0}
              <circle
                class="flag"
                class:flag-error={hasError}
                cx={NODE_W - 16}
                cy="14"
                r="5"
              >
                <title>{nodeProblems.map((p) => p.message).join("\n")}</title>
              </circle>
            {/if}
            {#if !isStart(node)}
              <circle class="port in" cx="0" cy={NODE_H / 2} r={PORT_R} />
            {/if}
            {#each outputPorts(node) as port, index (port)}
              {@const at = outPoint(node, port)}
              <g>
                {#if outputPorts(node).length > 1}
                  <text class="port-label" x={NODE_W - 14} y={at.y - node.y + 4} text-anchor="end">
                    {port}
                  </text>
                {/if}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <circle
                  class="port out"
                  cx={NODE_W}
                  cy={at.y - node.y}
                  r={PORT_R}
                  data-port={index}
                  onpointerdown={(e) => onPortDown(e, node, port)}
                >
                  <title>Drag to another box to connect ({port})</title>
                </circle>
              </g>
            {/each}
          </g>
        {/each}
      </svg>
      {#if notice}
        <div class="notice {notice.kind}" role="status">{notice.text}</div>
      {/if}
    </div>

    <aside class="panel" aria-label="Properties">
      {#if selectedNode}
        {@const def = kindOf(selectedNode.type)}
        <h2>{def.label}</h2>
        <p class="wf-hint">{def.summary}</p>
        {#if def.needsApproval}
          <p class="wf-hint warn">
            This runs code or commands, so Sevak asks you to allow the workflow first.
          </p>
        {/if}
        <div class="wf-field">
          <label for="node-title">Label</label>
          <input
            id="node-title"
            class="wf-input"
            type="text"
            placeholder={def.label}
            value={selectedNode.title ?? ""}
            oninput={(e) => (selectedNode.title = e.currentTarget.value)}
          />
          {#if selectedNode.type === "keyword"}
            <p class="wf-hint">Shown as the result's title; may use {"{query}"}.</p>
          {/if}
        </div>
        {#key selectedNode.id}
          <NodeFields node={selectedNode} onset={(key, value) => setField(selectedNode, key, value)} />
        {/key}

        {#each outputPorts(selectedNode) as port (port)}
          <div class="wf-field">
            <span class="wf-label">{portLabel(port)}</span>
            {#each workflow.connection.filter((c) => c.from === selectedNode.id && portOf(c) === port) as conn (edgeKey(conn))}
              <div class="wf-row conn">
                <span class="wf-grow">{nodeLabel(nodeById(conn.to) ?? selectedNode)}</span>
                <button
                  type="button"
                  class="wf-btn small danger"
                  aria-label="Remove the connection to {conn.to}"
                  onclick={() => removeConnection(conn)}>Remove</button
                >
              </div>
            {/each}
            <select
              class="wf-input"
              aria-label="Connect {port} to"
              value=""
              onchange={(e) => {
                addFromPanel(selectedNode, port, e.currentTarget.value);
                e.currentTarget.value = "";
              }}
            >
              <option value="">Connect to…</option>
              {#each targetsFor(selectedNode, port) as other (other.id)}
                <option value={other.id}>{nodeLabel(other)} ({other.id})</option>
              {/each}
            </select>
          </div>
        {/each}

        <div class="wf-field">
          <label for="node-id">Id</label>
          <input
            id="node-id"
            class="wf-input mono"
            type="text"
            value={selectedNode.id}
            onchange={(e) => setId(selectedNode, e.currentTarget.value, e.currentTarget)}
          />
          <p class="wf-hint">
            {#if selectedNode.type === "external"}
              <code>sevak --trigger {currentFolder ?? "&lt;folder&gt;"}/{selectedNode.id}</code>
              starts it.
            {:else}
              Names the node in the file{selectedNode.type === "hotkey" ? " and in the result id of the hotkey" : ""}.
            {/if}
          </p>
        </div>
        <button type="button" class="wf-btn danger" onclick={deleteSelection}>Delete node</button>
      {:else if selectedEdge}
        <h2>Connection</h2>
        <p>
          {nodeLabel(nodeById(selectedEdge.from) ?? workflow.node[0])}
          {#if portOf(selectedEdge) !== OUT}<span class="wf-chip">{portOf(selectedEdge)}</span>{/if}
          →
          {nodeLabel(nodeById(selectedEdge.to) ?? workflow.node[0])}
        </p>
        <button type="button" class="wf-btn danger" onclick={deleteSelection}>
          Delete connection
        </button>
      {:else}
        <h2>Workflow</h2>
        <div class="wf-field">
          <label for="wf-name">Name</label>
          <input id="wf-name" class="wf-input" type="text" bind:value={workflow.name} />
        </div>
        <div class="wf-field">
          <label for="wf-desc">Description</label>
          <textarea id="wf-desc" class="wf-input" rows="2" bind:value={workflow.description}></textarea>
        </div>
        <div class="wf-field">
          <label for="wf-author">Author</label>
          <input id="wf-author" class="wf-input" type="text" bind:value={workflow.author} />
        </div>
        <div class="wf-field">
          <label for="wf-version">Version</label>
          <input id="wf-version" class="wf-input" type="text" bind:value={workflow.version} />
        </div>
        <div class="wf-field">
          <span class="wf-label">Variables</span>
          {#each variableRows as row, index (index)}
            <div class="wf-row var">
              <input
                class="wf-input mono"
                type="text"
                placeholder="name"
                aria-label="Variable name"
                bind:value={row.name}
                oninput={syncVariables}
              />
              <input
                class="wf-input"
                type="text"
                placeholder="value"
                aria-label="Value of {row.name || 'the variable'}"
                bind:value={row.value}
                oninput={syncVariables}
              />
              <button
                type="button"
                class="wf-btn small danger"
                aria-label="Remove the variable {row.name}"
                onclick={() => removeVariable(index)}>×</button
              >
            </div>
          {/each}
          <button type="button" class="wf-btn small" onclick={addVariable}>Add a variable</button>
          <p class="wf-hint">
            Every run starts with these: <code>{"{var:name}"}</code>, and an environment variable
            for scripts.
          </p>
        </div>
        <p class="wf-hint help">
          Drag a box to move it. Drag from the dot on its right edge to another box to connect
          them. Click a box or a line, then press Delete to remove it. Ctrl+S saves.
        </p>
      {/if}

      {#if problems.length > 0}
        <section class="problems" aria-label="Problems">
          <h3>Problems</h3>
          <ul>
            {#each problems as problem, i (i)}
              <li class={problem.severity}>
                {#if problem.node}
                  <button type="button" class="wf-link" onclick={() => focusProblem(problem)}>
                    {problem.node}
                  </button>:
                {/if}
                {problem.message}
              </li>
            {/each}
          </ul>
        </section>
      {/if}
    </aside>
  </div>
</div>

<style>
  .editor {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: flex;
    flex-direction: column;
    background: var(--bg);
    color: var(--fg);
    font-size: 13.5px;
    outline: none;
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 14px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .name {
    display: flex;
    flex-direction: column;
    line-height: 1.25;
    overflow: hidden;
  }

  .name strong,
  .name .wf-sub {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .add {
    width: 150px;
  }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }

  .banner {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 14px;
    border-bottom: 1px solid var(--warn);
    background: var(--selected);
    font-size: 12.5px;
  }

  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 1fr 292px;
  }

  .canvas {
    position: relative;
    overflow: auto;
    background: var(--bg);
  }

  svg {
    display: block;
    touch-action: none;
    user-select: none;
  }

  .grid-dot {
    fill: var(--border);
  }

  .arrow {
    fill: var(--muted);
  }

  .notice {
    position: sticky;
    left: 12px;
    bottom: 12px;
    display: inline-block;
    margin: 0 0 12px 12px;
    padding: 6px 12px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
    box-shadow: var(--shadow);
    font-size: 12.5px;
  }

  .notice.error {
    border-color: var(--error);
    color: var(--error);
  }

  .notice.ok {
    border-color: var(--ok);
    color: var(--ok);
  }

  .notice.warn {
    border-color: var(--warn);
  }

  /* Edges ------------------------------------------------------------------ */

  .edge-hit {
    fill: none;
    stroke: transparent;
    stroke-width: 14;
    cursor: pointer;
  }

  .edge-line {
    fill: none;
    stroke: var(--muted);
    stroke-width: 2;
    pointer-events: none;
  }

  .edge-line.temp {
    stroke: var(--accent-strong);
    stroke-dasharray: 5 4;
  }

  .edge.selected .edge-line {
    stroke: var(--accent-strong);
    stroke-width: 3;
  }

  /* Nodes ------------------------------------------------------------------ */

  .node {
    cursor: grab;
    outline: none;
  }

  .node:active {
    cursor: grabbing;
  }

  .box {
    fill: var(--input-bg);
    stroke: var(--input-border);
    stroke-width: 1.5;
  }

  .node:hover .box {
    stroke: var(--accent-strong);
  }

  .node:focus-visible .box {
    stroke: var(--accent);
    stroke-width: 3;
  }

  .node.selected .box {
    stroke: var(--accent-strong);
    stroke-width: 3;
  }

  .node.bad .box {
    stroke: var(--error);
  }

  .node.ok-target .box {
    stroke: var(--ok);
    stroke-width: 3;
  }

  .node.no-target .box {
    stroke: var(--error);
    stroke-dasharray: 4 3;
  }

  .stripe {
    fill: #8b8aa0;
  }

  .node.trigger .stripe {
    fill: #f59e0b;
  }

  .node.input .stripe {
    fill: #d97706;
  }

  .node.action .stripe {
    fill: #3b82f6;
  }

  .node.utility .stripe {
    fill: #8b5cf6;
  }

  .node.output .stripe {
    fill: #10b981;
  }

  .t1 {
    fill: var(--fg);
    font-size: 13px;
    font-weight: 600;
  }

  .t2 {
    fill: var(--muted);
    font-size: 11px;
  }

  .port-label {
    fill: var(--muted);
    font-size: 10px;
    pointer-events: none;
  }

  .port {
    fill: var(--bg);
    stroke: var(--muted);
    stroke-width: 2;
  }

  .port.out {
    cursor: crosshair;
  }

  .port.out:hover {
    fill: var(--accent);
    stroke: var(--accent-strong);
  }

  .flag {
    fill: var(--warn);
  }

  .flag.flag-error {
    fill: var(--error);
  }

  /* Side panel ---------------------------------------------------------------- */

  .panel {
    overflow-y: auto;
    padding: 14px;
    border-left: 1px solid var(--border);
    background: var(--surface);
  }

  .panel h2 {
    margin: 0 0 2px;
    font-size: 15px;
  }

  .panel h3 {
    margin: 0 0 4px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }

  .conn,
  .var {
    margin-bottom: 5px;
  }

  .var .wf-input:first-child {
    flex: 0 0 38%;
  }

  .wf-hint.warn {
    color: var(--warn);
  }

  .help {
    margin-top: 14px;
  }

  .problems {
    margin-top: 16px;
    padding-top: 10px;
    border-top: 1px solid var(--border);
  }

  .problems ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .problems li {
    margin-bottom: 6px;
    font-size: 12px;
    line-height: 1.4;
  }

  .problems li.error {
    color: var(--error);
  }

  .problems li.warning {
    color: var(--warn);
  }

  @media (max-width: 760px) {
    .body {
      grid-template-columns: 1fr 240px;
    }
  }
</style>

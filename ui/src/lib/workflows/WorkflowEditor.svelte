<script lang="ts">
  // The visual builder: nodes as boxes on an SVG canvas, connectors between
  // output and input ports, a side panel for the selected node. It edits a
  // copy of the workflow and writes it with `save_workflow` (workflow.toml).
  //
  // Mouse: drag a box to move it, drag from an output dot to another box to
  // connect, click a box or a line to select it, Delete removes the selection.
  // Keyboard: Tab to a box, arrows move it, Enter selects it; the side panel
  // can add and remove connections without dragging. Press ? for every shortcut
  // (the list is SHORTCUTS below, and docs/keyboard.md).
  import "./workflows.css";
  import { onMount, tick } from "svelte";
  import ConfigBuilder from "./ConfigBuilder.svelte";
  import NodeFields from "./NodeFields.svelte";
  import Markdown from "../Markdown.svelte";
  import { t, tHtml, type MessageKey } from "../i18n.svelte";
  import { checkWorkflow, saveWorkflow } from "./ipc";
  import {
    alignBoxes,
    boxesInRect,
    distributeBoxes,
    followConnection,
    nearestInDirection,
    type AlignMode,
    type Axis,
    type Direction,
  } from "./layout";
  import {
    CATEGORIES,
    categoryLabel,
    CANCEL,
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
  /** Every selected box (Shift/Ctrl/Cmd+click, rubber band); `selection` is the one the panel shows. */
  let multi = $state<string[]>([]);
  let lastAlign = $state<AlignMode>("left");
  let helpOpen = $state(false);
  let rubber = $state<{ x1: number; y1: number; x2: number; y2: number; add: boolean } | null>(null);
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

  /** A node being dragged, with where every selected box started. */
  let drag: { id: string; dx: number; dy: number; origins: Map<string, Point> } | null = null;
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
    if (multi.length > 1) return undefined;
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

  /** The shortcut list for the `?` overlay; the keys are the same on every layout. */
  const SHORTCUTS: [string, MessageKey][] = [
    ["Click · Shift/Ctrl/Cmd+Click", "workflows.editor.shortcut.select"],
    ["Drag (empty canvas)", "workflows.editor.shortcut.rubber"],
    ["Alt+Click", "workflows.editor.shortcut.alignTo"],
    ["Shift+Alt+Click", "workflows.editor.shortcut.autoConnect"],
    ["← ↑ → ↓ · Shift", "workflows.editor.shortcut.move"],
    ["← ↑ → ↓ (nothing selected)", "workflows.editor.shortcut.scroll"],
    ["Ctrl/Cmd + ← ↑ → ↓", "workflows.editor.shortcut.nearest"],
    ["Ctrl/Cmd + Alt + ← →", "workflows.editor.shortcut.follow"],
    ["D", "workflows.editor.shortcut.disable"],
    ["Delete", "workflows.editor.shortcut.delete"],
    ["Esc", "workflows.editor.shortcut.escape"],
    ["Ctrl/Cmd + S", "workflows.editor.shortcut.save"],
    ["Double-click (note)", "workflows.editor.shortcut.note"],
    ["?", "workflows.editor.shortcut.help"],
  ];

  const ALIGNS: [AlignMode, MessageKey][] = [
    ["left", "workflows.editor.alignLeft"],
    ["hcenter", "workflows.editor.alignCenter"],
    ["top", "workflows.editor.alignTop"],
    ["vmiddle", "workflows.editor.alignMiddle"],
  ];

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

  function selectOnly(id: string) {
    selection = { kind: "node", id };
    multi = [id];
  }

  function clearSelection() {
    selection = null;
    multi = [];
  }

  function onNodeDown(e: PointerEvent, node: WfNode) {
    if (e.button !== 0) return;
    e.stopPropagation();
    root?.focus();
    if (e.altKey && e.shiftKey) {
      // Shift+Alt+click: wire the selected box's first output to this one.
      const from = selectedNode ?? (multi.length === 1 ? nodeById(multi[0]) : undefined);
      if (from && from.id !== node.id) {
        const port = outputPorts(from)[0];
        if (port) connect(from.id, port, node.id);
      }
      return;
    }
    if (e.altKey) {
      // Alt+click: line the others up with this box.
      if (multi.length > 0 && !multi.includes(node.id)) multi = [...multi, node.id];
      if (multi.length > 1) alignSelection(lastAlign, node.id);
      else selectOnly(node.id);
      return;
    }
    if (e.shiftKey || e.ctrlKey || e.metaKey) {
      if (multi.includes(node.id)) multi = multi.filter((id) => id !== node.id);
      else multi = [...multi.filter((id) => nodeById(id)), node.id];
      selection = multi.length > 0 ? { kind: "node", id: multi[multi.length - 1] } : null;
      return;
    }
    if (!multi.includes(node.id)) selectOnly(node.id);
    else selection = { kind: "node", id: node.id };
    const p = toCanvas(e);
    const origins = new Map<string, Point>();
    for (const id of multi) {
      const other = nodeById(id);
      if (other) origins.set(id, { x: other.x, y: other.y });
    }
    drag = { id: node.id, dx: p.x - node.x, dy: p.y - node.y, origins };
    svg!.setPointerCapture(e.pointerId);
  }

  function onPortDown(e: PointerEvent, node: WfNode, port: string) {
    if (e.button !== 0) return;
    e.stopPropagation();
    const p = toCanvas(e);
    selectOnly(node.id);
    link = { from: node.id, port, x: p.x, y: p.y, hover: null };
    svg!.setPointerCapture(e.pointerId);
    root?.focus();
  }

  function onBackgroundDown(e: PointerEvent) {
    const add = e.shiftKey || e.ctrlKey || e.metaKey;
    if (!add) clearSelection();
    confirmingDiscard = false;
    root?.focus();
    if (e.button === 0 && svg) {
      const p = toCanvas(e);
      rubber = { x1: p.x, y1: p.y, x2: p.x, y2: p.y, add };
      svg.setPointerCapture(e.pointerId);
    }
  }

  function onMove(e: PointerEvent) {
    if (drag) {
      const node = nodeById(drag.id);
      const start = drag.origins.get(drag.id);
      if (!node || !start) return;
      const p = toCanvas(e);
      const x = snap(p.x - drag.dx);
      const y = snap(p.y - drag.dy);
      // Everything selected moves by the same amount as the grabbed box.
      for (const [id, origin] of drag.origins) {
        const other = nodeById(id);
        if (!other) continue;
        other.x = id === drag.id ? x : Math.max(0, origin.x + (x - start.x));
        other.y = id === drag.id ? y : Math.max(0, origin.y + (y - start.y));
      }
    } else if (rubber) {
      const p = toCanvas(e);
      rubber.x2 = p.x;
      rubber.y2 = p.y;
    } else if (link) {
      const p = toCanvas(e);
      link.x = p.x;
      link.y = p.y;
      link.hover = nodeAt(p)?.id ?? null;
      autoScroll(e);
    }
  }

  function onUp(e: PointerEvent) {
    if (svg?.hasPointerCapture(e.pointerId)) svg.releasePointerCapture(e.pointerId);
    if (drag) {
      drag = null;
    } else if (rubber) {
      const band = rubber;
      rubber = null;
      if (Math.abs(band.x2 - band.x1) > 4 || Math.abs(band.y2 - band.y1) > 4) {
        const hit = boxesInRect(workflow.node, { x: band.x1, y: band.y1 }, { x: band.x2, y: band.y2 });
        multi = band.add ? [...new Set([...multi, ...hit])] : hit;
        selection = multi.length > 0 ? { kind: "node", id: multi[multi.length - 1] } : null;
      }
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
    rubber = null;
  }

  /** Dragging a connection near the canvas edge scrolls the canvas. */
  function autoScroll(e: PointerEvent) {
    if (!wrap) return;
    const rect = wrap.getBoundingClientRect();
    const edge = 40;
    const speed = 24;
    if (e.clientX > rect.right - edge) wrap.scrollBy({ left: speed });
    else if (e.clientX < rect.left + edge) wrap.scrollBy({ left: -speed });
    if (e.clientY > rect.bottom - edge) wrap.scrollBy({ top: speed });
    else if (e.clientY < rect.top + edge) wrap.scrollBy({ top: -speed });
  }

  function connect(from: string, port: string, to: string) {
    const verdict = canConnect(workflow, from, port, to);
    if (!verdict.ok) {
      say(verdict.reason);
      return;
    }
    workflow.connection.push({ from, port, to });
    multi = [];
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
    if (helpOpen) {
      if (e.key === "Escape" || e.key === "?") {
        e.preventDefault();
        helpOpen = false;
      }
      return;
    }
    const mod = e.ctrlKey || e.metaKey;
    const arrows: Record<string, Direction> = {
      ArrowLeft: "left",
      ArrowRight: "right",
      ArrowUp: "up",
      ArrowDown: "down",
    };
    if (e.key === "?") {
      e.preventDefault();
      helpOpen = true;
    } else if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      deleteSelection();
    } else if (e.key === "Escape") {
      if (link) link = null;
      else clearSelection();
    } else if ((e.key === "d" || e.key === "D") && !mod && !e.altKey) {
      e.preventDefault();
      toggleDisabled();
    } else if (arrows[e.key]) {
      e.preventDefault();
      onArrow(e, arrows[e.key], mod);
    }
  }

  function onNodeKey(e: KeyboardEvent, node: WfNode) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      selectOnly(node.id);
    }
  }

  function focusedNodeId(e: KeyboardEvent): string | undefined {
    return (e.target as HTMLElement).closest?.("[data-node]")?.getAttribute("data-node") ?? undefined;
  }

  async function reveal(id: string) {
    selectOnly(id);
    const node = nodeById(id);
    if (!node || !wrap) return;
    wrap.scrollTo({
      left: Math.max(0, Math.min(wrap.scrollLeft, node.x - 40), node.x + NODE_W + 40 - wrap.clientWidth),
      top: Math.max(0, Math.min(wrap.scrollTop, node.y - 40), node.y + NODE_H + 40 - wrap.clientHeight),
    });
    await tick();
    svg?.querySelector<SVGElement>(`[data-node="${CSS.escape(id)}"]`)?.focus();
  }

  function onArrow(e: KeyboardEvent, direction: Direction, mod: boolean) {
    const focused = focusedNodeId(e);
    const from = selection?.kind === "node" ? selection.id : focused;
    if (mod && e.altKey) {
      // Ctrl/Cmd+Alt: along the connections.
      if (!from || (direction !== "left" && direction !== "right")) return;
      const next = followConnection(workflow.connection, from, direction);
      if (next) void reveal(next);
      return;
    }
    if (mod) {
      // Ctrl/Cmd: the nearest box that way (the top-left one when none is selected).
      if (from) {
        const next = nearestInDirection(workflow.node, from, direction);
        if (next) void reveal(next);
      } else if (workflow.node.length > 0) {
        const first = [...workflow.node].sort((a, b) => a.y - b.y || a.x - b.x)[0];
        void reveal(first.id);
      }
      return;
    }
    const step = e.shiftKey ? 40 : 10;
    const [dx, dy] = {
      left: [-step, 0],
      right: [step, 0],
      up: [0, -step],
      down: [0, step],
    }[direction];
    if (focused && !multi.includes(focused)) selectOnly(focused);
    if (multi.length === 0) {
      // Nothing selected: the arrows scroll the canvas, four times as far with Shift.
      wrap?.scrollBy({ left: dx * 4, top: dy * 4 });
      return;
    }
    for (const id of multi) {
      const node = nodeById(id);
      if (!node) continue;
      node.x = Math.max(0, node.x + dx);
      node.y = Math.max(0, node.y + dy);
    }
  }

  // ---- alignment ----------------------------------------------------------------

  function alignSelection(mode: AlignMode, referenceId?: string) {
    lastAlign = mode;
    const picked = multi.map((id) => nodeById(id)).filter((n): n is WfNode => !!n);
    if (picked.length < 2) return;
    const reference = referenceId ? nodeById(referenceId) : undefined;
    applyPlacements(alignBoxes(picked, mode, reference));
  }

  function distributeSelection(axis: Axis) {
    const picked = multi.map((id) => nodeById(id)).filter((n): n is WfNode => !!n);
    if (picked.length < 3) return;
    applyPlacements(distributeBoxes(picked, axis));
  }

  function applyPlacements(placed: { id: string; x: number; y: number }[]) {
    for (const spot of placed) {
      const node = nodeById(spot.id);
      if (!node) continue;
      node.x = Math.max(0, Math.round(spot.x));
      node.y = Math.max(0, Math.round(spot.y));
    }
  }

  // ---- disabling and notes ----------------------------------------------------------

  /** `D`: switch the selected connection, or the selected boxes, off and on. */
  function toggleDisabled() {
    if (selection?.kind === "edge") {
      if (selectedEdge) setConnectionDisabled(selectedEdge, !selectedEdge.disabled);
    } else if (multi.length > 0) {
      const picked = multi.map((id) => nodeById(id)).filter((n): n is WfNode => !!n);
      const off = picked.some((n) => !n.disabled);
      for (const node of picked) setNodeDisabled(node, off);
    }
  }

  function setNodeDisabled(node: WfNode, value: boolean) {
    if (value) node.disabled = true;
    else delete node.disabled;
  }

  function setConnectionDisabled(conn: WfConnection, value: boolean) {
    if (value) conn.disabled = true;
    else delete conn.disabled;
  }

  async function editNote(node: WfNode) {
    selectOnly(node.id);
    await tick();
    document.getElementById("node-note")?.focus();
  }

  function deleteSelection() {
    if (multi.length > 0 && selection?.kind !== "edge") {
      for (const id of [...multi]) removeNode(workflow, id);
    } else if (selection?.kind === "edge") {
      const key = selection.key;
      workflow.connection = workflow.connection.filter((c) => edgeKey(c) !== key);
    }
    clearSelection();
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
    selectOnly(node.id);
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
      say(t("workflows.editor.badId"));
      input.value = node.id;
      return;
    }
    if (workflow.node.some((n) => n.id === next)) {
      say(t("workflows.editor.idTaken"));
      input.value = node.id;
      return;
    }
    const old = node.id;
    renameNode(workflow, old, next);
    selectOnly(next);
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
    port === THEN
      ? t("workflows.editor.portThen")
      : port === ELSE
        ? t("workflows.editor.portElse")
        : port === CANCEL
          ? t("workflows.editor.portCancel")
          : port.startsWith("button")
            ? t("workflows.editor.portButton", { n: port.slice("button".length) })
            : t("workflows.editor.portNext");

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
    selectOnly(node.id);
    wrap?.scrollTo({
      left: Math.max(0, node.x - 80),
      top: Math.max(0, node.y - 80),
      behavior: window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth",
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
    say(needsAllow ? t("workflows.editor.savedNeedsAllow") : t("workflows.editor.saved"), "ok");
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
  aria-label={t("workflows.editor.region")}
  tabindex="0"
  bind:this={root}
  onkeydown={onKeydown}
>
  <header class="bar">
    <button type="button" class="wf-btn" onclick={back}>{t("workflows.editor.back")}</button>
    <div class="wf-grow name">
      <strong>{workflow.name || t("workflows.editor.untitled")}</strong>
      <span class="wf-sub">
        {currentFolder
          ? t("workflows.editor.folder", { folder: currentFolder })
          : t("workflows.editor.notSaved")}{dirty ? ` · ${t("workflows.editor.unsavedChanges")}` : ""}
      </span>
    </div>
    {#if errors.length > 0}
      <span class="wf-chip error" role="status">{t("workflows.editor.problemsToFix", { count: errors.length })}</span>
    {/if}
    <label class="add">
      <span class="visually-hidden">{t("workflows.editor.addNode")}</span>
      <select class="wf-input" bind:value={addValue} onchange={onAdd} aria-label={t("workflows.editor.addNode")}>
        <option value="">{t("workflows.editor.addNodeOption")}</option>
        {#each CATEGORIES as category (category)}
          <optgroup label={categoryLabel(category)}>
            {#each KINDS.filter((k) => k.category === category) as kind (kind.type)}
              <option value={kind.type}>{kind.label}</option>
            {/each}
          </optgroup>
        {/each}
      </select>
    </label>
    <div class="align" role="group" aria-label={t("workflows.editor.alignGroup")}>
      {#each ALIGNS as [mode, key] (mode)}
        <button
          type="button"
          class="wf-btn small"
          disabled={multi.length < 2}
          title="{t('workflows.editor.alignGroup')}: {t(key)}"
          onclick={() => alignSelection(mode)}>{t(key)}</button
        >
      {/each}
      <button
        type="button"
        class="wf-btn small"
        disabled={multi.length < 3}
        onclick={() => distributeSelection("horizontal")}>{t("workflows.editor.distributeH")}</button
      >
      <button
        type="button"
        class="wf-btn small"
        disabled={multi.length < 3}
        onclick={() => distributeSelection("vertical")}>{t("workflows.editor.distributeV")}</button
      >
    </div>
    <button type="button" class="wf-btn" onclick={tidy} title={t("workflows.editor.tidyTitle")}>
      {t("workflows.editor.tidy")}
    </button>
    <button
      type="button"
      class="wf-btn"
      aria-label={t("workflows.editor.shortcutsTitle")}
      title={t("workflows.editor.shortcutsTitle")}
      onclick={() => (helpOpen = true)}>?</button
    >
    <button type="button" class="wf-btn primary" disabled={!canSave} onclick={save}>
      {saving ? t("common.saving") : t("common.save")}
    </button>
  </header>

  {#if confirmingDiscard}
    <div class="banner" role="alert">
      <span>{t("workflows.editor.unsavedBanner")}</span>
      <button type="button" class="wf-btn small danger" onclick={() => onclose(changed)}>
        {t("workflows.editor.discard")}
      </button>
      <button type="button" class="wf-btn small" onclick={() => (confirmingDiscard = false)}>
        {t("workflows.editor.keepEditing")}
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
        aria-label={t("workflows.editor.canvas")}
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
              class:disabled={conn.disabled}
              class:selected={selection?.kind === "edge" && selection.key === edgeKey(conn)}
              onpointerdown={(e) => {
                e.stopPropagation();
                multi = [];
                selection = { kind: "edge", key: edgeKey(conn) };
                root?.focus();
              }}
            >
              <path {d} class="edge-hit" />
              <path {d} class="edge-line" marker-end="url(#wf-arrow)" />
              {#if conn.disabled}<title>{t("workflows.editor.disabledConnection")}</title>{/if}
            </g>
          {/if}
        {/each}

        {#if rubber}
          <rect
            class="rubber"
            x={Math.min(rubber.x1, rubber.x2)}
            y={Math.min(rubber.y1, rubber.y2)}
            width={Math.abs(rubber.x2 - rubber.x1)}
            height={Math.abs(rubber.y2 - rubber.y1)}
          />
        {/if}

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
            class:selected={multi.includes(node.id)}
            class:disabled={node.disabled}
            class:bad={hasError}
            class:ok-target={target === true}
            class:no-target={target === false}
            transform="translate({node.x} {node.y})"
            role="button"
            tabindex="0"
            data-node={node.id}
            aria-pressed={multi.includes(node.id)}
            aria-label={nodeProblems.length > 0
              ? t("workflows.editor.nodeAriaProblems", { label: nodeLabel(node), kind: def.label, count: nodeProblems.length })
              : t("workflows.editor.nodeAria", { label: nodeLabel(node), kind: def.label })}
            onpointerdown={(e) => onNodeDown(e, node)}
            onkeydown={(e) => onNodeKey(e, node)}
          >
            <rect class="box" width={NODE_W} height={NODE_H} rx="10" />
            <rect class="stripe" width="6" height={NODE_H} rx="3" />
            <text class="t1" x="16" y="26">{clip(nodeLabel(node), 24)}</text>
            <text class="t2" x="16" y="46">
              {clip(detail(node) ? `${def.label} · ${detail(node)}` : def.label, 30)}
            </text>
            {#if node.note?.trim()}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <text
                class="note"
                x="2"
                y={NODE_H + 16}
                ondblclick={(e) => {
                  e.stopPropagation();
                  void editNote(node);
                }}
              >
                {clip(node.note, 34)}
                <title>{node.note}{"\n"}{t("workflows.editor.noteEdit")}</title>
              </text>
            {/if}
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
                  <title>{t("workflows.editor.portTitle", { port })}</title>
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

    <aside class="panel" aria-label={t("workflows.editor.properties")}>
      {#if selectedNode}
        {@const def = kindOf(selectedNode.type)}
        <h2>{def.label}</h2>
        <p class="wf-hint">{def.summary}</p>
        {#if def.needsApproval}
          <p class="wf-hint warn">{t("workflows.editor.needsApproval")}</p>
        {/if}
        <div class="wf-field">
          <label for="node-title">{t("workflows.editor.label")}</label>
          <input
            id="node-title"
            class="wf-input"
            type="text"
            placeholder={def.label}
            value={selectedNode.title ?? ""}
            oninput={(e) => (selectedNode.title = e.currentTarget.value)}
          />
          {#if selectedNode.type === "keyword"}
            <p class="wf-hint">{t("workflows.editor.keywordTitleHint")}</p>
          {/if}
        </div>
        <label class="wf-check">
          <input
            type="checkbox"
            checked={!!selectedNode.disabled}
            onchange={(e) => setNodeDisabled(selectedNode, e.currentTarget.checked)}
          />
          {t("workflows.editor.disableNode")}
        </label>
        {#if selectedNode.disabled}
          <p class="wf-hint">{t("workflows.editor.disableNodeHint")}</p>
        {/if}
        {#key selectedNode.id}
          <NodeFields
            node={selectedNode}
            folder={currentFolder}
            onset={(key, value) => setField(selectedNode, key, value)}
          />
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
                  aria-label={t("workflows.editor.removeConnectionTo", { node: conn.to })}
                  onclick={() => removeConnection(conn)}>{t("common.remove")}</button
                >
              </div>
            {/each}
            <select
              class="wf-input"
              aria-label={t("workflows.editor.connectPort", { port })}
              value=""
              onchange={(e) => {
                addFromPanel(selectedNode, port, e.currentTarget.value);
                e.currentTarget.value = "";
              }}
            >
              <option value="">{t("workflows.editor.connectTo")}</option>
              {#each targetsFor(selectedNode, port) as other (other.id)}
                <option value={other.id}>{nodeLabel(other)} ({other.id})</option>
              {/each}
            </select>
          </div>
        {/each}

        <div class="wf-field">
          <label for="node-id">{t("workflows.editor.id")}</label>
          <input
            id="node-id"
            class="wf-input mono"
            type="text"
            value={selectedNode.id}
            onchange={(e) => setId(selectedNode, e.currentTarget.value, e.currentTarget)}
          />
          <p class="wf-hint">
            {#if selectedNode.type === "external"}
              {@html tHtml("workflows.editor.externalStarts", { path: `${currentFolder ?? "<folder>"}/${selectedNode.id}` })}
            {:else}
              {selectedNode.type === "hotkey" ? t("workflows.editor.idHintHotkey") : t("workflows.editor.idHint")}
            {/if}
          </p>
        </div>
        <button type="button" class="wf-btn danger" onclick={deleteSelection}>{t("workflows.editor.deleteNode")}</button>
      {:else if multi.length > 1}
        <h2>{t("workflows.editor.multiSelected", { count: multi.length })}</h2>
        <p class="wf-hint">{t("workflows.editor.multiHint")}</p>
      {:else if selectedEdge}
        <h2>{t("workflows.editor.connection")}</h2>
        <p>
          {nodeLabel(nodeById(selectedEdge.from) ?? workflow.node[0])}
          {#if portOf(selectedEdge) !== OUT}<span class="wf-chip">{portOf(selectedEdge)}</span>{/if}
          →
          {nodeLabel(nodeById(selectedEdge.to) ?? workflow.node[0])}
        </p>
        <button
          type="button"
          class="wf-btn"
          onclick={() => setConnectionDisabled(selectedEdge, !selectedEdge.disabled)}
        >
          {selectedEdge.disabled
            ? t("workflows.editor.enableConnection")
            : t("workflows.editor.disableConnection")}
        </button>
        {#if selectedEdge.disabled}
          <p class="wf-hint">{t("workflows.editor.connectionDisabledHint")}</p>
        {/if}
        <button type="button" class="wf-btn danger" onclick={deleteSelection}>
          {t("workflows.editor.deleteConnection")}
        </button>
      {:else}
        <h2>{t("workflows.editor.workflow")}</h2>
        <div class="wf-field">
          <label for="wf-name">{t("workflows.editor.name")}</label>
          <input id="wf-name" class="wf-input" type="text" bind:value={workflow.name} />
        </div>
        <div class="wf-field">
          <label for="wf-desc">{t("workflows.editor.description")}</label>
          <textarea id="wf-desc" class="wf-input" rows="2" bind:value={workflow.description}></textarea>
          {#if workflow.description.trim() !== ""}
            <div class="wf-desc-preview" role="region" aria-label={t("workflows.editor.descriptionPreview")}>
              <Markdown source={workflow.description} compact />
            </div>
          {/if}
        </div>
        <div class="wf-field">
          <label for="wf-author">{t("workflows.editor.author")}</label>
          <input id="wf-author" class="wf-input" type="text" bind:value={workflow.author} />
        </div>
        <div class="wf-field">
          <label for="wf-version">{t("workflows.editor.version")}</label>
          <input id="wf-version" class="wf-input" type="text" bind:value={workflow.version} />
        </div>
        <div class="wf-field">
          <span class="wf-label">{t("workflows.editor.variables")}</span>
          {#each variableRows as row, index (index)}
            <div class="wf-row var">
              <input
                class="wf-input mono"
                type="text"
                placeholder={t("workflows.editor.namePlaceholder")}
                aria-label={t("workflows.editor.variableName")}
                bind:value={row.name}
                oninput={syncVariables}
              />
              <input
                class="wf-input"
                type="text"
                placeholder={t("workflows.editor.valuePlaceholder")}
                aria-label={row.name
                  ? t("workflows.editor.valueOf", { name: row.name })
                  : t("workflows.editor.valueOfUnnamed")}
                bind:value={row.value}
                oninput={syncVariables}
              />
              <button
                type="button"
                class="wf-btn small danger"
                aria-label={t("workflows.editor.removeVariable", { name: row.name })}
                onclick={() => removeVariable(index)}>×</button
              >
            </div>
          {/each}
          <button type="button" class="wf-btn small" onclick={addVariable}>{t("workflows.editor.addVariable")}</button>
          <p class="wf-hint">{@html tHtml("workflows.editor.variablesHint")}</p>
        </div>
        <ConfigBuilder
          fields={workflow.config ?? []}
          onchange={(next) => (workflow.config = next)}
        />
        <p class="wf-hint help">{t("workflows.editor.help")}</p>
      {/if}

      {#if problems.length > 0}
        <section class="problems" aria-label={t("workflows.editor.problems")}>
          <h3>{t("workflows.editor.problems")}</h3>
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

  {#if helpOpen}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="help-backdrop" onclick={() => (helpOpen = false)}>
      <div
        class="help"
        role="dialog"
        aria-modal="true"
        aria-label={t("workflows.editor.shortcutsTitle")}
        tabindex="-1"
        onclick={(e) => e.stopPropagation()}
      >
        <h2>{t("workflows.editor.shortcutsTitle")}</h2>
        <dl>
          {#each SHORTCUTS as [keys, key] (key)}
            <dt><kbd>{keys}</kbd></dt>
            <dd>{t(key)}</dd>
          {/each}
        </dl>
        <button type="button" class="wf-btn" onclick={() => (helpOpen = false)}>
          {t("workflows.editor.shortcutsClose")}
        </button>
      </div>
    </div>
  {/if}
</div>

<style>
  .wf-desc-preview {
    margin-top: 6px;
    padding: 6px 10px;
    border: 1px dashed var(--border);
    border-radius: 6px;
    max-height: 160px;
    overflow: auto;
  }

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

  /* The editor itself takes focus (for its keys); show it when that was by keyboard. */
  .editor:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -2px;
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

  .edge.disabled .edge-line {
    stroke-dasharray: 6 5;
    opacity: 0.45;
  }

  .node.disabled .box,
  .node.disabled .stripe {
    opacity: 0.45;
  }

  .node.disabled .box {
    stroke-dasharray: 6 4;
  }

  .note {
    font-size: 11px;
    font-style: italic;
    fill: var(--muted, currentColor);
    cursor: text;
  }

  .rubber {
    fill: color-mix(in srgb, var(--accent, #4c8bf5) 15%, transparent);
    stroke: var(--accent, #4c8bf5);
    stroke-dasharray: 4 3;
    pointer-events: none;
  }

  .align {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
  }

  .help-backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 0.4);
  }

  .help {
    max-width: min(560px, 92vw);
    max-height: 86vh;
    overflow: auto;
    padding: 16px 20px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--bg);
    color: var(--fg);
  }

  .help dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 6px 14px;
    margin: 0 0 12px;
  }

  .help dd {
    margin: 0;
  }

  .help kbd {
    font-family: inherit;
    font-size: 12px;
    padding: 1px 6px;
    border: 1px solid var(--border);
    border-radius: 4px;
    white-space: nowrap;
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

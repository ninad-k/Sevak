// Pure geometry for the canvas: aligning, distributing and moving between
// nodes. Nothing here touches the DOM or the workflow; each function takes
// plain boxes and returns new positions, so it is easy to test.
import { NODE_H, NODE_W } from "./model";

export interface Box {
  id: string;
  x: number;
  y: number;
  w?: number;
  h?: number;
}

export interface Placement {
  id: string;
  x: number;
  y: number;
}

export type AlignMode = "left" | "hcenter" | "top" | "vmiddle";
export type Axis = "horizontal" | "vertical";
export type Direction = "left" | "right" | "up" | "down";

const wOf = (b: Box) => b.w ?? NODE_W;
const hOf = (b: Box) => b.h ?? NODE_H;
const cx = (b: Box) => b.x + wOf(b) / 2;
const cy = (b: Box) => b.y + hOf(b) / 2;

/**
 * Aligns `boxes` on one edge or centre line. With `reference` (Alt+click in the
 * editor) the line is that box's; otherwise it is the bounds of the selection.
 */
export function alignBoxes(boxes: Box[], mode: AlignMode, reference?: Box): Placement[] {
  if (boxes.length === 0) return [];
  const bounds = {
    left: Math.min(...boxes.map((b) => b.x)),
    right: Math.max(...boxes.map((b) => b.x + wOf(b))),
    top: Math.min(...boxes.map((b) => b.y)),
    bottom: Math.max(...boxes.map((b) => b.y + hOf(b))),
  };
  return boxes.map((b) => {
    let { x, y } = b;
    switch (mode) {
      case "left":
        x = reference ? reference.x : bounds.left;
        break;
      case "hcenter":
        x = (reference ? cx(reference) : (bounds.left + bounds.right) / 2) - wOf(b) / 2;
        break;
      case "top":
        y = reference ? reference.y : bounds.top;
        break;
      case "vmiddle":
        y = (reference ? cy(reference) : (bounds.top + bounds.bottom) / 2) - hOf(b) / 2;
        break;
    }
    return { id: b.id, x, y };
  });
}

/**
 * Spaces `boxes` evenly along `axis` between the outermost two, which stay
 * where they are. Fewer than three boxes have nothing to distribute.
 */
export function distributeBoxes(boxes: Box[], axis: Axis): Placement[] {
  if (boxes.length < 3) return boxes.map((b) => ({ id: b.id, x: b.x, y: b.y }));
  const horizontal = axis === "horizontal";
  const size = (b: Box) => (horizontal ? wOf(b) : hOf(b));
  const pos = (b: Box) => (horizontal ? b.x : b.y);
  const sorted = [...boxes].sort((a, b) => pos(a) - pos(b) || a.id.localeCompare(b.id));
  const first = sorted[0];
  const last = sorted[sorted.length - 1];
  const span = pos(last) + size(last) - pos(first);
  const used = sorted.reduce((sum, b) => sum + size(b), 0);
  const gap = (span - used) / (sorted.length - 1);
  let at = pos(first);
  const placed = new Map<string, number>();
  for (const b of sorted) {
    placed.set(b.id, at);
    at += size(b) + gap;
  }
  return boxes.map((b) => ({
    id: b.id,
    x: horizontal ? (placed.get(b.id) as number) : b.x,
    y: horizontal ? b.y : (placed.get(b.id) as number),
  }));
}

/**
 * The box nearest to `from` in `direction`, judged by centres: it must lie
 * strictly that way, and sideways drift counts double so a box straight ahead
 * beats a nearer one far off to the side.
 */
export function nearestInDirection(boxes: Box[], from: string, direction: Direction): string | null {
  const origin = boxes.find((b) => b.id === from);
  if (!origin) return null;
  let best: { id: string; score: number } | null = null;
  for (const b of boxes) {
    if (b.id === origin.id) continue;
    const dx = cx(b) - cx(origin);
    const dy = cy(b) - cy(origin);
    const along = direction === "right" ? dx : direction === "left" ? -dx : direction === "down" ? dy : -dy;
    const across = direction === "left" || direction === "right" ? Math.abs(dy) : Math.abs(dx);
    if (along <= 0) continue;
    const score = along + across * 2;
    if (!best || score < best.score || (score === best.score && b.id < best.id)) {
      best = { id: b.id, score };
    }
  }
  return best?.id ?? null;
}

/**
 * Moves along the wires: `right` goes to the first node `from` feeds, `left`
 * to the first node that feeds it, both in connection (file) order.
 */
export function followConnection(
  connections: { from: string; to: string }[],
  from: string,
  direction: "left" | "right",
): string | null {
  if (direction === "right") return connections.find((c) => c.from === from)?.to ?? null;
  return connections.find((c) => c.to === from)?.from ?? null;
}

/** The ids of the boxes a rubber band between corners `a` and `b` touches. */
export function boxesInRect(
  boxes: Box[],
  a: { x: number; y: number },
  b: { x: number; y: number },
): string[] {
  const left = Math.min(a.x, b.x);
  const right = Math.max(a.x, b.x);
  const top = Math.min(a.y, b.y);
  const bottom = Math.max(a.y, b.y);
  return boxes
    .filter(
      (box) => box.x < right && box.x + wOf(box) > left && box.y < bottom && box.y + hOf(box) > top,
    )
    .map((box) => box.id);
}

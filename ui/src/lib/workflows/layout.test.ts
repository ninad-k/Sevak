import { describe, expect, it } from "vitest";
import { NODE_H, NODE_W } from "./model";
import {
  alignBoxes,
  boxesInRect,
  distributeBoxes,
  followConnection,
  nearestInDirection,
} from "./layout";

const boxes = [
  { id: "a", x: 0, y: 0 },
  { id: "b", x: 300, y: 50 },
  { id: "c", x: 100, y: 200 },
];
const at = (list: { id: string; x: number; y: number }[], id: string) =>
  list.find((p) => p.id === id)!;

describe("alignBoxes", () => {
  it("aligns left, centres and tops to the selection bounds", () => {
    expect(alignBoxes(boxes, "left").map((p) => p.x)).toEqual([0, 0, 0]);
    expect(alignBoxes(boxes, "top").map((p) => p.y)).toEqual([0, 0, 0]);
    const centre = (0 + 300 + NODE_W) / 2;
    expect(alignBoxes(boxes, "hcenter").map((p) => p.x + NODE_W / 2)).toEqual([centre, centre, centre]);
    const middle = (0 + 200 + NODE_H) / 2;
    expect(alignBoxes(boxes, "vmiddle").map((p) => p.y + NODE_H / 2)).toEqual([middle, middle, middle]);
  });

  it("keeps the other axis", () => {
    expect(at(alignBoxes(boxes, "left"), "b").y).toBe(50);
  });

  it("aligns to a reference box", () => {
    const placed = alignBoxes(boxes, "left", boxes[1]);
    expect(placed.map((p) => p.x)).toEqual([300, 300, 300]);
    const mid = alignBoxes(boxes, "vmiddle", boxes[1]);
    expect(at(mid, "a").y).toBe(50);
  });

  it("handles an empty selection", () => {
    expect(alignBoxes([], "left")).toEqual([]);
  });
});

describe("distributeBoxes", () => {
  it("spaces the middle boxes evenly between the outer two", () => {
    const row = [
      { id: "a", x: 0, y: 0 },
      { id: "b", x: 100, y: 0 },
      { id: "c", x: 1000, y: 0 },
    ];
    const placed = distributeBoxes(row, "horizontal");
    const gap = (1000 + NODE_W - 3 * NODE_W) / 2;
    expect(at(placed, "a").x).toBe(0);
    expect(at(placed, "c").x).toBe(1000);
    expect(at(placed, "b").x).toBe(NODE_W + gap);
  });

  it("works vertically and does not depend on input order", () => {
    const col = [
      { id: "c", x: 0, y: 600 },
      { id: "a", x: 0, y: 0 },
      { id: "b", x: 0, y: 10 },
    ];
    const placed = distributeBoxes(col, "vertical");
    const gap = (600 + NODE_H - 3 * NODE_H) / 2;
    expect(at(placed, "b").y).toBe(NODE_H + gap);
    expect(at(placed, "c").y).toBe(600);
  });

  it("leaves fewer than three boxes alone", () => {
    expect(distributeBoxes(boxes.slice(0, 2), "horizontal")).toEqual([
      { id: "a", x: 0, y: 0 },
      { id: "b", x: 300, y: 50 },
    ]);
  });
});

describe("nearestInDirection", () => {
  const grid = [
    { id: "m", x: 200, y: 200 },
    { id: "r", x: 500, y: 200 },
    { id: "r2", x: 800, y: 200 },
    { id: "rfar", x: 400, y: 600 },
    { id: "l", x: -100, y: 210 },
    { id: "u", x: 200, y: 0 },
    { id: "d", x: 210, y: 400 },
  ];
  it("picks the box straight ahead over a nearer one off to the side", () => {
    expect(nearestInDirection(grid, "m", "right")).toBe("r");
    expect(nearestInDirection(grid, "m", "left")).toBe("l");
    expect(nearestInDirection(grid, "m", "up")).toBe("u");
    expect(nearestInDirection(grid, "m", "down")).toBe("d");
  });
  it("returns null when nothing lies that way or the origin is unknown", () => {
    expect(nearestInDirection(grid, "r2", "right")).toBeNull();
    expect(nearestInDirection(grid, "zzz", "right")).toBeNull();
  });
});

describe("followConnection", () => {
  const wires = [
    { from: "k", to: "a" },
    { from: "k", to: "b" },
    { from: "x", to: "b" },
  ];
  it("goes to the first downstream or upstream node", () => {
    expect(followConnection(wires, "k", "right")).toBe("a");
    expect(followConnection(wires, "b", "left")).toBe("k");
    expect(followConnection(wires, "a", "right")).toBeNull();
    expect(followConnection(wires, "k", "left")).toBeNull();
  });
});

describe("boxesInRect", () => {
  it("selects boxes the rubber band touches, from any corner", () => {
    expect(boxesInRect(boxes, { x: 350, y: 60 }, { x: 250, y: 20 })).toEqual(["b"]);
    expect(boxesInRect(boxes, { x: -10, y: -10 }, { x: 5, y: 5 })).toEqual(["a"]);
    expect(boxesInRect(boxes, { x: 1000, y: 1000 }, { x: 1100, y: 1100 })).toEqual([]);
  });
});

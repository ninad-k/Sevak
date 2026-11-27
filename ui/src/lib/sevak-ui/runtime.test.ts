import { describe, expect, it } from "vitest";
import {
  definitionPorts,
  filterItems,
  fuzzy,
  initialState,
  progressValue,
  resolveValue,
  substitute,
} from "./runtime";
import type { UiDefinition } from "./types";

const scope = { state: { a: "x", n: 3, list: ["p", "q"] }, vars: { v: "V" }, query: "Q" };

describe("substitute", () => {
  it("fills state, vars and the query, also in composite strings", () => {
    expect(substitute("{var:v} is {state:a} for {query}", scope)).toBe("V is x for Q");
    expect(substitute("{state:n}/{state:list}", scope)).toBe("3/p, q");
  });
  it("blanks unknown ids and leaves other braces alone", () => {
    expect(substitute("{state:none}|{var:none}|{other}|{param:x}", scope)).toBe("||{other}|{param:x}");
  });
  it("does not read inherited properties as variables", () => {
    expect(substitute("{var:constructor}", scope)).toBe("");
  });
  it("only text values are templates", () => {
    expect(resolveValue("{state:a}!", scope)).toBe("x!");
    expect(resolveValue(true, scope)).toBe(true);
  });
});

describe("initialState", () => {
  it("gives every input a default and keeps the definition's values", () => {
    const definition: UiDefinition = {
      state: { name: "Ada" },
      root: {
        type: "column",
        children: [
          { type: "textfield", id: "name" },
          { type: "textarea", id: "body" },
          { type: "checkbox", id: "c", label: "c" },
          { type: "slider", id: "s", min: 2, max: 9 },
          { type: "picker", id: "p", options: [{ label: "A", value: "a" }] },
          { type: "list", id: "l", items: [], multi: true },
        ],
      },
    };
    expect(initialState(definition)).toEqual({ name: "Ada", body: "", c: false, s: 2, p: "a", l: [] });
  });
});

describe("filtering", () => {
  it("matches letters in order", () => {
    expect(fuzzy("gml", "Gmail")).toBe(true);
    expect(fuzzy("lmg", "Gmail")).toBe(false);
    const items = [{ title: "Apple" }, { title: "Banana", subtitle: "yellow" }];
    expect(filterItems(items, "ylw")).toEqual([items[1]]);
    expect(filterItems(items, " ")).toEqual(items);
  });
});

describe("progress and ports", () => {
  it("clamps numbers and reads templates", () => {
    expect(progressValue(2, scope)).toBe(1);
    expect(progressValue("{state:n}", { ...scope, state: { n: "0.25" } })).toBe(0.25);
    expect(progressValue("{state:a}", scope)).toBeNull();
    expect(progressValue(undefined, scope)).toBeNull();
  });
  it("lists output ports then closed", () => {
    expect(
      definitionPorts({
        type: "row",
        children: [
          { type: "button", label: "a", events: { click: [{ output: "save" }, { output: "closed" }] } },
          { type: "button", label: "b", events: { click: [{ output: "save" }, { notify: "x" }] } },
        ],
      }),
    ).toEqual(["save", "closed"]);
  });
});

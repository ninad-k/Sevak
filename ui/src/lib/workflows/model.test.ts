import { describe, expect, it } from "vitest";
import rustModel from "../../../../crates/sevak-plugins/src/workflow/model.rs?raw";
import {
  CATEGORIES,
  ELSE,
  KINDS,
  NODE_H,
  NODE_W,
  OUT,
  THEN,
  canConnect,
  categoryLabel,
  inPoint,
  isStart,
  kindOf,
  makeNode,
  newWorkflow,
  normalize,
  outPoint,
  outputPorts,
  removeNode,
  renameNode,
  toSave,
  uniqueId,
  wouldLoop,
  type Category,
  type NodeType,
  type Workflow,
} from "./model";

/** A keyword trigger feeding a transform feeding a copy: a -> b -> c. */
function chain(): Workflow {
  const w = newWorkflow();
  w.node = [
    { id: "a", type: "keyword", x: 0, y: 0, keyword: "k", argument: "optional" },
    { id: "b", type: "transform", x: 200, y: 0, op: "trim" },
    { id: "c", type: "copy", x: 400, y: 0 },
  ];
  w.connection = [
    { from: "a", port: OUT, to: "b" },
    { from: "b", port: OUT, to: "c" },
  ];
  return w;
}

describe("the node catalog", () => {
  it("lists every kind once, in a known category, with callable defaults", () => {
    const types = KINDS.map((k) => k.type);
    expect(new Set(types).size).toBe(types.length);
    for (const kind of KINDS) {
      expect(CATEGORIES, kind.type).toContain(kind.category);
      expect(typeof kind.defaults()).toBe("object");
      expect(new Set(kind.fields.map((f) => f.key)).size, kind.type).toBe(kind.fields.length);
    }
  });

  it("covers exactly the node types the Rust model defines", () => {
    const body = rustModel.slice(rustModel.indexOf("pub enum NodeKind"));
    const rustVariants = [...body.slice(0, body.indexOf("\n}\n")).matchAll(/^ {4}([A-Z][A-Za-z]*)\s*[{,]/gm)].map((m) =>
      m[1].replace(/([a-z])([A-Z])/g, "$1_$2").toLowerCase(),
    );
    expect(rustVariants.length).toBeGreaterThan(10);
    expect([...KINDS.map((k) => k.type)].sort()).toEqual([...rustVariants].sort());
  });

  it("finds a message for every label, summary, hint and option of the catalog", () => {
    const words: string[] = [];
    for (const kind of KINDS) {
      words.push(kind.label, kind.summary);
      for (const field of kind.fields) {
        words.push(field.label, ...(field.hint ? [field.hint] : []));
        for (const option of field.options ?? []) words.push(option.label);
      }
    }
    for (const category of CATEGORIES) words.push(categoryLabel(category));
    // A missing message comes back as its key.
    expect(words.filter((text) => text.startsWith("workflows."))).toEqual([]);
    expect(kindOf("transform").fields[0].options?.[0]).toEqual({ value: "set", label: "Set to the text" });
  });

  it("marks the nodes that run code as needing approval", () => {
    const needing = KINDS.filter((k) => k.needsApproval).map((k) => k.type).sort();
    expect(needing).toEqual(expect.arrayContaining(["run_script", "script_filter"]));
    expect(kindOf("copy").needsApproval).toBeFalsy();
  });

  it("offers the utility nodes with their categories, defaults and approval", () => {
    const expected: Record<string, Category> = {
      split_arg: "utility",
      join_arg: "utility",
      random: "utility",
      expression: "utility",
      debug: "utility",
      write_file: "action",
      reveal_file: "action",
      call_external: "action",
    };
    for (const [type, category] of Object.entries(expected)) {
      expect(kindOf(type as NodeType).type, type).toBe(type);
      expect(kindOf(type as NodeType).category, type).toBe(category);
    }
    // Writing files and calling another workflow ask for the user's Allow.
    const needing = KINDS.filter((k) => k.needsApproval).map((k) => k.type);
    expect(needing).toContain("write_file");
    expect(needing).toContain("call_external");
    expect(kindOf("split_arg").defaults()).toEqual({
      delimiter: "newline",
      custom: "",
      discard_empty: true,
      trim: true,
      prefix: "split_",
    });
    expect(kindOf("random").defaults()).toMatchObject({ mode: "number", min: 1, max: 100 });
    expect(kindOf("expression").defaults()).toMatchObject({ variable: "result", set_arg: true });
    expect(kindOf("write_file").fields.find((f) => f.key === "mode")?.options?.map((o) => o.value)).toEqual([
      "overwrite",
      "append",
      "prepend",
    ]);
    expect(kindOf("split_arg").fields[0].options?.[4]).toEqual({ value: "custom", label: "Custom text" });
  });

  it("drops empty optional fields of the new nodes when saving", () => {
    const w = newWorkflow();
    w.node.push(
      { ...makeNode(w, "random", 0, 0), id: "r" },
      { ...makeNode(w, "join_arg", 0, 0), id: "j" },
      { ...makeNode(w, "call_external", 0, 0), id: "c", workflow: "other", trigger: "go" },
    );
    const saved = toSave(w).node;
    const random = saved.find((n) => n.id === "r")!;
    expect(random).not.toHaveProperty("lines");
    expect(random).not.toHaveProperty("variable");
    expect(random).toMatchObject({ mode: "number", min: 1, max: 100 });
    expect(saved.find((n) => n.id === "j")).not.toHaveProperty("discard_empty");
    expect(saved.find((n) => n.id === "c")).toMatchObject({ workflow: "other", arg: "{query}" });
  });

  it("falls back to the first kind for an unknown type", () => {
    expect(kindOf("nonsense" as NodeType)).toBe(KINDS[0]);
  });

  it("treats triggers and inputs as starts and everything else as having an input", () => {
    const start = (type: NodeType) => isStart({ id: "x", type, x: 0, y: 0 });
    expect(start("keyword")).toBe(true);
    expect(start("script_filter")).toBe(true);
    expect(start("hotkey")).toBe(true);
    expect(start("transform")).toBe(false);
    expect(start("copy")).toBe(false);
  });

  it("gives a conditional two output ports and every other node one", () => {
    expect(outputPorts({ id: "c", type: "conditional", x: 0, y: 0 })).toEqual([THEN, ELSE]);
    expect(outputPorts({ id: "t", type: "transform", x: 0, y: 0 })).toEqual([OUT]);
  });
});

describe("filters, dialog, speech and sound nodes", () => {
  it("offers the three new inputs as starts and none of them needs approval", () => {
    for (const type of ["list_filter", "file_filter", "running_apps"] as NodeType[]) {
      expect(kindOf(type).category).toBe("input");
      expect(isStart({ id: "x", type, x: 0, y: 0 })).toBe(true);
      expect(kindOf(type).needsApproval).toBeFalsy();
    }
    expect(kindOf("dialog_conditional").category).toBe("utility");
    expect(kindOf("speak").category).toBe("output");
    expect(kindOf("play_sound").category).toBe("output");
  });

  it("gives a dialog a port per button, then cancel", () => {
    const dialog = (buttons: string[]) => ({ id: "d", type: "dialog_conditional" as const, x: 0, y: 0, buttons });
    expect(outputPorts(dialog(["Yes", "No"]))).toEqual(["button1", "button2", "cancel"]);
    expect(outputPorts(dialog(["Only"]))).toEqual(["button1", "cancel"]);
    expect(outputPorts(dialog(["a", "b", "c", "d"]))).toEqual(["button1", "button2", "button3", "cancel"]);
    // The defaults of a new node are two buttons.
    const made = makeNode(newWorkflow(), "dialog_conditional", 0, 0);
    expect(outputPorts(made)).toEqual(["button1", "button2", "cancel"]);
  });

  it("connects only the ports a dialog has", () => {
    const w = newWorkflow();
    const dialog = makeNode(w, "dialog_conditional", 0, 0);
    const copy = makeNode(w, "copy", 0, 0);
    w.node.push(dialog, copy);
    expect(canConnect(w, dialog.id, "button2", copy.id).ok).toBe(true);
    expect(canConnect(w, dialog.id, "button3", copy.id).ok).toBe(false);
    expect(canConnect(w, dialog.id, "cancel", copy.id).ok).toBe(true);
  });

  it("packs the ports of a dialog into the box", () => {
    const node = { id: "d", type: "dialog_conditional" as const, x: 0, y: 0, buttons: ["a", "b", "c"] };
    const ys = ["button1", "button2", "button3", "cancel"].map((port) => outPoint(node, port).y);
    expect(ys).toEqual([...ys].sort((a, b) => a - b));
    expect(ys[ys.length - 1]).toBeLessThan(NODE_H);
  });

  it("keeps the list items and drops empty optional lists when saving", () => {
    const w = newWorkflow();
    const list = makeNode(w, "list_filter", 0, 0);
    list.items = [{ title: "Apple", subtitle: "", arg: "a-1" }];
    const files = makeNode(w, "file_filter", 0, 0);
    w.node.push(list, files);
    const saved = toSave(w);
    expect(saved.node.find((n) => n.id === list.id)?.items).toEqual([{ title: "Apple", subtitle: "", arg: "a-1" }]);
    const savedFiles = saved.node.find((n) => n.id === files.id)!;
    expect(savedFiles.folders).toBeUndefined();
    expect(savedFiles.extensions).toBeUndefined();
  });
});

describe("building", () => {
  it("starts a new workflow with one keyword trigger", () => {
    const w = newWorkflow();
    expect(w.node).toHaveLength(1);
    expect(w.node[0]).toMatchObject({ id: "keyword", type: "keyword", argument: "optional" });
    expect(w.connection).toEqual([]);
    expect(w.enabled).toBe(true);
  });

  it("uniqueId counts up from -2", () => {
    const w = chain();
    expect(uniqueId(w, "free")).toBe("free");
    expect(uniqueId(w, "a")).toBe("a-2");
    w.node.push({ id: "a-2", type: "copy", x: 0, y: 0 });
    expect(uniqueId(w, "a")).toBe("a-3");
  });

  it("makeNode names a node after its type with dashes and applies the kind's defaults", () => {
    const w = newWorkflow();
    const node = makeNode(w, "set_variable", 10, 20);
    expect(node).toMatchObject({ id: "set-variable", type: "set_variable", x: 10, y: 20 });
    w.node.push(node);
    expect(makeNode(w, "set_variable", 0, 0).id).toBe("set-variable-2");
  });
});

describe("normalize", () => {
  it("fills in what the backend may leave out", () => {
    const loaded = {
      node: [{ id: "a", type: "copy" }],
      connection: [{ from: "a", to: "b" }],
    } as unknown as Workflow;
    const w = normalize(loaded);
    expect(w).toMatchObject({ format: 1, name: "", enabled: true, variables: {} });
    expect(w.node[0]).toMatchObject({ x: 0, y: 0 });
    expect(w.connection[0].port).toBe(OUT);
  });

  it("keeps what is there and does not alias the input's variables", () => {
    const loaded: Workflow = { ...newWorkflow(), name: "Mine", enabled: false, variables: { k: "v" } };
    const w = normalize(loaded);
    expect(w.name).toBe("Mine");
    expect(w.enabled).toBe(false);
    w.variables.k = "changed";
    expect(loaded.variables.k).toBe("v");
  });

  it("copes with a completely empty object", () => {
    expect(normalize({} as Workflow).node).toEqual([]);
  });
});

describe("toSave", () => {
  it("rounds positions and truncates numbers", () => {
    const w = newWorkflow();
    w.node.push({ id: "d", type: "delay", x: 10.6, y: 3.4, ms: 250.9 });
    const saved = toSave(w);
    const delay = saved.node.find((n) => n.id === "d")!;
    expect(delay.x).toBe(11);
    expect(delay.y).toBe(3);
    expect(delay.ms).toBe(250);
  });

  it("leaves out empty optional fields but keeps empty required ones", () => {
    const w = newWorkflow();
    w.node[0].keyword = "";
    w.node[0].subtitle = "";
    const keyword = toSave(w).node[0];
    expect("subtitle" in keyword).toBe(false);
    expect(keyword.keyword).toBe("");
  });

  it("writes every connection with an explicit port", () => {
    const w = chain();
    w.connection = [{ from: "a", to: "b" }];
    expect(toSave(w).connection).toEqual([{ from: "a", port: OUT, to: "b" }]);
  });

  it("writes notes and disabled flags only when they are used", () => {
    const w = chain();
    w.node[0].note = "  ";
    w.node[0].disabled = false;
    w.node[1].note = "why";
    w.node[1].disabled = true;
    w.connection = [
      { from: "a", to: "b", disabled: false },
      { from: "b", to: "a", disabled: true },
    ];
    const saved = toSave(w);
    expect("note" in saved.node[0] || "disabled" in saved.node[0]).toBe(false);
    expect(saved.node[1].note).toBe("why");
    expect(saved.node[1].disabled).toBe(true);
    expect(saved.connection[0]).toEqual({ from: "a", port: OUT, to: "b" });
    expect(saved.connection[1].disabled).toBe(true);
  });

  it("does not modify the workflow it is given", () => {
    const w = chain();
    w.node[1].x = 10.5;
    const before = JSON.stringify(w);
    toSave(w);
    expect(JSON.stringify(w)).toBe(before);
  });
});

describe("graph rules", () => {
  it("wouldLoop sees direct, self and transitive loops", () => {
    const w = chain();
    expect(wouldLoop(w, "a", "a")).toBe(true);
    expect(wouldLoop(w, "c", "a")).toBe(true); // a -> b -> c, so c -> a closes a loop
    expect(wouldLoop(w, "b", "a")).toBe(true);
    expect(wouldLoop(w, "a", "c")).toBe(false); // a second path, not a loop
  });

  it("wouldLoop terminates on a graph that already has a cycle", () => {
    const w = chain();
    w.connection.push({ from: "c", port: OUT, to: "b" });
    expect(wouldLoop(w, "a", "b")).toBe(false);
    expect(wouldLoop(w, "b", "c")).toBe(true);
  });

  it("canConnect explains each refusal", () => {
    const w = chain();
    w.node.push({ id: "cond", type: "conditional", x: 0, y: 0 }, { id: "d", type: "copy", x: 0, y: 0 });
    const refusal = (from: string, port: string, to: string) => {
      const result = canConnect(w, from, port, to);
      return result.ok ? "ok" : result.reason;
    };
    expect(refusal("a", OUT, "zzz")).toBe("That node does not exist.");
    expect(refusal("zzz", OUT, "a")).toBe("That node does not exist.");
    expect(refusal("b", OUT, "a")).toMatch(/has no input/);
    expect(refusal("a", THEN, "d")).toBe("No such output.");
    expect(refusal("cond", OUT, "d")).toBe("No such output.");
    expect(refusal("b", OUT, "b")).toBe("A node cannot connect to itself.");
    expect(refusal("a", OUT, "b")).toBe("Already connected.");
    expect(refusal("c", OUT, "b")).toBe("That would make a loop.");
    expect(refusal("a", OUT, "d")).toBe("ok");
    expect(refusal("cond", THEN, "d")).toBe("ok");
    expect(refusal("cond", ELSE, "d")).toBe("ok");
  });

  it("removeNode takes its connections with it", () => {
    const w = chain();
    removeNode(w, "b");
    expect(w.node.map((n) => n.id)).toEqual(["a", "c"]);
    expect(w.connection).toEqual([]);
  });

  it("renameNode updates both ends of every connection", () => {
    const w = chain();
    renameNode(w, "b", "middle");
    expect(w.node.map((n) => n.id)).toEqual(["a", "middle", "c"]);
    expect(w.connection).toEqual([
      { from: "a", port: OUT, to: "middle" },
      { from: "middle", port: OUT, to: "c" },
    ]);
  });
});

describe("geometry", () => {
  it("puts a single output at the right edge, mid height", () => {
    const node = { id: "n", type: "copy" as const, x: 100, y: 50 };
    expect(outPoint(node, OUT)).toEqual({ x: 100 + NODE_W, y: 50 + NODE_H / 2 });
  });

  it("stacks the two ports of a conditional, then before else", () => {
    const node = { id: "n", type: "conditional" as const, x: 0, y: 0 };
    const then = outPoint(node, THEN);
    const otherwise = outPoint(node, ELSE);
    expect(then.x).toBe(NODE_W);
    expect(otherwise.y).toBeGreaterThan(then.y);
    expect(outPoint(node, "unknown")).toEqual(then);
  });

  it("puts the input at the left edge", () => {
    const node = { id: "n", type: "copy" as const, x: 100, y: 50 };
    expect(inPoint(node).x).toBe(100);
  });
});

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

  it("marks the nodes that run code as needing approval", () => {
    const needing = KINDS.filter((k) => k.needsApproval).map((k) => k.type).sort();
    expect(needing).toEqual(expect.arrayContaining(["run_script", "script_filter"]));
    expect(kindOf("copy").needsApproval).toBeFalsy();
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

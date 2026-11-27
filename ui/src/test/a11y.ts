// Shared by the accessibility tests: run axe-core over what a test rendered and
// keep the violations a screen-reader or keyboard user would hit hardest.
//
// jsdom has no layout, so the rules that need one (colour contrast, which is
// covered by the theme tests in lib/themes.test.ts) cannot run here.
import axe from "axe-core";

export type Violation = axe.Result;

/** Serious and critical axe violations inside `root`. */
export async function seriousViolations(root: Element = document.body): Promise<Violation[]> {
  const result = await axe.run(root, {
    resultTypes: ["violations"],
    rules: { "color-contrast": { enabled: false } },
  });
  return result.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
}

/** A readable list of violations for an assertion message. */
export function describe(violations: Violation[]): string {
  return violations
    .map((v) => `${v.impact} ${v.id}: ${v.help}\n${v.nodes.map((n) => `    ${n.target.join(" ")}`).join("\n")}`)
    .join("\n");
}

/** jsdom lacks the layout APIs the launcher and Settings call. */
export function stubLayout(): void {
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
  Element.prototype.scrollIntoView ??= function () {};
  // axe probes a canvas for icon fonts; jsdom has none and says so loudly.
  HTMLCanvasElement.prototype.getContext = (() => null) as typeof HTMLCanvasElement.prototype.getContext;
  window.matchMedia ??= ((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener() {},
    removeListener() {},
    addEventListener() {},
    removeEventListener() {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
}

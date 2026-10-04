// Opaque sRGB colors only. Contrast follows WCAG 2.x relative luminance.
export function parseColor(input) {
  const text = input.trim();
  const hex = /^#?([\da-f]{3}|[\da-f]{6})$/i.exec(text);
  if (hex) {
    const value = hex[1].length === 3 ? [...hex[1]].map(c => c + c).join("") : hex[1];
    return [0, 2, 4].map(i => parseInt(value.slice(i, i + 2), 16));
  }
  const rgb = /^rgb\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*\)$/i.exec(text);
  if (rgb) {
    const values = rgb.slice(1).map(Number);
    return values.every(v => v <= 255) ? values : null;
  }
  const hsl = /^hsl\(\s*(-?\d+(?:\.\d+)?)\s*(?:deg)?\s*,\s*(\d+(?:\.\d+)?)%\s*,\s*(\d+(?:\.\d+)?)%\s*\)$/i.exec(text);
  if (!hsl) return null;
  const [hue, sat, light] = hsl.slice(1).map(Number);
  if (![hue, sat, light].every(Number.isFinite) || sat > 100 || light > 100) return null;
  const h = ((hue % 360) + 360) % 360 / 60;
  const l = light / 100;
  const c = (1 - Math.abs(2 * l - 1)) * sat / 100;
  const x = c * (1 - Math.abs(h % 2 - 1));
  const rgbUnit = [[c,x,0], [x,c,0], [0,c,x], [0,x,c], [x,0,c], [c,0,x]][Math.floor(h)];
  return rgbUnit.map(v => Math.round((v + l - c / 2) * 255));
}

export const hexColor = rgb => "#" + rgb.map(v => v.toString(16).padStart(2, "0")).join("").toUpperCase();

export function hslColor(rgb) {
  const [r, g, b] = rgb.map(v => v / 255);
  const max = Math.max(r, g, b), min = Math.min(r, g, b), d = max - min;
  const light = (max + min) / 2;
  const sat = d === 0 ? 0 : d / (1 - Math.abs(2 * light - 1));
  let hue = d === 0 ? 0 : max === r ? ((g - b) / d) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  hue = (hue * 60 + 360) % 360;
  const round = v => Number(v.toFixed(1));
  return `hsl(${round(hue)}, ${round(sat * 100)}%, ${round(light * 100)}%)`;
}

export function contrastRatio(a, b) {
  const luminance = rgb => rgb.map(v => {
    const s = v / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  }).reduce((sum, v, i) => sum + v * [0.2126, 0.7152, 0.0722][i], 0);
  const l1 = luminance(a), l2 = luminance(b);
  return (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05);
}

export function contrastReport(a, b) {
  const ratio = contrastRatio(a, b);
  const pass = threshold => ratio >= threshold ? "Pass" : "Fail";
  return `${hexColor(a)} on ${hexColor(b)}: ${ratio.toFixed(2)}:1\n` +
    `Normal text: AA ${pass(4.5)}; AAA ${pass(7)}\n` +
    `Large text: AA ${pass(3)}; AAA ${pass(4.5)}\n` +
    "Opaque sRGB colors. Pass/fail uses the unrounded ratio.\n" +
    "Large text: at least 18 pt, or 14 pt bold. This checks color contrast only.";
}

export function results(query) {
  const help = message => [{ key: "help", title: message, subtitle: "Enter for supported formats and examples", view: "text",
    text: "Color tools\n\ncolor #f5b52c\ncolor rgb(245, 181, 44)\ncolor hsl(41, 91%, 57%)\ncolor #fff on #111\n\nEnter a conversion to copy it. Contrast reports check WCAG 2.x text thresholds.\nSupports opaque #RGB, #RRGGBB, integer RGB and comma-separated HSL. Alpha, named colors and screen sampling are not supported." }];
  if (!query.trim()) return help("Convert a color or check contrast");
  const pair = query.split(/\s+(?:on|vs)\s+/i);
  const colors = pair.map(parseColor);
  if (pair.length > 2 || colors.some(c => c === null)) return help("Color not recognized — try #f5b52c");
  const copy = (key, title, subtitle) => ({ key, title, subtitle, icon: { kind: "builtin", name: "copy" }, action: { type: "copy_text", text: title } });
  if (colors.length === 2) {
    const report = contrastReport(...colors);
    return [{ key: "contrast", title: report.split("\n")[0], subtitle: report.split("\n")[1] + " · Enter for the report", view: "text", text: report },
      { key: "copy-report", title: "Copy contrast report", subtitle: "Copy both colors, ratio and ratings", action: { type: "copy_text", text: report } }];
  }
  const rgb = colors[0];
  return [copy("hex", hexColor(rgb), "HEX · Enter to copy"),
    copy("rgb", `rgb(${rgb.join(", ")})`, "RGB · Enter to copy"),
    copy("hsl", hslColor(rgb), "HSL · Enter to copy"),
    ...[[0,0,0], [255,255,255]].map(bg => ({ key: `contrast-${hexColor(bg)}`, title: `On ${hexColor(bg)}: ${contrastRatio(rgb,bg).toFixed(2)}:1`, subtitle: "Enter for text contrast ratings", view: "text", text: contrastReport(rgb,bg) }))];
}

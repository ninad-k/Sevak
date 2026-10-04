// Regenerates the artwork of the Windows installers from the canonical icon.
//
//   npm install --no-save --package-lock=false @napi-rs/canvas
//   node scripts/generate-installer-images.mjs [--preview]
//
// Writes to src-tauri/installer/ (committed, so CI needs no image tooling):
//   sidebar.bmp      164x314  NSIS Welcome and Finish pages
//   header.bmp       150x57   NSIS page header (installer and uninstaller)
//   wix-dialog.bmp   493x312  MSI Welcome and Exit dialogs
//   wix-banner.bmp   493x58   MSI dialog banner
// With --preview it also writes PNG copies to target/installer-preview/.
//
// The flame is cut out of assets/sevak-icon.png (its red channel is the
// anti-aliased coverage of amber on indigo), so the installer uses the very
// pixels of the app icon. Text is drawn in Segoe UI where it exists; other
// systems fall back to their sans-serif font, so regenerate on Windows.
import { mkdirSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { encodeBmp24 } from "./bmp.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));
const require = createRequire(
  process.env.SEVAK_MEDIA_NODE_MODULES
    ? join(resolve(process.env.SEVAK_MEDIA_NODE_MODULES), "__installer.cjs")
    : import.meta.url,
);
const { createCanvas, loadImage, GlobalFonts } = require("@napi-rs/canvas");

const out = join(root, "src-tauri", "installer");
const previews = join(root, "target", "installer-preview");
const wantPreview = process.argv.includes("--preview");

// App palette (ui/src styles and docs/brand.md).
const C = {
  indigoTop: "#2b2870",
  indigoBottom: "#17152b",
  indigoTile: ["#3730a3", "#1e1b4b"],
  amber: "#f59e0b",
  ink: "#1c1b2e",
  light: "#fcfcfd", // MUI_BGCOLOR in installer.nsi: the app's light background
  onDark: "#ece9fb",
  mutedOnDark: "#b9b5d8",
};

let sans = "sans-serif";
let sansBold = "sans-serif";
if (process.platform === "win32") {
  const fonts = join(process.env.WINDIR || "C:/Windows", "Fonts");
  GlobalFonts.registerFromPath(join(fonts, "segoeui.ttf"), "Sevak Installer");
  GlobalFonts.registerFromPath(join(fonts, "segoeuib.ttf"), "Sevak Installer Bold");
  sans = "Sevak Installer";
  sansBold = "Sevak Installer Bold";
}

// ---- The flame, cut out of the icon -------------------------------------

async function loadFlame() {
  const icon = await loadImage(join(root, "assets", "sevak-icon.png"));
  const canvas = createCanvas(icon.width, icon.height);
  const ctx = canvas.getContext("2d");
  ctx.drawImage(icon, 0, 0);
  const { data } = ctx.getImageData(0, 0, icon.width, icon.height);
  const at = (x, y) => (y * icon.width + x) * 4;
  const bg = data[at(8, 8)]; // red channel of the indigo background
  const bowl = at(Math.round(icon.width * 0.5), Math.round(icon.height * 0.85));
  const fg = data[bowl]; // red channel of the amber lamp bowl
  const amber = [data[bowl], data[bowl + 1], data[bowl + 2]];

  const cut = ctx.createImageData(icon.width, icon.height);
  let [x0, y0, x1, y1] = [icon.width, icon.height, 0, 0];
  for (let y = 0; y < icon.height; y += 1) {
    for (let x = 0; x < icon.width; x += 1) {
      const i = at(x, y);
      const a = Math.min(1, Math.max(0, (data[i] - bg) / (fg - bg)));
      cut.data[i] = amber[0];
      cut.data[i + 1] = amber[1];
      cut.data[i + 2] = amber[2];
      cut.data[i + 3] = Math.round(a * 255);
      if (a > 0.5) {
        x0 = Math.min(x0, x);
        y0 = Math.min(y0, y);
        x1 = Math.max(x1, x);
        y1 = Math.max(y1, y);
      }
    }
  }
  ctx.putImageData(cut, 0, 0);
  return { canvas, box: { x: x0, y: y0, w: x1 - x0 + 1, h: y1 - y0 + 1 }, amber };
}

/** Draws the flame so its bounding box is `w` wide and centred on (cx, top + h/2). */
function drawFlame(ctx, flame, cx, top, w) {
  const { box } = flame;
  const h = (w * box.h) / box.w;
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = "high";
  ctx.drawImage(flame.canvas, box.x, box.y, box.w, box.h, cx - w / 2, top, w, h);
  return h;
}

// ---- Compositions, drawn in design pixels then scaled to the BMP ---------

const SCALE = 4; // supersampling; the final image is box-filtered down

function canvasFor(width, height) {
  const canvas = createCanvas(width * SCALE, height * SCALE);
  const ctx = canvas.getContext("2d");
  ctx.scale(SCALE, SCALE);
  return { canvas, ctx };
}

function finish(canvas, width, height) {
  const small = createCanvas(width, height);
  const ctx = small.getContext("2d");
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = "high";
  ctx.drawImage(canvas, 0, 0, width, height);
  return small;
}

function rounded(ctx, x, y, w, h, r) {
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + w, y, x + w, y + h, r);
  ctx.arcTo(x + w, y + h, x, y + h, r);
  ctx.arcTo(x, y + h, x, y, r);
  ctx.arcTo(x, y, x + w, y, r);
  ctx.closePath();
}

/** The app icon as a small tile: indigo rounded square with the flame. */
function drawTile(ctx, flame, x, y, size) {
  const g = ctx.createLinearGradient(x, y, x + size, y + size);
  g.addColorStop(0, C.indigoTile[0]);
  g.addColorStop(1, C.indigoTile[1]);
  ctx.fillStyle = g;
  rounded(ctx, x, y, size, size, size * 0.22);
  ctx.fill();
  const w = size * 0.52;
  const h = (w * flame.box.h) / flame.box.w;
  drawFlame(ctx, flame, x + size / 2, y + (size - h) / 2, w);
}

/** Welcome/Finish artwork: the flame glowing over indigo, the name and the tagline. */
function drawSidebar(ctx, flame, width, height) {
  const g = ctx.createLinearGradient(0, 0, 0, height);
  g.addColorStop(0, C.indigoTop);
  g.addColorStop(1, C.indigoBottom);
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, width, height);

  const cx = width / 2;
  const flameTop = height * 0.2;
  const flameWidth = width * 0.58;
  const flameHeight = (flameWidth * flame.box.h) / flame.box.w;

  const glow = ctx.createRadialGradient(cx, flameTop + flameHeight * 0.62, 4, cx, flameTop + flameHeight * 0.62, width * 0.95);
  glow.addColorStop(0, "rgba(245, 158, 11, 0.34)");
  glow.addColorStop(0.55, "rgba(245, 158, 11, 0.08)");
  glow.addColorStop(1, "rgba(245, 158, 11, 0)");
  ctx.fillStyle = glow;
  ctx.fillRect(0, 0, width, height);

  drawFlame(ctx, flame, cx, flameTop, flameWidth);

  const nameY = flameTop + flameHeight + height * 0.085;
  ctx.textAlign = "center";
  ctx.textBaseline = "alphabetic";
  ctx.fillStyle = C.onDark;
  ctx.font = `30px "${sansBold}"`;
  ctx.fillText("Sevak", cx, nameY);

  ctx.fillStyle = C.amber;
  ctx.fillRect(cx - 14, nameY + 9, 28, 2);

  ctx.fillStyle = C.mutedOnDark;
  ctx.font = `10.5px "${sans}"`;
  ctx.fillText("Your desktop.", cx, nameY + 30);
  ctx.fillText("At your service.", cx, nameY + 45);
}

/** Page-header artwork: tile and name on the light page colour, right-aligned. */
function drawHeader(ctx, flame, width, height, fill) {
  ctx.fillStyle = fill;
  ctx.fillRect(0, 0, width, height);
  const tile = Math.min(40, height - 12);
  const x = width - tile - 12;
  drawTile(ctx, flame, x, (height - tile) / 2, tile);
  ctx.textAlign = "right";
  ctx.textBaseline = "middle";
  ctx.fillStyle = C.ink;
  ctx.font = `17px "${sansBold}"`;
  ctx.fillText("Sevak", x - 9, height / 2 + 1);
}

// ---- Output --------------------------------------------------------------

function toBmp(small) {
  const { width, height } = small;
  const { data } = small.getContext("2d").getImageData(0, 0, width, height);
  return encodeBmp24(width, height, data);
}

async function main() {
  const flame = await loadFlame();
  mkdirSync(out, { recursive: true });
  if (wantPreview) mkdirSync(previews, { recursive: true });

  const jobs = {
    "sidebar": [164, 314, (ctx, w, h) => drawSidebar(ctx, flame, w, h)],
    "header": [150, 57, (ctx, w, h) => drawHeader(ctx, flame, w, h, C.light)],
    "wix-banner": [493, 58, (ctx, w, h) => drawHeader(ctx, flame, w, h, "#ffffff")],
    "wix-dialog": [
      493,
      312,
      (ctx, w, h) => {
        ctx.fillStyle = "#ffffff";
        ctx.fillRect(0, 0, w, h);
        ctx.save();
        ctx.beginPath();
        ctx.rect(0, 0, 164, h);
        ctx.clip();
        drawSidebar(ctx, flame, 164, h);
        ctx.restore();
      },
    ],
  };

  for (const [name, [width, height, paint]] of Object.entries(jobs)) {
    const { canvas, ctx } = canvasFor(width, height);
    paint(ctx, width, height);
    const small = finish(canvas, width, height);
    writeFileSync(join(out, `${name}.bmp`), toBmp(small));
    if (wantPreview) writeFileSync(join(previews, `${name}.png`), small.toBuffer("image/png"));
    console.log(`wrote src-tauri/installer/${name}.bmp (${width}x${height})`);
  }
}

await main();

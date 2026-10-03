// Documentation artwork from the real Svelte UI, using isolated example data.
// No desktop automation, native app actions, or real user files are involved.
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { join, resolve } from "node:path";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { createServer } from "vite";

const root = fileURLToPath(new URL("../../", import.meta.url));
const require = createRequire(process.env.SEVAK_MEDIA_NODE_MODULES
  ? join(resolve(process.env.SEVAK_MEDIA_NODE_MODULES), "__media.cjs") : import.meta.url);
const { chromium } = require("playwright");
const { createCanvas, loadImage, GlobalFonts } = require("@napi-rs/canvas");
const media = join(root, "docs", "media");
const work = join(root, "target", "docs-media");
await mkdir(media, { recursive: true });
await mkdir(work, { recursive: true });
const fixtures = JSON.parse(await readFile(new URL("fixtures.json", import.meta.url), "utf8"));
if (process.platform === "win32") {
  const fonts = join(process.env.WINDIR || "C:/Windows", "Fonts");
  GlobalFonts.registerFromPath(join(fonts, "segoeui.ttf"), "Sevak UI");
  GlobalFonts.registerFromPath(join(fonts, "segoeuib.ttf"), "Sevak Bold");
}
const font = process.platform === "win32" ? "Sevak UI" : "sans-serif";
const bold = process.platform === "win32" ? "Sevak Bold" : "sans-serif";
const C = { bg: "#111020", panel: "#1b1932", ink: "#f3f0ff", muted: "#aaa6c3", amber: "#f5b52c", border: "#37334f" };

const server = await createServer({
  logLevel: "error",
  configFile: join(root, "ui", "vite.config.ts"),
  server: { host: "127.0.0.1", port: 1436, strictPort: true, open: false },
  plugins: [{ name: "documentation-fixtures", enforce: "pre", transform(code, id) {
    if (!id.replaceAll("\\", "/").endsWith("/ui/src/lib/mock.ts")) return;
    return code.replace(/export function mockSearch\(query: string\): ResultDto\[\] \{[\s\S]*?\n\}/,
      `const docsRows: Record<string, ResultDto[]> = ${JSON.stringify(fixtures)};\nexport function mockSearch(query: string): ResultDto[] { return docsRows[query.trim()] ?? []; }`);
  } }],
});
let browser;
const shots = {};
try {
  await server.listen();
  browser = await chromium.launch({ headless: true, ...(process.env.SEVAK_MEDIA_BROWSER ? { executablePath: process.env.SEVAK_MEDIA_BROWSER } : { channel: "chrome" }) });
  const page = await browser.newPage({ viewport: { width: 744, height: 600 }, deviceScaleFactor: 2, colorScheme: "dark" });
  for (const [name, query] of Object.entries({ ready: "", apps: "code", calculator: "12*7", files: "f project", web: "g rust traits" })) {
    await page.goto("http://127.0.0.1:1436/");
    await page.getByRole("combobox").fill(query);
    if (query) await page.locator(".row").first().waitFor();
    await page.addStyleTag({ content: "input { caret-color: transparent !important }" });
    const path = join(media, `launcher-${name}.png`);
    await page.locator(".card").screenshot({ path, omitBackground: true });
    shots[name] = await loadImage(path);
  }
  await page.emulateMedia({ colorScheme: "light" });
  await page.goto("http://127.0.0.1:1436/");
  await page.getByRole("combobox").fill("code");
  await page.locator(".row").first().waitFor();
  await page.addStyleTag({ content: "input { caret-color: transparent !important }" });
  await page.locator(".card").screenshot({ path: join(media, "launcher-light.png"), omitBackground: true });
  shots.light = await loadImage(join(media, "launcher-light.png"));
  await page.setViewportSize({ width: 1040, height: 700 });
  await page.goto("about:blank");
  await page.goto("http://127.0.0.1:1436/#settings");
  await page.getByRole("heading", { name: "General", exact: true }).waitFor();
  await page.screenshot({ path: join(media, "settings-general.png") });
  shots.settings = await loadImage(join(media, "settings-general.png"));
  console.log("Captured six launcher states and the actual Settings component.");
} finally {
  await browser?.close();
  await server.close();
}

const logo = await loadImage(join(root, "assets", "sevak-icon.png"));
const canvas = createCanvas(1440, 860);
let ctx = canvas.getContext("2d");
function rr(x, y, w, h, radius, fill, stroke) {
  ctx.beginPath(); ctx.roundRect(x, y, w, h, radius);
  if (fill) { ctx.fillStyle = fill; ctx.fill(); }
  if (stroke) { ctx.strokeStyle = stroke; ctx.lineWidth = 1; ctx.stroke(); }
}
function text(value, x, y, size = 28, color = C.ink, weight = false, align = "left") {
  ctx.fillStyle = color; ctx.font = `${size}px "${weight ? bold : font}"`;
  ctx.textAlign = align; ctx.textBaseline = "top"; ctx.fillText(value, x, y);
}
function backdrop(w, h, light = false) {
  canvas.width = w; canvas.height = h; ctx = canvas.getContext("2d");
  ctx.fillStyle = light ? "#f6f5fb" : C.bg; ctx.fillRect(0, 0, w, h);
  if (!light) {
    const glow = ctx.createRadialGradient(w * .85, h * .2, 0, w * .85, h * .2, w * .7);
    glow.addColorStop(0, "#272244"); glow.addColorStop(1, C.bg);
    ctx.fillStyle = glow; ctx.fillRect(0, 0, w, h);
    ctx.strokeStyle = "#ffffff05"; ctx.lineWidth = 1;
    for (let x = 0; x < w; x += 80) { ctx.beginPath(); ctx.moveTo(x, 0); ctx.lineTo(x, h); ctx.stroke(); }
  }
}
function icon(x, y, size) { ctx.save(); ctx.beginPath(); ctx.roundRect(x,y,size,size,size*.23); ctx.clip(); ctx.drawImage(logo,x,y,size,size); ctx.restore(); }
function shot(img, x, y, w, shadow = true) {
  if (shadow) { ctx.shadowColor = "#00000055"; ctx.shadowBlur = 26; ctx.shadowOffsetY = 14; }
  ctx.drawImage(img, x, y, w, w * img.height / img.width);
  ctx.shadowColor = "transparent"; ctx.shadowBlur = 0; ctx.shadowOffsetY = 0;
}
function key(value, x, y, w, size=24) {
  rr(x,y,w,58,12,"#27243e",C.border); text(value,x+w/2,y+12,size,C.ink,true,"center");
}
async function save(name) { await writeFile(join(media,name),canvas.toBuffer("image/png")); }

backdrop(1440,860);
icon(80,64,68); text("Sevak",168,70,42,C.ink,true);
text("WINDOWS  /  macOS  /  LINUX",1360,89,19,C.muted,false,"right");
text("Your desktop.",80,185,82,C.ink,true);
text("At your service.",80,284,82,C.amber,true);
text("Find apps and files. Calculate. Search the web.",84,400,29,C.muted);
shot(shots.apps,240,494,960);
key("Alt",501,766,80); text("+",596,780,24,C.muted); key("Space",628,766,128);
text("Type. Enter. Done.",792,782,23,C.muted);
await save("sevak-overview.png");

backdrop(1440,980);
text("One search bar. Everyday actions.",64,56,48,C.ink,true);
text("A few characters are all it takes to get started.",64,120,25,C.muted);
const tiles = [
  ["01", "Launch an app", "Type an app name, then press Enter.", shots.apps],
  ["02", "Calculate and copy", "Type 12*7. Press Enter to copy 84.", shots.calculator],
  ["03", "Find a file", "Use f + a filename in your indexed folders.", shots.files],
  ["04", "Search the web", "Use g, yt or gh + your search terms.", shots.web],
];
tiles.forEach(([num,title,desc,img],i)=>{
  const x=64+(i%2)*672, y=200+Math.floor(i/2)*362;
  rr(x,y,640,334,22,C.panel,C.border);
  text(num,x+28,y+28,19,C.amber,true); text(title,x+75,y+22,30,C.ink,true);
  text(desc,x+28,y+74,20,C.muted);
  shot(img,x+24,y+137,592,false);
});
text("Actual Sevak UI components · illustrative sample results",64,943,17,C.muted);
await save("sevak-features.png");

backdrop(1440,520,true);
text("Comfortable in light or dark.",64,52,46,"#1c1b2e",true);
text("Follow your system theme, or choose your own in Settings.",64,119,24,"#6b6a80");
text("LIGHT",64,196,18,"#6b6a80",true); text("DARK",756,196,18,"#6b6a80",true);
shot(shots.light,64,240,620); shot(shots.apps,756,240,620);
await save("sevak-themes.png");

backdrop(1440,1120);
text("Make it work your way.",80,58,50,C.ink,true);
text("Shortcut, appearance, search engines, folders and plugins — in Settings.",80,128,25,C.muted);
shot(shots.settings,140,224,1160);
text("Actual Settings UI · sample configuration",80,1061,18,C.muted);
await save("sevak-settings.png");

const workflow=`<svg xmlns="http://www.w3.org/2000/svg" width="1440" height="290" viewBox="0 0 1440 290" role="img" aria-labelledby="title desc"><title id="title">Open, search, act</title><desc id="desc">Press Alt plus Space to open Sevak. Type an app, file, calculation or web keyword. Press Enter to launch, open or copy.</desc><rect width="1440" height="290" rx="24" fill="#1b1932"/>${[
  [64,"01","OPEN","Alt + Space","Bring Sevak into focus."],
  [530,"02","SEARCH","Type what you need","Apps, files, math or web."],
  [996,"03","ACT","Press Enter","Launch, open or copy."]
].map(([x,n,label,title,sub])=>`<g font-family="Segoe UI,Arial,sans-serif"><text x="${x}" y="62" font-size="18" font-weight="700" fill="#f5b52c">${n} / ${label}</text><text x="${x}" y="132" font-size="34" font-weight="700" fill="#f3f0ff">${title}</text><text x="${x}" y="188" font-size="23" fill="#aaa6c3">${sub}</text></g>`).join("")}<path d="M449 124h28m-9-9 10 9-10 9M915 124h28m-9-9 10 9-10 9" fill="none" stroke="#f5b52c" stroke-width="3"/></svg>`;
await writeFile(join(media,"sevak-workflow.svg"),workflow);
console.log("Rendered overview, feature gallery, themes, settings and workflow.");

if (!process.argv.includes("--video")) process.exit(0);
function frame(t) {
  backdrop(1080,1920);
  icon(72,80,64); text("Sevak",160,86,42,C.ink,true);
  text("AT YOUR SERVICE",72,1750,20,C.muted,true);
  rr(72,1830,936,5,2,"#39334e"); rr(72,1830,936*Math.min(t/12,1),5,2,C.amber);
  const bounds=[0,1.8,4,6.8,9.4,12];
  let s=0; while(s<4 && t>=bounds[s+1])s++;
  const a=Math.min((t-bounds[s])/.24,1); ctx.save(); ctx.globalAlpha=Math.max(0,a); ctx.translate(0,(1-a)*30);
  if(s===0){
    icon(396,385,288); text("Your desktop.",540,800,82,C.ink,true,"center");
    text("At your service.",540,900,82,C.amber,true,"center");
    text("Sevak means “one who serves”.",540,1110,33,C.muted,false,"center");
    text("A keyboard-first launcher.",540,1180,33,C.muted,false,"center");
  }else if(s===1){
    text("01 / OPEN",72,370,26,C.amber,true); text("One shortcut.",72,445,76,C.ink,true); text("Ready to help.",72,539,76,C.ink,true);
    key("Alt",282,738,160,40); text("+",482,748,36,C.muted); key("Space",552,738,246,40);
    shot(shots.ready,72,952,936); text("Press Alt + Space to open Sevak.",72,1240,34,C.muted);
  }else if(s===2){
    text("02 / SEARCH",72,370,26,C.amber,true); text("Type a name.",72,445,76,C.ink,true); text("Find your app.",72,539,76,C.ink,true);
    shot(shots.apps,72,846,936); text("Use ↑ / ↓ to choose a result.",72,1240,34,C.muted); key("Enter to launch",72,1360,344,30);
  }else if(s===3){
    text("03 / ACT",72,370,26,C.amber,true); text("Quick math.",72,445,76,C.ink,true); text("One key to copy.",72,539,70,C.ink,true);
    shot(shots.calculator,72,862,936); text("12 × 7 = 84",72,1220,48,C.amber,true); key("Enter to copy",72,1350,325,30);
  }else{
    icon(414,360,252); text("Back to your flow.",540,740,74,C.ink,true,"center");
    text("Apps  ·  Files  ·  Math  ·  Web",540,888,36,C.muted,false,"center");
    key("Alt + Space",344,1070,392,40); text("github.com/ninad-k/Sevak",540,1280,34,C.amber,false,"center");
  }
  ctx.restore(); text("Illustrative walkthrough · sample results",540,1700,20,C.muted,false,"center");
}
frame(5.4); await save("sevak-reel-poster.png");
const sampleRate=48000, samples=12*sampleRate;
const wav=Buffer.alloc(44+samples*2);
wav.write("RIFF",0); wav.writeUInt32LE(wav.length-8,4); wav.write("WAVEfmt ",8); wav.writeUInt32LE(16,16);
wav.writeUInt16LE(1,20); wav.writeUInt16LE(1,22); wav.writeUInt32LE(sampleRate,24); wav.writeUInt32LE(sampleRate*2,28); wav.writeUInt16LE(2,32); wav.writeUInt16LE(16,34); wav.write("data",36); wav.writeUInt32LE(samples*2,40);
const tones=[[.08,523.25],[1.86,659.25],[4.06,783.99],[6.86,659.25],[9.46,523.25],[10.0,783.99]];
for(let i=0;i<samples;i++){
  const t=i/sampleRate; let value=0;
  for(const [start,hz] of tones){const d=t-start;if(d>=0&&d<.38)value+=.075*Math.sin(2*Math.PI*hz*d)*Math.exp(-d*12)*Math.min(d/.008,1);}
  wav.writeInt16LE(Math.round(value*32767),44+i*2);
}
const sound=join(work,"sound.wav"); await writeFile(sound,wav);
const video=join(media,"sevak-12s.mp4");
const ffmpeg=spawn(process.env.FFMPEG || "ffmpeg",["-hide_banner","-loglevel","error","-y","-f","rawvideo","-pixel_format","rgba","-video_size","1080x1920","-framerate","30","-i","pipe:0","-i",sound,"-c:v","libx264","-preset","medium","-crf","21","-pix_fmt","yuv420p","-c:a","aac","-b:a","128k","-movflags","+faststart","-t","12",video],{stdio:["pipe","inherit","inherit"]});
const finished=once(ffmpeg,"close");
for(let f=0;f<360;f++){
  frame(f/30); const pixels=ctx.getImageData(0,0,1080,1920).data;
  if(!ffmpeg.stdin.write(Buffer.from(pixels.buffer,pixels.byteOffset,pixels.byteLength))) await once(ffmpeg.stdin,"drain");
}
ffmpeg.stdin.end(); if((await finished)[0]!==0)throw new Error("Video encoding failed");
console.log("Rendered 12-second vertical MP4 with original subtle sound.");

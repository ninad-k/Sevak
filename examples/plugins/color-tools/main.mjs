import { results } from "./colors.mjs";
process.stdout.write(JSON.stringify({ items: results(process.argv.slice(2).join(" ")) }));

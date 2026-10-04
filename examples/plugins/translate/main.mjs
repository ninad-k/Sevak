import { results } from "./translate.mjs";
process.stdout.write(JSON.stringify({ items: results(process.argv.slice(2).join(" ")) }));

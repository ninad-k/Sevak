// Alfred Script Filter output: {"items": [{"uid", "title", "subtitle", "arg"}]}
// Sevak passes the query as the last command-line argument.

const query = process.argv.slice(2).join(" ").trim();

// "userAccount-ID_v2" -> ["user", "Account", "ID", "v2"]
const words = query
  .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
  .split(/[^\p{L}\p{N}]+/u)
  .filter(Boolean);

const capitalize = (word) => word.charAt(0).toUpperCase() + word.slice(1).toLowerCase();
const lower = words.map((word) => word.toLowerCase());

const conversions = [
  ["camelCase", lower.map((word, i) => (i === 0 ? word : capitalize(word))).join("")],
  ["PascalCase", lower.map(capitalize).join("")],
  ["snake_case", lower.join("_")],
  ["CONSTANT_CASE", lower.join("_").toUpperCase()],
  ["kebab-case", lower.join("-")],
  ["Title Case", lower.map(capitalize).join(" ")],
  ["lower case", lower.join(" ")],
];

const items =
  words.length === 0
    ? [{ title: "Type some words to convert", subtitle: "for example: user account id", valid: false }]
    : conversions.map(([name, value]) => ({
        uid: name, // a stable id lets Sevak learn which conversion you pick
        title: value,
        subtitle: `${name}. Enter to copy`,
        arg: value,
      }));

process.stdout.write(JSON.stringify({ items }));

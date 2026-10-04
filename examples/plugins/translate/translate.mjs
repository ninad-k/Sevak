export const languages = {
  en: "English", ar: "Arabic", hi: "Hindi", mr: "Marathi", fr: "French", de: "German",
  es: "Spanish", it: "Italian", pt: "Portuguese", ja: "Japanese", ko: "Korean",
  "zh-CN": "Chinese (Simplified)", "zh-TW": "Chinese (Traditional)", ru: "Russian",
  nl: "Dutch", tr: "Turkish", uk: "Ukrainian", pl: "Polish", id: "Indonesian",
  vi: "Vietnamese", ta: "Tamil", te: "Telugu", bn: "Bengali", ur: "Urdu",
};

export function results(query) {
  const input = query.trim();
  const [target = ""] = input.split(/\s+/);
  const text = input.slice(target.length).trim();
  const language = Object.keys(languages).find(code => code.toLowerCase() === target.toLowerCase() || languages[code].toLowerCase() === target.toLowerCase());
  const help = title => [{ key: "help", title, subtitle: "Enter for language codes and examples", view: "text",
    text: "Translate in browser\n\ntr fr hello world\ntr en bonjour\ntr hi good morning\n\n" +
      Object.entries(languages).map(([code,name]) => `${code} — ${name}`).join("\n") +
      "\n\nGoogle detects the source language. Enter opens the translation website; the plugin does not fetch or display translations locally. Text is not saved by this plugin and is sent to Google only when you open the result. Maximum input: 5,000 characters." }];
  if (!input) return help("Translate text — try tr fr hello");
  if (!language) return help(`Unknown target language: ${target.slice(0, 40)}`);
  if (!text) return help(`Type the text to translate into ${languages[language]}`);
  if ([...text].length > 5000) return help("Use 5,000 characters or fewer");
  const url = new URL("https://translate.google.com/");
  url.search = new URLSearchParams({ sl: "auto", tl: language, text, op: "translate" }).toString();
  return [{ key: `translate-${language}`, title: `Translate into ${languages[language]}`, subtitle: `${text.slice(0, 160)} · Opens Google Translate`, icon: { kind: "builtin", name: "web" }, action: { type: "open_url", url: url.href } }];
}

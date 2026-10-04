import { createInterface } from "node:readline";
import { Timer } from "./timer.mjs";

const send = message => process.stdout.write(JSON.stringify(message) + "\n");
let timer;
for await (const line of createInterface({ input: process.stdin, crlfDelay: Infinity })) {
  let message;
  try { message = JSON.parse(line); } catch { continue; }
  if (!message || typeof message !== "object") continue;
  if (message.type === "initialize") send({ type: "ready" });
  else if (message.type === "shutdown") break;
  else if (message.type === "query" || message.type === "execute") {
    try {
      timer ??= new Timer(process.env.SEVAK_PLUGIN_DATA);
      if (message.type === "query") {
        const items = timer.results(typeof message.input === "string" ? message.input : "");
        send({ type: "results", request_id: message.request_id, items });
      } else {
        timer.execute(message.payload);
      }
    } catch (error) {
      if (message.type === "query") {
        send({ type: "results", request_id: message.request_id, items: [{ key: "error", title: "Pomodoro needs attention", subtitle: error.message, view: "text", text: error.message }] });
      } else send({ type: "error", message: error.message });
    }
  }
}

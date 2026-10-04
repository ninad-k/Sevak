import { mkdirSync, readFileSync, writeFileSync, renameSync } from "node:fs";
import { join } from "node:path";

const MAX_MS = 180 * 60_000;
const initial = () => ({ version: 1, phase: "focus", status: "idle", remaining: 0, deadline: null, completed: 0 });
const helpText = "Pomodoro\n\npomo start 25 — start a focus session\npomo break 5 — start a break\npomo pause — pause the current timer\npomo resume — continue it\npomo reset — stop the timer (keeps completed count)\npomo status — check progress\n\nDurations are whole minutes from 1 to 180. Enter confirms a command.\nThe deadline is saved locally, so elapsed time survives plugin or app restarts. A completed focus counts once. Breaks start manually.\nReopen or retype pomo to refresh the status; this extension has no background alarm, notification or continuously updating countdown.";
const help = title => ({ key: "help", title, subtitle: "Enter for commands", view: "text", text: helpText });

export class Timer {
  constructor(directory, now = Date.now) {
    if (!directory) throw new Error("Sevak did not provide a plugin data folder.");
    this.directory = directory;
    this.file = join(directory, "timer.json");
    this.now = now;
    try {
      this.state = JSON.parse(readFileSync(this.file, "utf8"));
      const s = this.state;
      if (s.version !== 1 || !["focus", "break"].includes(s.phase) ||
        !["idle", "running", "paused", "complete"].includes(s.status) ||
        !Number.isFinite(s.remaining) || s.remaining < 0 || s.remaining > MAX_MS ||
        !Number.isSafeInteger(s.completed) || s.completed < 0 ||
        (s.status === "running" ? !Number.isFinite(s.deadline) || s.deadline < 0 : s.deadline !== null)) {
        throw new Error("Invalid timer state");
      }
    } catch (error) {
      if (error.code !== "ENOENT") throw new Error("Cannot read timer.json. Back up or rename it in the plugin data folder to reset the timer.");
      this.state = initial();
    }
  }

  commit(state) {
    // Replace the file only after the full new state has been written.
    mkdirSync(this.directory, { recursive: true });
    const temporary = this.file + ".tmp";
    writeFileSync(temporary, JSON.stringify(state) + "\n", { mode: 0o600 });
    renameSync(temporary, this.file);
    this.state = state;
  }

  current() {
    if (this.state.status === "running" && this.now() >= this.state.deadline) {
      this.commit({ ...this.state, status: "complete", remaining: 0, deadline: null,
        completed: this.state.completed + (this.state.phase === "focus" ? 1 : 0) });
    }
    return { ...this.state, remaining: this.state.status === "running"
      ? Math.min(MAX_MS, Math.max(0, this.state.deadline - this.now())) : this.state.remaining };
  }

  execute(payload) {
    const command = JSON.parse(payload);
    const s = this.current();
    if (["start", "break"].includes(command.op)) {
      if (!Number.isInteger(command.minutes) || command.minutes < 1 || command.minutes > 180) throw new Error("Use 1–180 whole minutes.");
      const remaining = command.minutes * 60_000;
      this.commit({ ...s, phase: command.op === "start" ? "focus" : "break", status: "running", remaining, deadline: this.now() + remaining });
    } else if (command.op === "pause" && s.status === "running") {
      this.commit({ ...s, status: "paused", deadline: null });
    } else if (command.op === "resume" && s.status === "paused") {
      this.commit({ ...s, status: "running", deadline: this.now() + s.remaining });
    } else if (command.op === "reset") {
      this.commit({ ...initial(), completed: s.completed });
    } else {
      throw new Error("That timer action is no longer available. Search pomo again.");
    }
  }

  results(query) {
    const s = this.current();
    const seconds = Math.ceil(s.remaining / 1000);
    const time = `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
    const phase = s.phase === "focus" ? "Focus" : "Break";
    const title = s.status === "idle" ? "Ready for a focus session" : s.status === "complete" ? `${phase} complete` : `${phase}: ${time} ${s.status === "paused" ? "paused" : "remaining"}`;
    const status = { key: "status", title, subtitle: `${s.completed} completed focus sessions · Enter for details`, view: "text", text: `${title}\nCompleted focus sessions: ${s.completed}\n\n${helpText}` };
    const action = (op, minutes) => {
      const title = op === "start" ? `Start ${minutes}-minute focus` : op === "break" ? `Start ${minutes}-minute break` : { pause: "Pause timer", resume: "Resume timer", reset: "Stop and reset timer" }[op];
      return { key: op, title, subtitle: ["start", "break"].includes(op) && ["running", "paused"].includes(s.status) ? "Enter to replace the current timer" : "Enter to confirm; typing does not change the timer",
        action: { type: "custom", payload: JSON.stringify({ op, ...(minutes === undefined ? {} : { minutes }) }) } };
    };
    const text = query.trim().toLowerCase();
    if (text && text !== "status") {
      const start = /^(start|break)(?:\s+(\d+))?$/.exec(text);
      if (start) {
        const minutes = start[2] === undefined ? (start[1] === "start" ? 25 : 5) : Number(start[2]);
        return minutes >= 1 && minutes <= 180 ? [action(start[1], minutes), status] : [help("Use 1–180 whole minutes"), status];
      }
      if (text === "pause" && s.status === "running" || text === "resume" && s.status === "paused" || text === "reset") return [action(text), status];
      return [help(text === "pause" || text === "resume" ? "That action is unavailable for the current timer" : "Unknown command — try pomo start 25"), status];
    }
    const actions = s.status === "running" ? [action("pause"), action("reset")]
      : s.status === "paused" ? [action("resume"), action("reset")]
      : [action("start", 25), action("break", 5)];
    return [status, ...actions, help("Pomodoro commands")];
  }
}

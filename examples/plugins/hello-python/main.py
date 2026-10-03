"""A persistent Sevak script plugin: type `hello <name>`.

Sevak starts this process on the first `hello ` query and keeps it running.
Each line on stdin is one JSON message from Sevak; each line this script prints
to stdout is one JSON message back. stdout is only for the protocol: print
debugging output to stderr, which Sevak writes to its log.

The protocol is documented in docs/plugins.md ("External plugins").
"""

import json
import os
import sys
from urllib.parse import quote


def send(message):
    # flush=True matters: Sevak waits for the line, not for a full buffer.
    print(json.dumps(message), flush=True)


def results(request_id, name):
    greeting = f"Hello, {name}!"
    return {
        "type": "results",
        "request_id": request_id,
        "items": [
            {
                # A stable key per kind of row, so Sevak can learn which one you pick.
                "key": "greeting",
                "title": greeting,
                "subtitle": "Enter to copy",
                "icon": {"kind": "builtin", "name": "copy"},
                "action": {"type": "copy_text", "text": greeting},
            },
            {
                "key": "shout",
                "title": greeting.upper(),
                "subtitle": "Enter to copy, louder",
                "icon": {"kind": "builtin", "name": "copy"},
                "action": {"type": "copy_text", "text": greeting.upper()},
            },
            {
                "key": "search",
                "title": f"Search the web for {name}",
                "icon": {"kind": "builtin", "name": "web"},
                "action": {
                    "type": "open_url",
                    "url": "https://duckduckgo.com/?q=" + quote(name),
                },
            },
            {
                # "custom" comes back to this script as an `execute` message.
                "key": "remember",
                "title": f"Remember {name}",
                "subtitle": "Appends the name to names.txt in the plugin's data folder",
                "action": {"type": "custom", "payload": name},
            },
        ],
    }


def remember(name):
    folder = os.environ.get("SEVAK_PLUGIN_DATA", ".")
    os.makedirs(folder, exist_ok=True)
    with open(os.path.join(folder, "names.txt"), "a", encoding="utf-8") as handle:
        handle.write(name + "\n")


def main():
    # Sevak escapes everything outside ASCII in the messages it sends, so the
    # default decoding is safe; this is only for text read from other places.
    for line in sys.stdin:
        message = json.loads(line)
        kind = message.get("type")
        if kind == "initialize":
            send({"type": "ready"})
        elif kind == "query":
            name = message["input"].strip() or "world"
            send(results(message["request_id"], name))
        elif kind == "execute":
            if message.get("key") == "remember":
                remember(message.get("payload", ""))
        elif kind == "shutdown":
            break
        # Unknown message types are ignored, so newer Sevaks keep working.


if __name__ == "__main__":
    main()

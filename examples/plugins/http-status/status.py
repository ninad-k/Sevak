"""HTTP status code reference for the "http" keyword (Sevak one-shot plugin).

"http 404" explains one code, "http redirect" or "http 5" lists a group, and
"http teapot" searches the names and meanings. The table is built in, so it
works offline. Standard library only; nothing is stored or sent.
"""
import json
import re
import sys

CODES = {
    100: ("Continue", "The server got the request headers; the client may send the body."),
    101: ("Switching Protocols", "The server agrees to switch protocol, for example to WebSocket."),
    102: ("Processing", "WebDAV: the server is still working on the request."),
    103: ("Early Hints", "Preload hints sent before the final response."),
    200: ("OK", "The request succeeded."),
    201: ("Created", "The request succeeded and a new resource was created."),
    202: ("Accepted", "Accepted for processing, which has not finished yet."),
    203: ("Non-Authoritative Information", "Succeeded, but a proxy changed the response."),
    204: ("No Content", "Succeeded and there is nothing to send back."),
    205: ("Reset Content", "Succeeded; the client should reset its form or view."),
    206: ("Partial Content", "Only the byte range that was asked for is returned."),
    207: ("Multi-Status", "WebDAV: several results in one response."),
    301: ("Moved Permanently", "The resource has a new permanent address; update your links."),
    302: ("Found", "Temporary redirect; keep using the original address."),
    303: ("See Other", "Get the result from another address with GET."),
    304: ("Not Modified", "The cached copy is still good; no body is sent."),
    307: ("Temporary Redirect", "Repeat the same request, same method, at another address."),
    308: ("Permanent Redirect", "Like 301, but the method must not change."),
    400: ("Bad Request", "The server cannot understand the request (malformed syntax)."),
    401: ("Unauthorized", "Authentication is missing or wrong; log in first."),
    402: ("Payment Required", "Reserved; some APIs use it for billing limits."),
    403: ("Forbidden", "Understood but refused; logging in again will not help."),
    404: ("Not Found", "Nothing at this address (or the server hides that it exists)."),
    405: ("Method Not Allowed", "The method (GET, POST...) is not supported for this resource."),
    406: ("Not Acceptable", "Nothing matches the Accept headers the client sent."),
    407: ("Proxy Authentication Required", "Log in to the proxy first."),
    408: ("Request Timeout", "The server gave up waiting for the request."),
    409: ("Conflict", "The request conflicts with the current state, such as an edit clash."),
    410: ("Gone", "Permanently removed, with no forwarding address."),
    411: ("Length Required", "A Content-Length header is required."),
    412: ("Precondition Failed", "A condition in the request headers (If-Match...) was false."),
    413: ("Content Too Large", "The request body is bigger than the server accepts."),
    414: ("URI Too Long", "The address is longer than the server accepts."),
    415: ("Unsupported Media Type", "The body's format (Content-Type) is not supported."),
    416: ("Range Not Satisfiable", "The requested byte range is outside the resource."),
    417: ("Expectation Failed", "The Expect header cannot be met."),
    418: ("I'm a teapot", "An April Fools' joke from RFC 2324: the server refuses to brew coffee."),
    421: ("Misdirected Request", "Sent to a server that cannot answer for this host."),
    422: ("Unprocessable Content", "Well formed, but the content is semantically invalid."),
    423: ("Locked", "WebDAV: the resource is locked."),
    425: ("Too Early", "The server will not risk replaying an early request."),
    426: ("Upgrade Required", "Switch to a different protocol to continue."),
    428: ("Precondition Required", "The request must be conditional (If-Match) to avoid lost updates."),
    429: ("Too Many Requests", "Rate limited; slow down and retry after the Retry-After time."),
    431: ("Request Header Fields Too Large", "The headers are too big."),
    451: ("Unavailable For Legal Reasons", "Blocked for legal reasons, such as a court order."),
    500: ("Internal Server Error", "The server hit an unexpected error."),
    501: ("Not Implemented", "The server does not support this request method."),
    502: ("Bad Gateway", "A gateway or proxy got an invalid answer from the upstream server."),
    503: ("Service Unavailable", "Overloaded or down for maintenance; try again later."),
    504: ("Gateway Timeout", "A gateway or proxy timed out waiting for the upstream server."),
    505: ("HTTP Version Not Supported", "The HTTP version is not supported."),
    507: ("Insufficient Storage", "WebDAV: the server is out of storage."),
    508: ("Loop Detected", "WebDAV: the server found an infinite loop."),
    511: ("Network Authentication Required", "Log in to the network (captive portal) first."),
}
GROUPS = {1: "Informational", 2: "Success", 3: "Redirection", 4: "Client error", 5: "Server error"}
ALIASES = {"info": 1, "informational": 1, "success": 2, "ok": 2, "redirect": 3, "redirection": 3,
           "client": 4, "error": 4, "server": 5}


def row(code):
    name, meaning = CODES[code]
    text = f"{code} {name}"
    return {
        "key": str(code),
        "title": text,
        "subtitle": f"{GROUPS[code // 100]}. {meaning}",
        "icon": {"kind": "builtin", "name": "copy"},
        "action": {"type": "copy_text", "text": text},
    }


def results(query):
    text = query.strip().lower()
    if not text:
        picks = [200, 201, 204, 301, 302, 304, 400, 401, 403, 404, 429, 500, 502, 503]
        return [row(c) for c in picks]
    if text.isdigit() and len(text) == 3:
        if int(text) in CODES:
            return [row(int(text))]
        group = int(text) // 100
        if group in GROUPS:
            return [{"key": "unknown", "title": f"{text} is not a standard {GROUPS[group].lower()} code",
                     "subtitle": f"Type {group} to list the {group}xx codes",
                     "icon": {"kind": "builtin", "name": "copy"},
                     "action": {"type": "copy_text", "text": text}}]
    shape = re.fullmatch(r"([1-5])x{0,2}", text)
    group = int(shape.group(1)) if shape else ALIASES.get(text)
    if group in GROUPS:
        return [row(c) for c in CODES if c // 100 == group]
    words = text.split()
    hits = [c for c, (name, meaning) in CODES.items()
            if all(w in (name + " " + meaning).lower() for w in words)]
    if not hits:
        return [{"key": "none", "title": "No status code matches", "subtitle": "Try a number (404), a group (redirect, 5xx) or a word (timeout)",
                 "icon": {"kind": "builtin", "name": "copy"}, "action": {"type": "copy_text", "text": "404 Not Found"}}]
    return [row(c) for c in hits[:20]]


def main():
    sys.stdout.write(json.dumps({"items": results(" ".join(sys.argv[1:]))}))


if __name__ == "__main__":
    main()

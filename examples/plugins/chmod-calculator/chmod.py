"""Unix permission calculator for the "chmod" keyword (Sevak one-shot plugin).

"chmod 755" shows rwxr-xr-x and what each class may do, "chmod rwxr-xr-x" or
"chmod -rw-r--r--" (as `ls -l` prints it) gives the octal number and the
command. A fourth leading octal digit is setuid/setgid/sticky. It only
calculates; it never touches a file. Standard library only.
"""
import json
import re
import sys

WHO = ("owner", "group", "others")


def symbolic(mode):
    out = ""
    for i in range(3):
        bits = (mode >> (6 - 3 * i)) & 7
        special = bool(mode & (0o4000 >> i))
        execute = "x" if bits & 1 else "-"
        if special:
            mark = "s" if i < 2 else "t"
            execute = mark if bits & 1 else mark.upper()
        out += ("r" if bits & 4 else "-") + ("w" if bits & 2 else "-") + execute
    return out


def parse_symbolic(text):
    text = text[-9:]
    mode = 0
    for i in range(3):
        r, w, x = text[3 * i:3 * i + 3]
        allowed = ("r-", "w-", "xsStT-" if i < 2 else "xtT-")
        if r not in allowed[0] or w not in allowed[1] or x not in allowed[2]:
            return None
        if x in "sStT" and (i == 2) != (x in "tT"):
            return None
        mode |= ((r == "r") * 4 | (w == "w") * 2 | (x in "xst") * 1) << (6 - 3 * i)
        if x in "sStT":
            mode |= 0o4000 >> i
    return mode


def parse(query):
    text = query.strip()
    if re.fullmatch(r"0?[0-7]{3}|[0-7]{4}", text):
        return int(text, 8)
    if re.fullmatch(r"[-dlcbps]?[-rwxsStT]{9}", text):
        return parse_symbolic(text)
    return None


def words(bits):
    names = [n for n, b in (("read", 4), ("write", 2), ("execute", 1)) if bits & b]
    return ", ".join(names) if names else "nothing"


def row(key, title, subtitle, copy=None):
    return {
        "key": key,
        "title": title,
        "subtitle": subtitle,
        "icon": {"kind": "builtin", "name": "copy"},
        "action": {"type": "copy_text", "text": copy if copy is not None else title},
    }


def results(query):
    if not query.strip():
        return [row(f"ex-{m}", m, f"{symbolic(int(m, 8))}  {note}. Enter to copy")
                for m, note in (("644", "files: owner writes, everyone reads"), ("755", "programs and folders"),
                                ("600", "private files such as SSH keys"), ("700", "private folders"),
                                ("777", "everyone can do everything; avoid"))]
    mode = parse(query)
    if mode is None:
        return [row("bad", "Type 755, 0644, 4755 or rwxr-xr-x", "Octal digits 0-7, or nine symbols like ls -l shows", "755")]
    octal = f"{mode & 0o7777:04o}" if mode & 0o7000 else f"{mode:03o}"
    items = [
        row("octal", octal, "Octal mode. Enter to copy"),
        row("symbolic", symbolic(mode), "Symbolic mode, as ls -l shows. Enter to copy"),
        row("command", f"chmod {octal} file", "The command. Enter to copy"),
    ]
    for i, who in enumerate(WHO):
        bits = (mode >> (6 - 3 * i)) & 7
        items.append(row(who, f"{who.capitalize()}: {words(bits)}", f"{symbolic(mode)[3 * i:3 * i + 3]} ({bits})",
                         symbolic(mode)[3 * i:3 * i + 3]))
    for bit, label in ((0o4000, "setuid: runs as the file's owner"), (0o2000, "setgid: runs as the file's group, or new files inherit the folder's group"),
                       (0o1000, "sticky: only a file's owner may delete it in this folder")):
        if mode & bit:
            items.append(row(f"special-{bit:o}", label, "Special bit"))
    if mode & 0o002 and not mode & 0o1000:
        items.append(row("warn", "World-writable: anyone can change it", "Rarely what you want", octal))
    return items


def main():
    sys.stdout.write(json.dumps({"items": results(" ".join(sys.argv[1:]))}))


if __name__ == "__main__":
    main()

import collections
import html
import re
import sys

COPYRIGHT = re.compile(r"\s*(Copyright\b(?!\s+(notice|license|holder|owner|and))|©|\([cC]\)\s*\d)")
ONE_TEXT = {"Apache License 2.0", "Mozilla Public License 2.0"}


def sections(text):
    for part in re.split(r"\n### ", "\n" + html.unescape(text))[1:]:
        name, rest = part.split("\n", 1)
        used = re.search(r"Used by (.*?)\.\n", rest)
        crates = [c for c in used.group(1).split(", ") if not c.startswith("citrine-gb")]
        if crates:
            yield name.strip(), crates, rest[used.end():].strip("\n")


def split(name, body):
    notices, text = [], []
    for line in body.splitlines():
        if COPYRIGHT.match(line) and not re.search(r"\[yyyy\]|<year>", line):
            notices.append(line.strip())
        elif not text and line.strip() in ("", name):
            continue
        else:
            text.append(line.rstrip())
    return notices, "\n".join(text).strip()


def main(source, out):
    groups = collections.OrderedDict()
    for name, crates, body in sections(open(source, encoding="utf-8").read()):
        notices, text = split(name, body)
        key = (name, name if name in ONE_TEXT else re.sub(r"\s+", " ", text))
        group = groups.setdefault(key, {"text": text, "rows": []})
        if len(text) > len(group["text"]):
            group["text"] = text
        group["rows"].append((", ".join(crates), " ".join(notices)))
    ordered = sorted(groups.items(), key=lambda item: (item[0][0], -len(item[1]["rows"])))
    with open(out, "w", encoding="utf-8") as page:
        for (name, _), group in ordered:
            page.write(f"### {name}\n\n")
            for crates, notice in sorted(group["rows"], key=lambda row: row[0].lower()):
                page.write(f"- {crates}: {notice}\n" if notice else f"- {crates}\n")
            page.write(f"\n{group['text']}\n\n")


main(sys.argv[1], sys.argv[2])

import collections
import glob
import os
import re
import sys

KINDS = [
    ("apache", "Under the Apache License 2.0:"),
    ("sdk", "Under the Android Software Development Kit License, https://developer.android.com/studio/terms:"),
    ("bsd", "Protocol Buffers, under the BSD 3-clause license below:"),
]


def resolved(deps):
    found = {}
    for line in open(deps, encoding="utf-8"):
        m = re.search(r"--- ([\w.\-]+):([\w.\-]+)(?::([\w.\-]+))?(?: -> ([\w.\-]+))?", line)
        if m and (m.group(3) or m.group(4)):
            found[(m.group(1), m.group(2))] = m.group(4) or m.group(3)
    return found


def licence(cache, group, artifact, version, depth=0):
    poms = glob.glob(os.path.join(cache, group, artifact, version, "*", f"{artifact}-{version}.pom"))
    if not poms:
        sys.exit(f"no pom for {group}:{artifact}:{version} in {cache}")
    text = open(poms[0], encoding="utf-8", errors="replace").read()
    names = " ".join(re.findall(r"<license>\s*<name>(.*?)</name>", text, re.S))
    if names:
        return names
    parent = re.search(
        r"<parent>.*?<groupId>(.*?)</groupId>.*?<artifactId>(.*?)</artifactId>.*?<version>(.*?)</version>",
        text,
        re.S,
    )
    if parent and depth < 3:
        return licence(cache, *[part.strip() for part in parent.groups()], depth + 1)
    sys.exit(f"no licence in the pom of {group}:{artifact}:{version}")


def kind(name):
    if "Android Software Development Kit" in name:
        return "sdk"
    if "BSD" in name:
        return "bsd"
    if "Apache" in name:
        return "apache"
    sys.exit(f"unknown licence {name}")


def main(deps, cache, out):
    groups = collections.defaultdict(lambda: collections.defaultdict(list))
    for (group, artifact), version in sorted(resolved(deps).items()):
        if artifact.endswith("-bom"):
            continue
        groups[kind(licence(cache, group, artifact, version))][group].append(f"{artifact} {version}")
    with open(out, "w", encoding="utf-8") as page:
        for key, intro in KINDS:
            if not groups[key]:
                continue
            page.write(f"{intro}\n\n")
            for group in sorted(groups[key]):
                page.write(f"- {group}: {', '.join(sorted(groups[key][group]))}\n")
            page.write("\n")


main(*sys.argv[1:4])

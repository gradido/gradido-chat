#!/usr/bin/env python3
"""Copies what the chat needs from a gradido checkout: locale strings, icons, motifs.

Usage: tools/sync-assets.py [path/to/gradido]   (default: ../gradido)

Run it again after the web frontend changed chat strings; the copies here
are otherwise never updated.
"""
import json
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent
GRADIDO = Path(sys.argv[1] if len(sys.argv) > 1 else HERE.parent / "gradido").resolve()

LOCALES = ["de", "en", "es", "fr", "nl", "tr", "it", "ru", "pt", "el"]
GROUPS = ["chatThread", "chatGroup", "chatForward", "chatSearch", "chatHello", "contacts"]
SINGLE = [("form", "close"), ("form", "cancel"), ("form", "save"), ("error", "session-expired")]

MDI = [
    "chat-outline", "dots-horizontal", "dots-vertical", "share", "share-outline",
    "image-outline", "email-outline", "reply-outline", "pencil-outline", "content-copy",
    "send", "check", "close", "magnify", "bell-outline", "bell-off-outline", "heart",
    "heart-outline", "chevron-up", "chevron-down", "chevron-right", "plus",
    "file-document-outline", "open-in-new", "account-group-outline", "cog-outline",
    "logout", "video-outline", "account-multiple-outline", "volume-high", "volume-off",
]
MOTIFS = ["heart-leaves", "giving-hands", "bouquet", "glowing-swirl", "morning-light"]


def locales():
    src = GRADIDO / "frontend/src/locales"
    for code in LOCALES:
        full = json.loads((src / f"{code}.json").read_text(encoding="utf-8"))
        out = {g: full[g] for g in GROUPS if g in full}
        for group, key in SINGLE:
            if key in full.get(group, {}):
                out.setdefault(group, {})[key] = full[group][key]
        (HERE / "locales" / f"{code}.json").write_text(
            json.dumps(out, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )


def icons():
    data = json.loads((GRADIDO / "node_modules/@iconify-json/mdi/icons.json").read_text())
    size = data.get("width", 24)
    for name in MDI:
        body = data["icons"][name]["body"]
        svg = f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}">{body}</svg>\n'
        (HERE / "assets/icons" / f"{name}.svg").write_text(svg)


def motifs():
    for name in MOTIFS:
        shutil.copy(GRADIDO / f"frontend/public/img/thank-you-greeting/{name}.svg", HERE / "assets/motifs")
    # The app icon is the coin on a light square; the taskbar and the tray want the coin alone,
    # so everything outside it becomes transparent. The unread one carries a gold dot in a gap.
    mask = ["-size", "192x192", "xc:black", "-fill", "white", "-draw", "circle 95.5,95.5 95.5,19.5"]
    gap = ["-fill", "black", "-draw", "circle 150,42 150,2"]
    cut = ["-alpha", "off", ")", "-alpha", "off", "-compose", "CopyOpacity", "-composite"]
    dot = ["-compose", "Over", "-fill", "#c08935", "-draw", "circle 150,42 150,10"]
    source = str(GRADIDO / "frontend/public/img/brand/app-icon-192.png")
    subprocess.run(["convert", source, "(", *mask, *cut, str(HERE / "assets/gradido.png")], check=True)
    subprocess.run(["convert", source, "(", *mask, *gap, *cut, *dot, str(HERE / "assets/gradido-unread.png")], check=True)


locales()
icons()
motifs()
print("synced from", GRADIDO)

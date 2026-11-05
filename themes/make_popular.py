#!/usr/bin/env python3
"""Builds themes/popular.json: well-known editor/terminal colour schemes as GTDCraft themes.

Colours come unchanged from each scheme's Ghostty file in mbadolato/iTerm2-Color-Schemes (the set Ghostty ships):
background, foreground and the 16 ANSI colours. GTDCraft's extra surfaces (rail, fields, lines…) are mixes of the
scheme's own background and foreground, so every theme keeps its look. Run from app/:  python3 -I themes/make_popular.py
"""
import json
import urllib.parse
import urllib.request

SRC = "https://raw.githubusercontent.com/mbadolato/iTerm2-Color-Schemes/master/ghostty/"

# (upstream file name, name shown in GTDCraft)
SCHEMES = [
    ("TokyoNight Night", "Tokyo Night"),
    ("TokyoNight Storm", "Tokyo Night Storm"),
    ("TokyoNight Moon", "Tokyo Night Moon"),
    ("TokyoNight Day", "Tokyo Night Day"),
    ("Catppuccin Mocha", "Catppuccin Mocha"),
    ("Catppuccin Macchiato", "Catppuccin Macchiato"),
    ("Catppuccin Frappe", "Catppuccin Frappé"),
    ("Catppuccin Latte", "Catppuccin Latte"),
    ("Everforest Dark Hard", "Everforest Dark"),
    ("Everforest Light Med", "Everforest Light"),
    ("Gruvbox Dark", "Gruvbox Dark"),
    ("Gruvbox Light", "Gruvbox Light"),
    ("Gruvbox Material Dark", "Gruvbox Material"),
    ("Rose Pine", "Rosé Pine"),
    ("Rose Pine Moon", "Rosé Pine Moon"),
    ("Rose Pine Dawn", "Rosé Pine Dawn"),
    ("Kanagawa Wave", "Kanagawa Wave"),
    ("Kanagawa Dragon", "Kanagawa Dragon"),
    ("Kanagawa Lotus", "Kanagawa Lotus"),
    ("Nord", "Nord"),
    ("Dracula", "Dracula"),
    ("Atom One Dark", "One Dark"),
    ("Nightfox", "Nightfox"),
    ("Carbonfox", "Carbonfox"),
    ("iTerm2 Solarized Dark", "Solarized Dark"),
    ("iTerm2 Solarized Light", "Solarized Light"),
    ("GitHub Dark Default", "GitHub Dark"),
    ("Ayu Mirage", "Ayu Mirage"),
    ("Melange Dark", "Melange"),
    ("Oxocarbon", "Oxocarbon"),
    ("Vesper", "Vesper"),
    ("Poimandres", "Poimandres"),
    ("Night Owl", "Night Owl"),
    ("Monokai Pro", "Monokai Pro"),
]


def rgb(h):
    h = h.strip().lstrip("#")
    return tuple(int(h[i : i + 2], 16) for i in (0, 2, 4))


def hexs(c):
    return "#%02x%02x%02x" % tuple(max(0, min(255, round(v))) for v in c)


def mix(a, b, t):
    a, b = rgb(a), rgb(b)
    return hexs(tuple(a[i] + (b[i] - a[i]) * t for i in range(3)))


def lum(h):
    def ch(v):
        v /= 255
        return v / 12.92 if v <= 0.03928 else ((v + 0.055) / 1.055) ** 2.4

    r, g, b = rgb(h)
    return 0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)


def contrast(a, b):
    la, lb = sorted([lum(a), lum(b)], reverse=True)
    return (la + 0.05) / (lb + 0.05)


def parse(text):
    p, kv = {}, {}
    for line in text.splitlines():
        if "=" not in line:
            continue
        k, v = (s.strip() for s in line.split("=", 1))
        if k == "palette":
            i, c = v.split("=", 1)
            p[int(i)] = c.strip()
        else:
            kv[k] = v
    return p, kv


def theme(upstream, name):
    text = urllib.request.urlopen(SRC + urllib.parse.quote(upstream), timeout=20).read().decode()
    p, kv = parse(text)
    bg, fg = kv["background"], kv["foreground"]
    dark = lum(bg) < 0.4
    edge = "#000000" if dark else fg
    blue = p[4]
    pal = {
        "bg": bg,
        "fg": fg,
        "black": p[0], "red": p[1], "green": p[2], "yellow": p[3], "blue": blue, "magenta": p[5], "cyan": p[6],
        "bright-black": p[8], "bright-magenta": p[13],
        "orange": mix(p[1], p[3], 0.5),
        "indigo": mix(blue, p[5], 0.5),
        "rail": mix(bg, edge, 0.16 if dark else 0.05),
        "inspector": mix(bg, edge, 0.08 if dark else 0.025),
        "field": mix(bg, fg, 0.07),
        "pill": mix(bg, fg, 0.09),
        "pill-hover": mix(bg, fg, 0.16),
        "line": mix(bg, fg, 0.11),
        "rail-sel": mix(bg, fg, 0.12),
        "selection": mix(bg, blue, 0.28),
        "badge": mix(bg, blue, 0.38),
        "fg-dim": mix(fg, bg, 0.38),
        "fg-faint": mix(fg, bg, 0.6),
    }
    on_accent = bg if contrast(blue, bg) >= contrast(blue, fg) else fg
    ui = {
        "bg": "bg", "fg": "fg", "accent": "blue", "fg_dim": "fg-dim", "fg_faint": "fg-faint",
        "rail": "rail", "toolbar": "rail", "column": "bg", "inspector": "inspector", "field": "field",
        "pill": "pill", "pill_hover": "pill-hover", "selection": "selection", "rail_selection": "rail-sel",
        "line": "line", "on_accent": on_accent, "overdue": "red", "due_soon": "yellow", "flagged": "orange",
        "next": "magenta", "tag_pill": "pill-hover", "tag_text": "fg-dim", "badge": "badge", "link": "cyan",
        "red": "red", "orange": "orange", "yellow": "yellow", "green": "green", "cyan": "cyan",
        "blue": "blue", "indigo": "indigo", "purple": "magenta", "violet": "bright-magenta",
    }
    return {
        "name": name,
        "author": f"colours from the “{upstream}” scheme in mbadolato/iTerm2-Color-Schemes",
        "mode": "dark" if dark else "light",
        "palette": pal,
        "ui": ui,
    }


if __name__ == "__main__":
    out = [theme(u, n) for u, n in SCHEMES]
    with open("themes/popular.json", "w") as f:
        json.dump(out, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print(f"wrote {len(out)} themes to themes/popular.json")

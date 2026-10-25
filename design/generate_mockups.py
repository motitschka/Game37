#!/usr/bin/env python3
"""Generates the Game37 screen mockups as SVG files in design/mockups/.

Cover art is placeholder gradients: real art comes from SteamGridDB at runtime.
"""
import math
import textwrap
from html import escape
from pathlib import Path

W, H = 1920, 1080
M = 96  # side margin
OUT = Path(__file__).parent / "mockups"
FONT = "Inter, 'Helvetica Neue', Arial, sans-serif"
MONO = "'JetBrains Mono', 'DejaVu Sans Mono', monospace"

C = {
    "bg": "#0B0D12",
    "surface": "#151922",
    "surface2": "#1F2431",
    "line": "#2A3142",
    "text": "#F2F4F8",
    "muted": "#8A93A6",
    "dim": "#5B6477",
    "accent": "#8B7BFF",
    "star": "#FFC84A",
    "ahead": "#3DDC84",  # Split delta: ahead of the comparison
    "behind": "#FF5C6C",  # Split delta: behind the comparison
    "best": "#FFD23F",  # Split delta: new Best Segment
    "danger": "#FF8F98",  # a destructive Settings row: softer than `behind`, so it reads as care, not alarm
}

PALETTE = [
    ("#FF6B6B", "#6B1D3A"), ("#4FACFE", "#1B2A6B"), ("#43E97B", "#0F4C3A"),
    ("#FA709A", "#5B2C83"), ("#F6D365", "#B3452C"), ("#A18CD1", "#2E1F5E"),
    ("#30CFD0", "#203A6B"), ("#FF9A44", "#7A1F2B"), ("#84FAB0", "#1F5F6B"),
    ("#E0C3FC", "#4B3B8F"), ("#FCCB90", "#8E3B46"), ("#5EE7DF", "#3B3486"),
]

# System Order: Nintendo then Sony, each by European release year.
SYSTEMS = [
    ("nes", "NES", "Nintendo · 1986", "#C62828"),
    ("gb", "Game Boy", "Nintendo · 1990", "#7A9A12"),
    ("snes", "SNES", "Nintendo · 1992", "#6A4C9C"),
    ("n64", "Nintendo 64", "Nintendo · 1997", "#1E9E57"),
    ("gbc", "Game Boy Color", "Nintendo · 1998", "#D4A017"),
    ("gba", "Game Boy Advance", "Nintendo · 2001", "#4B3FA8"),
    ("gc", "GameCube", "Nintendo · 2002", "#6A3FB5"),
    ("nds", "Nintendo DS", "Nintendo · 2005", "#7D8898"),
    ("wii", "Wii", "Nintendo · 2006", "#1F9FD6"),
    ("3ds", "Nintendo 3DS", "Nintendo · 2011", "#C9302C"),
    ("wiiu", "Wii U", "Nintendo · 2012", "#0A8FBF"),
    ("switch", "Switch", "Nintendo · 2017", "#E60012"),
    ("ps1", "PlayStation", "Sony · 1995", "#8C8C96"),
    ("ps2", "PlayStation 2", "Sony · 2000", "#2443B8"),
    ("psp", "PSP", "Sony · 2005", "#3A3F4B"),
    ("ps3", "PlayStation 3", "Sony · 2007", "#2B2F3A"),
    ("ps4", "PlayStation 4", "Sony · 2013", "#0B3D91"),
]
SYS = {s[0]: s for s in SYSTEMS}
SHORT = {"nes": "NES", "gb": "GB", "snes": "SNES", "n64": "N64", "gbc": "GBC", "gba": "GBA",
         "gc": "GAMECUBE", "nds": "DS", "wii": "WII", "3ds": "3DS", "wiiu": "WII U",
         "switch": "SWITCH", "ps1": "PS1", "ps2": "PS2", "psp": "PSP", "ps3": "PS3", "ps4": "PS4"}


def game(title, sys, hours=None):
    return {"title": title, "sys": sys, "hours": hours, "pal": sum(map(ord, title)) % len(PALETTE)}


FAVS = [game("Super Mario World", "snes", "12h 40m"), game("The Wind Waker", "gc", "38h 12m"),
        game("Metroid Prime", "gc", "31h 05m"), game("Pokémon Emerald", "gba", "54h 20m"),
        game("Gran Turismo 4", "ps2", "22h 47m")]
CONTINUE = [game("Mario Kart 8 Deluxe", "switch"), game("Persona 4", "ps2"), game("Crash Bandicoot", "ps1"),
            game("Super Mario 64", "n64"), game("Demon's Souls", "ps3"), game("Kirby Planet Robobot", "3ds"),
            game("Bloodborne", "ps4")]
RECENT = [game("Aria of Sorrow", "gba"), game("Xenoblade Chronicles X", "wiiu"), game("Monster Hunter Freedom Unite", "psp"),
          game("Metroid Dread", "switch"), game("Chrono Trigger", "snes"), game("Mario Kart DS", "nds"),
          game("Spyro the Dragon", "ps1")]
GAMECUBE = [game(t, "gc", h) for t, h in [
    ("Metroid Prime", "31h 05m"), ("The Wind Waker", "38h 12m"), ("Super Mario Sunshine", "19h 30m"),
    ("Pikmin 2", "17h 02m"), ("Thousand-Year Door", "15h 48m"), ("Resident Evil 4", "14h 10m"),
    ("Melee", "12h 55m"), ("Double Dash!!", "9h 41m"), ("F-Zero GX", "8h 20m"), ("Luigi's Mansion", "6h 05m"),
    ("Tales of Symphonia", "5h 33m"), ("Animal Crossing", "4h 12m"), ("Kirby Air Ride", "2h 40m"),
    ("Path of Radiance", "1h 58m"), ("Viewtiful Joe", "0h 45m"), ("Star Fox Adventures", "0h 20m")]]


def tw(s, size, weight=400):
    """Approximate rendered text width."""
    return len(s) * size * (0.6 if weight >= 600 else 0.54)


class Svg:
    def __init__(self, name, title, w=W, h=H):
        self.name, self.title, self.defs, self.body, self.n = name, title, [], [], 0
        self.w, self.h = w, h

    def add(self, *parts):
        self.body.extend(parts)

    def _id(self, prefix):
        self.n += 1
        return f"{prefix}{self.n}"

    def lin(self, stops, x1=0, y1=0, x2=0, y2=1):
        gid = self._id("lg")
        s = "".join(f'<stop offset="{st[0]}" stop-color="{st[1]}" stop-opacity="{st[2] if len(st) > 2 else 1}"/>'
                    for st in stops)
        self.defs.append(f'<linearGradient id="{gid}" x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}">{s}</linearGradient>')
        return f"url(#{gid})"

    def rad(self, stops, cx=0.5, cy=0.5, r=0.5):
        gid = self._id("rg")
        s = "".join(f'<stop offset="{st[0]}" stop-color="{st[1]}" stop-opacity="{st[2] if len(st) > 2 else 1}"/>'
                    for st in stops)
        self.defs.append(f'<radialGradient id="{gid}" cx="{cx}" cy="{cy}" r="{r}">{s}</radialGradient>')
        return f"url(#{gid})"

    def save(self):
        OUT.mkdir(parents=True, exist_ok=True)
        w, h = self.w, self.h
        svg = (f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">'
               f'<title>{escape(self.title)}</title><defs>{"".join(self.defs)}</defs>{"".join(self.body)}</svg>')
        (OUT / f"{self.name}.svg").write_text(svg)


def rect(x, y, w, h, fill, rx=0, stroke=None, sw=0, opacity=1, dash=None):
    s = f' stroke="{stroke}" stroke-width="{sw}"' if stroke else ""
    d = f' stroke-dasharray="{dash}"' if dash else ""
    return (f'<rect x="{x:.1f}" y="{y:.1f}" width="{w:.1f}" height="{h:.1f}" rx="{rx}" fill="{fill}"'
            f' fill-opacity="{opacity}"{s}{d}/>')


def circle(cx, cy, r, fill, opacity=1, stroke=None, sw=0):
    s = f' stroke="{stroke}" stroke-width="{sw}"' if stroke else ""
    return f'<circle cx="{cx:.1f}" cy="{cy:.1f}" r="{r:.1f}" fill="{fill}" fill-opacity="{opacity}"{s}/>'


def text(x, y, s, size=16, weight=400, fill=None, anchor="start", opacity=1, ls=0, font=FONT):
    return (f'<text x="{x:.1f}" y="{y:.1f}" font-family="{font}" font-size="{size}" font-weight="{weight}"'
            f' fill="{fill or C["text"]}" fill-opacity="{opacity}" text-anchor="{anchor}"'
            f' letter-spacing="{ls}">{escape(s)}</text>')


# ---------- shared components ----------

def background(sv, glow=C["accent"], glow_opacity=0.18):
    sv.add(rect(0, 0, W, H, C["bg"]))
    sv.add(rect(0, 0, W, 520, sv.rad([(0, glow, glow_opacity), (1, glow, 0)], cx=0.5, cy=0, r=0.9)))


def glyph(sv, x, cy, label):
    """Draws a controller/keyboard prompt with its left edge at x; returns its width."""
    if label in "ABXY" and len(label) == 1:
        sv.add(circle(x + 16, cy, 16, C["surface2"], stroke=C["text"], sw=2),
               text(x + 16, cy + 6, label, 16, 700, anchor="middle"))
        return 32
    w = max(40, tw(label, 14, 700) + 18)
    sv.add(rect(x, cy - 14, w, 28, C["surface2"], rx=8, stroke=C["text"], sw=2),
           text(x + w / 2, cy + 5, label, 14, 700, anchor="middle"))
    return w


def hintbar(sv, items):
    """items: list of (glyph labels, description), drawn right-aligned."""
    sv.add(rect(0, 1010, W, 70, C["bg"], opacity=0.92), rect(0, 1010, W, 1, C["line"]))
    x = W - M
    for glyphs, desc in reversed(items):
        x -= tw(desc, 18, 500)
        sv.add(text(x, 1052, desc, 18, 500, fill=C["muted"]))
        x -= 12
        for g in reversed(glyphs):
            width = 32 if (len(g) == 1 and g in "ABXY") else max(40, tw(g, 14, 700) + 18)
            x -= width
            glyph(sv, x, 1046, g)
            x -= 6
        x -= 34


def logo(sv, x=M, y=72):
    sv.add(f'<text x="{x}" y="{y}" font-family="{FONT}" font-size="30" font-weight="800" fill="{C["text"]}"'
           f' letter-spacing="1">GAME<tspan fill="{C["accent"]}">37</tspan></text>')


def gear(sv, cx, cy, r, fill, hole):
    """A Settings gear of outer radius r: eight teeth round a ring, its hole filled with `hole`."""
    pts = []
    for k in range(8):
        a = 2 * math.pi * k / 8
        for da, rr in [(-0.27, r * 0.74), (-0.16, r), (0.16, r), (0.27, r * 0.74)]:
            pts.append(f"{cx + rr * math.cos(a + da):.1f},{cy + rr * math.sin(a + da):.1f}")
    sv.add(f'<polygon points="{" ".join(pts)}" fill="{fill}"/>', circle(cx, cy, r * 0.36, hole))


def topbar(sv, active=None, crumb=None):
    logo(sv)
    if crumb:
        sv.add(text(M + 150, 70, crumb, 20, 500, fill=C["muted"]))
    if active:
        cx = W / 2
        glyph(sv, cx - 236, 62, "L1")
        glyph(sv, cx + 196, 62, "R1")
        for label, x in [("Home", cx - 82), ("Consoles", cx + 82)]:
            on = label.lower() == active
            sv.add(text(x, 70, label, 24, 700 if on else 500, fill=C["text"] if on else C["muted"], anchor="middle"))
            if on:
                sv.add(rect(x - tw(label, 24, 700) / 2, 86, tw(label, 24, 700), 4, C["accent"], rx=2))
    # search pill, Settings gear and clock
    sx = W - M - 504
    sv.add(rect(sx, 44, 330, 48, C["surface"], rx=24, stroke=C["line"], sw=1),
           circle(sx + 30, 66, 8, "none", stroke=C["muted"], sw=2.5),
           f'<line x1="{sx + 36}" y1="{72}" x2="{sx + 42}" y2="{78}" stroke="{C["muted"]}" stroke-width="2.5"'
           f' stroke-linecap="round"/>',
           text(sx + 52, 74, "Search games", 18, 400, fill=C["muted"]))
    l3_x = sx + 330 - 14 - 40
    glyph(sv, l3_x, 68, "L3")
    glyph(sv, l3_x - 8 - (tw("Ctrl F", 14, 700) + 18), 68, "Ctrl F")
    gx = W - M - 134
    sv.add(circle(gx, 68, 24, C["surface"], stroke=C["line"], sw=1))
    gear(sv, gx, 68, 13, C["muted"], C["surface"])
    sv.add(text(W - M, 76, "21:42", 24, 600, anchor="end"))


def cover(sv, x, y, w, h, g, focused=False, show_sys=True):
    c1, c2 = PALETTE[g["pal"]]
    if focused:
        sv.add(rect(x - 10, y - 10, w + 20, h + 20, "none", rx=18, stroke=C["accent"], sw=10, opacity=0.4),
               rect(x - 5, y - 5, w + 10, h + 10, "none", rx=15, stroke="#FFFFFF", sw=4))
    sv.add(rect(x, y, w, h, sv.lin([(0, c1), (1, c2)], 0, 0, 0.5, 1), rx=10))
    sv.add(circle(x + w * 0.68, y + h * 0.3, w * 0.26, "#FFFFFF", 0.12),
           circle(x + w * 0.68, y + h * 0.3, w * 0.13, "#FFFFFF", 0.16))
    sv.add(f'<polygon points="{x},{y + h * .62:.1f} {x + w},{y + h * .5:.1f} {x + w},{y + h - 10:.1f} '
           f'{x + w - 10},{y + h:.1f} {x + 10},{y + h:.1f} {x},{y + h - 10:.1f}" fill="#000000" fill-opacity=".3"/>')
    fs = round(w * 0.105)
    lines = textwrap.wrap(g["title"], max(6, int((w - 24) / (fs * 0.58))))[:3]
    for i, line in enumerate(lines):
        sv.add(text(x + 12, y + h - 16 - (len(lines) - 1 - i) * fs * 1.12, line, fs, 800))
    if show_sys:
        label = SHORT[g["sys"]]
        sv.add(rect(x + 10, y + 10, len(label) * 8.7 + 18, 22, "#000000", rx=11, opacity=0.5),
               text(x + 19, y + 25, label, 11, 700, ls=1))


def see_all(sv, x, y, w, h, count):
    sv.add(rect(x, y, w, h, C["surface"], rx=10, stroke=C["line"], sw=1),
           circle(x + w / 2, y + h / 2 - 24, 26, C["surface2"]),
           f'<path d="M{x + w / 2 - 6},{y + h / 2 - 36} l12,12 l-12,12" fill="none" stroke="{C["text"]}"'
           f' stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/>',
           text(x + w / 2, y + h / 2 + 30, "See all", 18, 600, anchor="middle"),
           text(x + w / 2, y + h / 2 + 54, f"{count} games", 15, 400, fill=C["muted"], anchor="middle"))


def scene(sv, x, y, w, h, night=False):
    """A generic 2D platformer frame, standing in for real game output."""
    sky = [(0, "#1B1446"), (1, "#5B3C8F")] if night else [(0, "#5DA9FF"), (1, "#BDE3FF")]
    sv.add(rect(x, y, w, h, sv.lin(sky)))
    sv.add(circle(x + w * 0.82, y + h * 0.2, h * 0.08, "#FFF4C2" if night else "#FFE066"))
    for cx, cy, s in [(0.18, 0.18, 1), (0.5, 0.12, 0.8), (0.7, 0.3, 0.6)]:
        sv.add(f'<ellipse cx="{x + w * cx:.1f}" cy="{y + h * cy:.1f}" rx="{w * .07 * s:.1f}" ry="{h * .035 * s:.1f}"'
               f' fill="#FFFFFF" fill-opacity="{.25 if night else .9}"/>')
    hill = "#2E6B4F" if night else "#4CAF50"
    for cx, rx in [(0.2, 0.18), (0.62, 0.25)]:
        sv.add(f'<ellipse cx="{x + w * cx:.1f}" cy="{y + h * .8:.1f}" rx="{w * rx:.1f}" ry="{h * .22:.1f}"'
               f' fill="{hill}"/>'.replace(f'ry="{h * .22:.1f}"', f'ry="{h * .19:.1f}"'))
    gy = y + h * 0.8
    sv.add(rect(x, gy, w, h * 0.2, "#8B5A2B"), rect(x, gy, w, h * 0.035, "#5CC45C" if not night else "#3E8E5E"))
    b = h * 0.075
    for i in range(4):
        bx = x + w * 0.42 + i * b * 1.05
        sv.add(rect(bx, y + h * 0.48, b, b, "#F2B035", rx=2, stroke="#9C5B12", sw=max(1, b * .08)))
    for i in range(3):
        sv.add(circle(x + w * 0.28 + i * b * 0.9, y + h * 0.6, b * 0.22, "#FFD84A"))
    px, py = x + w * 0.2, gy - b * 1.3
    sv.add(rect(px, py, b, b * 1.3, "#E04848", rx=b * 0.25), circle(px + b * 0.68, py + b * 0.4, b * 0.12, "#FFFFFF"))


def dim(sv, opacity=0.6, x=0, w=W):
    sv.add(rect(x, 0, w, H, "#000000", opacity=opacity))


# ---------- screens ----------

CW, CH, GAP = 176, 264, 28  # shelf/grid cover size


def home(sv, focus=0):
    background(sv)
    topbar(sv, active="home")
    rows = [("Favorites", "5 of 5", FAVS, None, 152), ("Continue playing", None, CONTINUE, 23, 590),
            ("Recently added", None, RECENT, 20, 968)]
    titles = []
    for title, suffix, games, total, ty in rows:
        titles.append(text(M, ty, title, 28, 700))
        if suffix:
            titles.append(text(M + tw(title, 28, 700) + 18, ty, suffix, 20, 500, fill=C["muted"]))
        y = ty + 44
        for i, g in enumerate(games):
            focused = title == "Favorites" and i == focus
            x = M + i * (CW + GAP)
            if focused:
                cover(sv, x - 8, y - 12, CW + 16, CH + 24, g, focused=True)
                sv.add(text(x - 8, y + CH + 44, g["title"], 22, 700),
                       text(x - 8, y + CH + 72, f"{SYS[g['sys']][1]} · {g['hours']} played", 17, 400,
                            fill=C["muted"]))
            else:
                cover(sv, x, y, CW, CH, g)
        if total:
            see_all(sv, M + len(games) * (CW + GAP), y, 150, CH, total)
    sv.add(rect(0, 930, W, 80, sv.lin([(0, C["bg"], 0), (1, C["bg"], 1)])))
    sv.add(*titles)


def screen_home():
    sv = Svg("01-home", "Home")
    home(sv)
    hintbar(sv, [(["A"], "Details"), (["Y"], "Options"), (["L1", "R1"], "Switch view"), (["L3"], "Search")])
    sv.save()


def screen_home_empty():
    sv = Svg("02-home-first-launch", "Home (first launch)")
    background(sv)
    topbar(sv, active="home")
    rows = [("Favorites", "Press  Y  on any game to add it to your favorites. You can have up to 5."),
            ("Continue playing", "Games you play show up here, newest first."),
            ("Recently added", "Put games in ~/Games/<system>/ and they appear here automatically."),
            ("Most played", "Fills up as you play. Ordered by playtime.")]
    for i, (title, hint) in enumerate(rows):
        ty = 160 + i * 218
        sv.add(text(M, ty, title, 28, 700))
        sv.add(rect(M, ty + 22, W - 2 * M, 140, C["surface"], rx=14, stroke=C["line"], sw=2, opacity=0.6,
                    dash="10 8"))
        for k in range(5):
            sv.add(rect(M + 28 + k * 116, ty + 44, 96, 96, C["surface2"], rx=8, opacity=0.7))
        sv.add(text(M + 640, ty + 100, hint, 22, 500, fill=C["muted"]))
    hintbar(sv, [(["L1", "R1"], "Switch view"), (["L3"], "Search")])
    sv.save()


def console_card(sv, cx, cy, size, sysid, opacity=1.0):
    _, name, meta, color = SYS[sysid]
    x, y = cx - size / 2, cy - size / 2
    g = sv.lin([(0, color), (1, "#0B0D12")], 0, 0, 0.7, 1)
    sv.add(f'<g opacity="{opacity}">')
    sv.add(rect(x, y, size, size, g, rx=size * 0.08))
    sv.add(circle(cx + size * 0.2, cy - size * 0.2, size * 0.26, "#FFFFFF", 0.07),
           circle(cx + size * 0.2, cy - size * 0.2, size * 0.14, "#FFFFFF", 0.08))
    fs = size * (0.13 if len(name) < 10 else 0.095)
    lines = textwrap.wrap(name, 12)
    for i, line in enumerate(lines):
        sv.add(text(cx, cy + size * 0.18 + (i - len(lines) + 1) * fs * 1.1, line, round(fs), 800, anchor="middle"))
    sv.add(text(cx, cy + size * 0.32, meta, round(size * 0.045), 500, anchor="middle", opacity=0.75))
    sv.add("</g>")


WARN = "#FFB020"
# The Standalone Emulator of each External System; every other System is Embedded.
EMULATORS = {"gc": "Dolphin", "wii": "Dolphin", "3ds": "Azahar", "wiiu": "Cemu", "switch": "Ryubing",
             "ps2": "PCSX2", "ps3": "RPCS3", "ps4": "shadPS4"}


def screen_carousel(focus="3ds", missing={"wiiu": "Cemu"}, games=12):
    sv = Svg("03-carousel", "Carousel")
    order = [s[0] for s in SYSTEMS]
    idx = order.index(focus)
    _, name, _, color = SYS[focus]
    sv.add(rect(0, 0, W, H, C["bg"]))
    sv.add(rect(0, 0, W, H, sv.rad([(0, color, 0.55), (1, color, 0)], cx=0.5, cy=0.42, r=0.6)))
    topbar(sv, active="consoles")
    neighbours = [(-2, 160, 210, 0.3), (-1, 470, 310, 0.55), (1, 1450, 310, 0.55), (2, 1760, 210, 0.3)]
    for off, cx, size, op in neighbours:
        sysid = order[(idx + off) % len(order)]
        console_card(sv, cx, 450, size, sysid, op * (0.6 if sysid in missing else 1))
        if sysid in missing:
            bx, by = cx + size / 2 - 8, 450 - size / 2 + 8
            sv.add(circle(bx, by, 22, WARN), text(bx, by + 9, "!", 26, 800, fill=C["bg"], anchor="middle"),
                   text(cx, 450 + size / 2 + 40, f"{missing[sysid]} not installed", 18, 600, fill=WARN, anchor="middle"))
    sv.add(rect(705, 195, 510, 510, "none", rx=46, stroke="#FFFFFF", sw=4))
    console_card(sv, 960, 450, 500, focus)
    emulator = EMULATORS.get(focus, "Game37")
    sv.add(text(960, 790, f"{games} games  ·  Plays in {emulator}", 26, 600, fill=C["muted"], anchor="middle"))
    n = len(SYSTEMS)
    for i in range(n):
        x = 960 + (i - (n - 1) / 2) * 22
        sv.add(rect(x - 12, 846, 24, 8, "#FFFFFF", rx=4) if i == idx else circle(x, 850, 4, "#FFFFFF", 0.3))
    hintbar(sv, [(["A"], "Open"), (["L1", "R1"], "Switch view"), (["L3"], "Search")])
    sv.save()


def screen_console_page():
    sv = Svg("04-console-page", "Console Page")
    color = SYS["gc"][3]
    background(sv, glow=color, glow_opacity=0.35)
    topbar(sv, active="consoles")
    sv.add(rect(M, 152, 8, 76, color, rx=4),
           text(M + 30, 202, "GameCube", 52, 800),
           text(M + 32, 236, "38 games  ·  Plays in Dolphin", 20, 500, fill=C["muted"]))
    chip_w = 330
    cx = W - M - chip_w
    sv.add(rect(cx, 168, chip_w, 52, C["surface"], rx=26, stroke=C["line"], sw=1))
    glyph(sv, cx + 14, 194, "X")
    sv.add(text(cx + 60, 201, "Sort:", 19, 500, fill=C["muted"]), text(cx + 112, 201, "Most played", 19, 700),
           text(cx + chip_w - 22, 201, "▾", 18, 700, fill=C["muted"], anchor="end"))
    cols, top, pitch = 8, 288, CH + 72
    for i, g in enumerate(GAMECUBE[:24]):
        r, c = divmod(i, cols)
        x, y = M + c * (CW + GAP), top + r * pitch
        cover(sv, x, y, CW, CH, g, focused=(i == 2), show_sys=False)
        sv.add(text(x, y + CH + 32, g["hours"], 17, 500, fill=C["muted"]))
    sv.add(rect(0, 900, W, 110, sv.lin([(0, C["bg"], 0), (1, C["bg"], 1)])))
    hintbar(sv, [(["A"], "Details"), (["B"], "Back"), (["Y"], "Options"), (["X"], "Sort")])
    sv.save()


WIIU = [game(t, "wiiu") for t in [
    "Xenoblade Chronicles X", "Breath of the Wild", "Mario Kart 8", "Splatoon", "Pikmin 3", "Bayonetta 2",
    "Super Mario 3D World", "Wind Waker HD", "Tropical Freeze", "Twilight Princess HD"]]


def screen_console_missing_emulator():
    sv = Svg("09-console-page-emulator-missing", "Console Page (emulator missing)")
    color = SYS["wiiu"][3]
    background(sv, glow=color, glow_opacity=0.35)
    topbar(sv, active="consoles")
    sv.add(rect(M, 152, 8, 76, color, rx=4),
           text(M + 30, 202, "Wii U", 52, 800),
           text(M + 32, 236, f"{len(WIIU)} games  ·  Plays in Cemu", 20, 500, fill=C["muted"]))
    chip_w = 330
    cx = W - M - chip_w
    sv.add(rect(cx, 168, chip_w, 52, C["surface"], rx=26, stroke=C["line"], sw=1))
    glyph(sv, cx + 14, 194, "X")
    sv.add(text(cx + 60, 201, "Sort:", 19, 500, fill=C["muted"]), text(cx + 112, 201, "Most played", 19, 700),
           text(cx + chip_w - 22, 201, "▾", 18, 700, fill=C["muted"], anchor="end"))
    # missing-emulator notice
    by, bh = 268, 176
    sv.add(rect(M, by, W - 2 * M, bh, WARN, rx=18, opacity=0.1),
           rect(M, by, W - 2 * M, bh, "none", rx=18, stroke=WARN, sw=2, opacity=0.55),
           circle(M + 52, by + 52, 24, WARN), text(M + 52, by + 62, "!", 28, 800, fill=C["bg"], anchor="middle"),
           text(M + 100, by + 50, "Cemu isn't installed", 26, 700),
           text(M + 100, by + 82, "Wii U games need Cemu to play. You can still browse, favorite, rename and pick "
                                  "artwork. Play stays off until Cemu is installed.", 18, 400, fill=C["muted"]))
    cy = by + 106
    cmd = "flatpak install flathub info.cemu.Cemu"
    sv.add(rect(M + 100, cy, 640, 48, "#000000", rx=10, opacity=0.4),
           text(M + 120, cy + 31, "$", 18, 600, fill=C["dim"], font=MONO),
           text(M + 142, cy + 31, cmd, 18, 500, font=MONO))
    bx = M + 760
    sv.add(rect(bx - 5, cy - 5, 216, 58, "none", rx=15, stroke="#FFFFFF", sw=3),
           rect(bx, cy, 206, 48, C["surface2"], rx=10),
           rect(bx + 22, cy + 13, 14, 17, "none", rx=3, stroke=C["text"], sw=2),
           rect(bx + 27, cy + 18, 14, 17, C["surface2"], rx=3, stroke=C["text"], sw=2),
           text(bx + 54, cy + 31, "Copy command", 18, 600))
    bx2 = bx + 226
    sv.add(rect(bx2, cy, 168, 48, "none", rx=10, stroke=C["line"], sw=2),
           f'<path d="M{bx2 + 34},{cy + 16} a9,9 0 1,0 3,7" fill="none" stroke="{C["text"]}" stroke-width="2.4"'
           f' stroke-linecap="round"/>',
           f'<path d="M{bx2 + 30},{cy + 11} l5,5 l-6,3" fill="none" stroke="{C["text"]}" stroke-width="2.4"'
           f' stroke-linecap="round" stroke-linejoin="round"/>',
           text(bx2 + 52, cy + 31, "Check again", 18, 600))
    # games stay browsable
    cols, top, pitch = 8, by + bh + 40, CH + 72
    for i, g in enumerate(WIIU):
        r, c = divmod(i, cols)
        x, y = M + c * (CW + GAP), top + r * pitch
        cover(sv, x, y, CW, CH, g, show_sys=False)
        sv.add(text(x, y + CH + 32, "Never played", 17, 500, fill=C["muted"]))
    sv.add(rect(0, 900, W, 110, sv.lin([(0, C["bg"], 0), (1, C["bg"], 1)])))
    hintbar(sv, [(["A"], "Copy command"), (["B"], "Back")])
    sv.save()


def details_page(name, title, g, sysid, emulator, stats, path, favorite=False, missing=False, speedrun=False):
    sv = Svg(name, title)
    c1, c2 = PALETTE[g["pal"]]
    sv.add(rect(0, 0, W, H, sv.lin([(0, c1), (1, c2)], 0, 0, 1, 1)))
    sv.add(circle(1500, 330, 420, "#FFFFFF", 0.08), circle(1500, 330, 230, "#FFFFFF", 0.08),
           circle(1150, 900, 300, "#000000", 0.15))
    sv.add(rect(0, 0, W, H, sv.lin([(0, C["bg"], 0.97), (0.55, C["bg"], 0.82), (1, C["bg"], 0.35)], 0, 0, 1, 0)))
    sv.add(rect(0, 640, W, 440, sv.lin([(0, C["bg"], 0), (1, C["bg"], 1)])))
    logo(sv)
    sv.add(text(M + 150, 70, f"{SYS[sysid][1]}   ›   {g['title']}", 20, 500, fill=C["muted"]),
           text(W - M, 76, "21:42", 24, 600, anchor="end"))
    cover(sv, M, 176, 380, 570, g, show_sys=False)
    x = 560
    label = SHORT[sysid]
    pill_w = len(label) * 10.5 + 48
    sv.add(rect(x, 196, pill_w, 32, SYS[sysid][3], rx=16),
           text(x + pill_w / 2, 218, label, 14, 700, anchor="middle", ls=1.5))
    if favorite:
        sv.add(text(x + pill_w + 20, 220, "★  Favorite", 18, 600, fill=C["star"]))
    sv.add(text(x - 4, 322, g["title"], 84, 800), text(x, 368, f"Opens in {emulator}", 22, 500, fill=C["muted"]))
    for i, (k, v) in enumerate(stats):
        sx = x + i * 270
        sv.add(rect(sx, 410, 250, 104, C["surface"], rx=14, opacity=0.75),
               text(sx + 22, 448, k, 14, 700, fill=C["muted"], ls=1.5), text(sx + 22, 490, v, 30, 700))
    sv.add(text(x, 560, path, 18, 400, fill=C["muted"], font=MONO))
    # actions
    if missing:
        sv.add(rect(x, 604, 270, 76, C["surface2"], rx=38, stroke=C["line"], sw=2, opacity=0.6),
               circle(x + 52, 642, 17, "none", stroke=C["dim"], sw=2),
               text(x + 52, 648, "A", 17, 800, fill=C["dim"], anchor="middle"),
               text(x + 86, 652, "Play", 28, 800, fill=C["dim"]))
        sv.add(rect(x + 285, 599, 246, 86, "none", rx=43, stroke="#FFFFFF", sw=3))
    else:
        sv.add(rect(x, 604, 270, 76, "#FFFFFF", rx=38))
        sv.add(circle(x + 52, 642, 17, C["bg"]), text(x + 52, 648, "A", 17, 800, fill="#FFFFFF", anchor="middle"),
               text(x + 86, 652, "Play", 28, 800, fill=C["bg"]))
    ox = x + 290
    if speedrun:
        # M7: the Speedrun action sits between Play and Options (FR-RUN-1).
        sv.add(rect(ox, 604, 260, 76, C["surface"], rx=38, stroke=C["line"], sw=2, opacity=0.85))
        glyph(sv, ox + 32, 642, "X")
        sv.add(text(ox + 78, 651, "Speedrun", 24, 700))
        ox += 280
    sv.add(rect(ox, 604, 236, 76, C["surface"], rx=38, stroke=C["line"], sw=2, opacity=0.85))
    glyph(sv, ox + 32, 642, "Y")
    sv.add(text(ox + 78, 651, "Options", 24, 700))
    if missing:
        sv.add(circle(x + 14, 731, 14, WARN), text(x + 14, 738, "!", 18, 800, fill=C["bg"], anchor="middle"),
               text(x + 40, 738, f"{emulator} isn't installed, so Play is off. The install command is on the "
                                 f"{SYS[sysid][1]} page.", 19, 500, fill=WARN))
        hintbar(sv, [(["B"], "Back"), (["Y"], "Options")])
    elif speedrun:
        hintbar(sv, [(["A"], "Play"), (["B"], "Back"), (["X"], "Speedrun"), (["Y"], "Options")])
    else:
        hintbar(sv, [(["A"], "Play"), (["B"], "Back"), (["Y"], "Options")])
    sv.save()


def screen_details():
    details_page("05-details", "Details Page", GAMECUBE[0], "gc", "Dolphin",
                 [("PLAYTIME", "31h 05m"), ("LAST PLAYED", "Yesterday"), ("ADDED", "3 Oct 2026"), ("SIZE", "1.36 GB")],
                 "~/Games/gc/Metroid Prime (USA).rvz", favorite=True)


def screen_details_missing_emulator():
    details_page("10-details-emulator-missing", "Details Page (emulator missing)", WIIU[0], "wiiu", "Cemu",
                 [("PLAYTIME", "—"), ("LAST PLAYED", "Never"), ("ADDED", "17 Oct 2026"), ("SIZE", "22.4 GB")],
                 "~/Games/wiiu/Xenoblade Chronicles X/", missing=True)


def screen_details_speedrun():
    details_page("16-details-speedrun", "Details Page (Speedrun)", SMW, "snes", "Game37 · bsnes core",
                 [("PLAYTIME", SMW["hours"]), ("LAST PLAYED", "Today"), ("SAVE STATES", "2 of 3"), ("SIZE", "512 KB")],
                 "~/Games/snes/Super Mario World (USA).sfc", favorite=True, speedrun=True)


def screen_options():
    sv = Svg("06-options-menu", "Options Menu")
    home(sv)
    dim(sv, 0.62)
    sx, sw = W - 600, 600
    sv.add(rect(sx, 0, sw, H, C["surface"]), rect(sx, 0, 1, H, C["line"]))
    g = FAVS[0]
    cover(sv, sx + 48, 64, 96, 144, g, show_sys=False)
    sv.add(text(sx + 168, 118, g["title"], 28, 800), text(sx + 168, 152, "SNES  ·  12h 40m played", 18, 500,
                                                            fill=C["muted"]))
    items = [("★", "Remove from favorites", None), ("↺", "Load save state", "2 of 3"),
             ("▣", "Choose artwork", None), ("✎", "Rename", None), ("⊘", "Hide game", None),
             ("▤", "Show file in file manager", None)]
    for i, (icon, label, extra) in enumerate(items):
        y = 252 + i * 86
        if i == 1:
            sv.add(rect(sx + 24, y, sw - 48, 72, C["surface2"], rx=14), rect(sx + 24, y + 16, 5, 40, C["accent"], rx=2))
        sv.add(text(sx + 62, y + 46, icon, 24, 600, fill=C["star"] if icon == "★" else C["muted"], anchor="middle"),
               text(sx + 100, y + 45, label, 22, 600 if i == 1 else 500))
        if extra:
            sv.add(text(sx + sw - 76, y + 45, extra, 18, 500, fill=C["muted"], anchor="end"))
        if label in ("Load save state", "Choose artwork"):
            sv.add(text(sx + sw - 48, y + 45, "›", 26, 600, fill=C["muted"], anchor="end"))
    hintbar(sv, [(["A"], "Choose"), (["B"], "Close")])
    sv.save()


def screen_pause():
    sv = Svg("07-pause-menu", "Pause Menu")
    sv.add(rect(0, 0, W, H, "#000000"))
    scene(sv, 240, 0, 1440, 1080)
    dim(sv, 0.6)
    sv.add(rect(0, 0, 600, H, C["surface"], opacity=0.94), rect(600, 0, 1, H, C["line"]))
    sv.add(text(M, 170, "PAUSED", 15, 700, fill=C["accent"], ls=3), text(M, 228, "Super Mario World", 40, 800),
           text(M, 266, "SNES  ·  12h 40m played", 19, 500, fill=C["muted"]))
    for i, label in enumerate(["Resume", "Save state", "Load state", "Quit to library"]):
        y = 320 + i * 84
        if i == 1:
            sv.add(rect(M - 24, y, 456, 70, C["surface2"], rx=14), rect(M - 24, y + 15, 5, 40, C["accent"], rx=2))
        sv.add(text(M, y + 44, label, 24, 700 if i == 1 else 500))
        if i in (1, 2):
            sv.add(text(M + 408, y + 44, "›", 26, 600, fill=C["muted"], anchor="end"))
    sv.add(text(M, 960, "Opens with Guide, Select + Start, or Esc", 16, 500, fill=C["dim"]))
    sv.add(text(680, 330, "Save to slot", 30, 800))
    slots = [("Slot 1", "Today, 21:14"), ("Slot 2", "Yesterday, 18:02"), ("Slot 3", "Empty")]
    for i, (name, when) in enumerate(slots):
        x, y = 680 + i * 392, 366
        if when == "Empty":
            sv.add(rect(x - 6, y - 6, 372, 282, "none", rx=16, stroke="#FFFFFF", sw=4),
                   rect(x, y, 360, 270, C["surface"], rx=12, stroke=C["line"], sw=2, dash="10 8"),
                   text(x + 180, y + 146, "+  Empty slot", 22, 600, fill=C["muted"], anchor="middle"))
        else:
            scene(sv, x, y, 360, 270)
            sv.add(rect(x, y, 360, 270, "none", rx=12, stroke=C["line"], sw=2))
        sv.add(text(x, y + 312, name, 22, 700), text(x, y + 340, when, 18, 500, fill=C["muted"]))
    hintbar(sv, [(["A"], "Save here"), (["B"], "Back")])
    sv.save()


def screen_ds():
    sv = Svg("08-ds-layout", "DS Layout")
    sv.add(rect(0, 0, W, H, "#06070A"))
    bx, by, bw, bh = 72, 72, 1248, 936
    scene(sv, bx, by, bw, bh, night=True)
    sx, sy, sw, sh = 1376, 640, 464, 348
    sv.add(rect(sx, sy, sw, sh, "#E9EDF5"))
    sv.add(rect(sx, sy, sw, 44, "#3B4A6B"), text(sx + 18, sy + 29, "MAP", 16, 700, ls=2))
    for r in range(3):
        for c in range(4):
            sv.add(rect(sx + 22 + c * 108, sy + 66 + r * 90, 96, 76, "#C7D0E2", rx=10))
    sv.add(rect(sx + 130, sy + 156, 96, 76, "#8FA6D6", rx=10))
    cx, cy = sx + 196, sy + 214
    sv.add(circle(cx, cy, 20, "none", stroke="#0B0D12", sw=3), circle(cx, cy, 4, "#0B0D12"),
           f'<path d="M{cx - 32},{cy} h18 M{cx + 14},{cy} h18 M{cx},{cy - 32} v18 M{cx},{cy + 14} v18"'
           f' stroke="#0B0D12" stroke-width="3" stroke-linecap="round"/>')
    sv.add(text(bx, by + bh + 34, "Main screen", 16, 600, fill=C["dim"]),
           text(sx, sy + sh + 34, "Touch screen", 16, 600, fill=C["dim"]))
    lx = sx
    sv.add(text(lx, 140, "Mario Kart DS", 34, 800), text(lx, 178, "Nintendo DS  ·  Large + small layout", 18, 500,
                                                         fill=C["muted"]))
    rows = [(["R3"], "Swap large and small screen"), (["Mouse"], "Click to touch"),
            (["Right stick"], "Move touch cursor"), (["R2"], "Tap with cursor (proposed)")]
    for i, (gs, desc) in enumerate(rows):
        y = 250 + i * 64
        wdt = glyph(sv, lx, y, gs[0])
        sv.add(text(lx + wdt + 16, y + 7, desc, 19, 500, fill=C["muted"]))
    sv.add(text(lx, 560, "Pause menu: Guide, Select + Start, or Esc", 16, 500, fill=C["dim"]))
    sv.save()


# ---------- speedrun timer (M7) ----------

def cs(t):
    """'m:ss.cc' or 'h:mm:ss.cc' to hundredths of a second."""
    *hm, sec = t.split(":")
    mins = sum(int(v) * 60 ** (len(hm) - 1 - i) for i, v in enumerate(hm))
    return mins * 6000 + round(float(sec) * 100)


def fmt(c):
    h, rest = divmod(c, 360000)
    m, rest = divmod(rest, 6000)
    s, cc = divmod(rest, 100)
    return f"{h}:{m:02}:{s:02}.{cc:02}" if h else f"{m}:{s:02}.{cc:02}"


def fmt_delta(d):
    m, rest = divmod(abs(d), 6000)
    s, cc = divmod(rest, 100)
    return ("−" if d < 0 else "+") + (f"{m}:{s:02}.{cc:02}" if m else f"{s}.{cc:02}")


def cumulative(segs):
    out, total = [], 0
    for seg in segs:
        total += seg
        out.append(total)
    return out


SMW = FAVS[0]
SPLITS = ["Yoshi's Island", "Donut Plains", "Vanilla Dome", "Twin Bridges", "Forest of Illusion",
          "Chocolate Island", "Valley of Bowser", "Bowser"]
PB_SEGS = [cs(t) for t in ["2:48.12", "3:17.21", "3:36.47", "2:40.25", "2:48.59", "3:36.65", "3:15.62", "2:28.56"]]
BEST_SEGS = [cs(t) for t in ["2:45.90", "3:12.48", "3:31.07", "2:38.66", "2:44.13", "3:30.82", "3:11.40", "2:26.95"]]
RUN_SEGS = [cs(t) for t in ["2:50.04", "3:08.77", "3:39.10", "2:47.30"]]  # a Run partway, now on Split 5
CATEGORIES = [("11 Exit", "24:31.47", 143), ("96 Exit", None, 3), ("0 Exit", "1:02.84", 61)]
HISTORY = [("20 Oct 2026", "24:58.30"), ("19 Oct 2026", "24:31.47"), ("19 Oct 2026", "25:12.08"),
           ("17 Oct 2026", "24:47.91"), ("14 Oct 2026", "25:40.66"), ("12 Oct 2026", "25:03.15"),
           ("9 Oct 2026", "26:11.42"), ("5 Oct 2026", "26:58.70"), ("3 Oct 2026", "27:35.19")]


def speedrun_header(sv, crumb, eyebrow, title, sub):
    background(sv, glow=SYS["snes"][3], glow_opacity=0.35)
    logo(sv)
    sv.add(text(M + 150, 70, crumb, 20, 500, fill=C["muted"]), text(W - M, 76, "21:42", 24, 600, anchor="end"))
    cover(sv, M, 136, 92, 138, SMW, show_sys=False)
    x = M + 124
    sv.add(text(x, 168, eyebrow, 15, 700, fill=C["accent"], ls=3), text(x - 2, 224, title, 48, 800),
           text(x, 260, sub, 20, 500, fill=C["muted"]))


def action_pill(sv, x, y, label, pad=None, primary=False, icon=None):
    """A Details Page style action button, 64 high; returns its width."""
    h, cy = 64, y + 32
    lead = 46 if pad else (tw(icon, 22, 600) + 12 if icon else 0)
    w = 28 + lead + tw(label, 22) + 28
    if primary:
        sv.add(rect(x, y, w, h, "#FFFFFF", rx=32), circle(x + 44, cy, 16, C["bg"]),
               text(x + 44, cy + 6, pad, 16, 800, fill="#FFFFFF", anchor="middle"),
               text(x + 28 + lead, cy + 8, label, 22, 800, fill=C["bg"]))
        return w
    sv.add(rect(x, y, w, h, C["surface"], rx=32, stroke=C["line"], sw=2, opacity=0.85))
    if pad:
        glyph(sv, x + 28, cy, pad)
    elif icon:
        sv.add(text(x + 28, cy + 8, icon, 22, 600, fill=C["muted"]))
    sv.add(text(x + 28 + lead, cy + 8, label, 22, 700))
    return w


def screen_speedrun_page():
    sv = Svg("11-speedrun-page", "Speedrun Page")
    speedrun_header(sv, f"SNES   ›   {SMW['title']}   ›   Speedrun", "SPEEDRUN", SMW["title"],
                    f"SNES  ·  {len(CATEGORIES)} Categories")
    x = M
    for label, pad, primary, icon in [("Start run", "A", True, None), ("Edit splits", "X", False, None),
                                      ("New category", "Y", False, None), ("Rename", None, False, None),
                                      ("Delete", None, False, None)]:
        x += action_pill(sv, x, 304, label, pad, primary, icon) + 16
    # Categories
    sv.add(text(M, 428, "CATEGORIES", 14, 700, fill=C["muted"], ls=1.5))
    for i, (name, pb, attempts) in enumerate(CATEGORIES):
        y = 448 + i * 108
        if i == 0:
            sv.add(rect(M - 24, y, 544, 96, C["surface2"], rx=14), rect(M - 24, y + 22, 5, 52, C["accent"], rx=2),
                   text(M + 496, y + 58, "›", 26, 600, fill=C["muted"], anchor="end"))
        sub = f"Personal Best {pb}" if pb else "No Personal Best yet"
        sv.add(text(M, y + 44, name, 26, 700 if i == 0 else 600),
               text(M, y + 76, f"{sub}  ·  {attempts} attempts", 18, 500, fill=C["muted"]))
    # the focused Category
    px, py, pw, ph = M + 560, 400, W - 2 * M - 560, 590
    sv.add(rect(px, py, pw, ph, C["surface"], rx=18, opacity=0.75))
    sv.add(text(px + 32, py + 56, "11 Exit", 30, 800),
           text(px + pw - 32, py + 56, f"143 attempts  ·  {len(HISTORY)} finished Runs", 18, 500, fill=C["muted"],
                anchor="end"))
    c_pb, c_best, r = px + 520, px + 712, px + 712
    sv.add(text(px + 32, py + 104, "SPLIT", 13, 700, fill=C["muted"], ls=1.5),
           text(c_pb, py + 104, "PERSONAL BEST", 13, 700, fill=C["muted"], ls=1.5, anchor="end"),
           text(c_best, py + 104, "BEST SEGMENT", 13, 700, fill=C["muted"], ls=1.5, anchor="end"))
    for i, (name, t) in enumerate(zip(SPLITS, cumulative(PB_SEGS))):
        y = py + 118 + i * 46
        sv.add(rect(px + 32, y, r - px - 32, 1, C["line"]),
               text(px + 32, y + 31, str(i + 1), 18, 500, fill=C["dim"]), text(px + 66, y + 31, name, 20, 500),
               text(c_pb, y + 31, fmt(t), 20, 600, anchor="end"),
               text(c_best, y + 31, fmt(BEST_SEGS[i]), 20, 500, fill=C["muted"], anchor="end"))
    y = py + 118 + 8 * 46
    sv.add(rect(px + 32, y, r - px - 32, 2, C["line"]),
           text(px + 32, y + 40, "Sum of Best", 20, 700),
           text(c_best, y + 40, fmt(sum(BEST_SEGS)), 20, 700, anchor="end"))
    # Run history
    hx = px + 768
    hr = px + pw - 32
    sv.add(rect(hx - 28, py + 84, 1, ph - 116, C["line"]),
           text(hx, py + 104, "RUN HISTORY", 13, 700, fill=C["muted"], ls=1.5))
    for i, (date, t) in enumerate(HISTORY):
        y = py + 118 + i * 46
        is_pb = t == CATEGORIES[0][1]
        if is_pb:
            sv.add(rect(hx - 12, y + 2, hr - hx + 24, 42, C["accent"], rx=10, opacity=0.16))
        sv.add(text(hx, y + 31, date, 18, 500, fill=C["text"] if is_pb else C["muted"]),
               text(hr, y + 31, t, 20, 700 if is_pb else 600, anchor="end"))
        if is_pb:
            sv.add(rect(hx + 124, y + 12, 112, 22, C["accent"], rx=11),
                   text(hx + 180, y + 28, "PERSONAL BEST", 11, 800, anchor="middle", ls=1))
    hintbar(sv, [(["A"], "Start run"), (["B"], "Back"), (["X"], "Edit splits"), (["Y"], "New category")])
    sv.save()


def screen_splits_editor():
    sv = Svg("12-splits-editor", "Splits editor")
    speedrun_header(sv, f"SNES   ›   {SMW['title']}   ›   Speedrun   ›   11 Exit", "EDIT SPLITS", "11 Exit",
                    f"{SMW['title']}  ·  {len(SPLITS)} Splits")
    lx, lw, focus = M, 1040, 2
    c_pb, c_best = lx + 820, lx + 1000
    sv.add(text(lx + 70, 336, "SPLIT", 13, 700, fill=C["muted"], ls=1.5),
           text(c_pb, 336, "PERSONAL BEST", 13, 700, fill=C["muted"], ls=1.5, anchor="end"),
           text(c_best, 336, "BEST SEGMENT", 13, 700, fill=C["muted"], ls=1.5, anchor="end"))
    for i, (name, t) in enumerate(zip(SPLITS, cumulative(PB_SEGS))):
        y = 352 + i * 70
        if i == focus:
            sv.add(rect(lx - 24, y, lw + 24, 60, C["surface2"], rx=14), rect(lx - 24, y + 12, 5, 36, C["accent"], rx=2),
                   text(lx + 94 + tw(name, 24), y + 37, "▲", 16, 600, fill=C["muted"], anchor="middle"),
                   text(lx + 122 + tw(name, 24), y + 37, "▼", 16, 600, fill=C["muted"], anchor="middle"))
        else:
            sv.add(rect(lx - 24, y, lw + 24, 60, C["surface"], rx=14, opacity=0.6))
        sv.add(text(lx + 20, y + 38, str(i + 1), 20, 600, fill=C["dim"], anchor="middle"),
               text(lx + 70, y + 39, name, 24, 700 if i == focus else 500),
               text(c_pb, y + 38, fmt(t), 20, 500, fill=C["muted"], anchor="end"),
               text(c_best, y + 38, fmt(BEST_SEGS[i]), 20, 500, fill=C["muted"], anchor="end"))
    y = 352 + len(SPLITS) * 70
    sv.add(rect(lx - 24, y, lw + 24, 60, "none", rx=14, stroke=C["line"], sw=2, dash="10 8"),
           text(lx + 70, y + 39, "+  Add split", 22, 600, fill=C["muted"]))
    # notes beside the list
    nx = lx + lw + 64
    sv.add(text(nx, 336, "GOOD TO KNOW", 13, 700, fill=C["muted"], ls=1.5))
    notes = [("Finished Runs are kept.", "Editing Splits never discards a finished Run."),
             ("New Splits start empty.", "A new Split has no times until a Run records one."),
             ("Shared with LiveSplit.", "This Category is a LiveSplit file in")]
    for i, (head, body) in enumerate(notes):
        y = 352 + i * 132
        sv.add(rect(nx, y, W - M - nx, 116, C["surface"], rx=14, opacity=0.75),
               text(nx + 24, y + 44, head, 20, 700), text(nx + 24, y + 76, body, 17, 500, fill=C["muted"]))
    sv.add(text(nx + 24, 352 + 2 * 132 + 102, "~/Games/splits/snes/Super Mario World (USA)/", 15, 400,
                fill=C["muted"], font=MONO))
    hintbar(sv, [(["A"], "Rename"), (["B"], "Back"), (["X"], "Add split"), (["Y"], "Remove"),
                 (["L1", "R1"], "Move up / down")])
    sv.save()


def splits_panel(sv, x, y, w, row_h, timer, controls=True):
    """The splits panel shared by the in-game view and the Timer Window, for the Run in RUN_SEGS."""
    r = x + w - 24
    sv.add(text(x + 24, y + 38, SMW["title"], 22, 800), text(x + 24, y + 66, "11 Exit", 18, 600, fill=C["muted"]),
           text(r, y + 66, "Attempt 144", 16, 500, fill=C["muted"], anchor="end"),
           rect(x + 24, y + 88, w - 48, 1, C["line"]))
    top, cur = y + 96, len(RUN_SEGS)
    pb, run = cumulative(PB_SEGS), cumulative(RUN_SEGS)
    for i, name in enumerate(SPLITS):
        ty = top + i * row_h
        base = ty + row_h / 2 + 7
        if i == cur:
            sv.add(rect(x + 12, ty + 3, w - 24, row_h - 6, C["surface2"], rx=10),
                   rect(x + 12, ty + row_h / 2 - 12, 4, 24, C["accent"], rx=2))
        sv.add(text(x + 28, base, name, 20, 700 if i == cur else 500, fill=C["text"] if i <= cur else C["muted"]))
        if i < cur:
            d = run[i] - pb[i]
            col = C["best"] if RUN_SEGS[i] < BEST_SEGS[i] else C["ahead"] if d < 0 else C["behind"]
            sv.add(text(x + w - 150, base, fmt_delta(d), 18, 700, fill=col, anchor="end"),
                   text(r, base, fmt(run[i]), 20, 600, anchor="end"))
        else:
            sv.add(text(r, base, fmt(pb[i]), 20, 500, fill=C["muted"], anchor="end"))
    t = top + len(SPLITS) * row_h + 8
    main, hundredths = timer.rsplit(".", 1)
    sv.add(rect(x + 24, t, w - 48, 1, C["line"]),
           f'<text x="{r:.1f}" y="{t + 92:.1f}" font-family="{FONT}" font-size="80" font-weight="800"'
           f' fill="{C["text"]}" text-anchor="end">{main}<tspan font-size="46">.{hundredths}</tspan></text>')
    sob = sum(min(b, RUN_SEGS[i]) if i < cur else b for i, b in enumerate(BEST_SEGS))
    for k, (label, val) in enumerate([("Personal Best", pb[-1]), ("Sum of Best", sob)]):
        sv.add(text(x + 24, t + 138 + k * 32, label, 18, 500, fill=C["muted"]),
               text(r, t + 138 + k * 32, fmt(val), 18, 700, anchor="end"))
    sv.add(text(x + 24, t + 212, "COMPARE AGAINST", 12, 700, fill=C["muted"], ls=1.5))
    half = (w - 48) / 2
    sv.add(rect(x + 24, t + 224, w - 48, 40, C["surface2"], rx=20),
           rect(x + 28, t + 228, half - 4, 32, C["accent"], rx=16),
           text(x + 24 + half / 2, t + 250, "Personal Best", 16, 700, anchor="middle"),
           text(x + 24 + half * 1.5, t + 250, "Sum of Best", 16, 500, fill=C["muted"], anchor="middle"))
    if controls:
        gx = x + 24
        gx += glyph(sv, gx, t + 300, "Share") + 10
        sv.add(text(gx, t + 306, "or", 16, 500, fill=C["dim"]))
        gx += tw("or", 16) + 10
        gx += glyph(sv, gx, t + 300, "Numpad 1") + 14
        sv.add(text(gx, t + 306, "Split", 16, 500, fill=C["muted"]))


def run_view(sv, timer):
    """An embedded Run on a 4:3 System: picture at the left edge, splits panel in the 480 px it leaves."""
    sv.add(rect(0, 0, W, H, "#000000"))
    scene(sv, 0, 0, 1440, 1080)
    sv.add(rect(1440, 0, 480, H, C["surface"]), rect(1440, 0, 1, H, C["line"]))
    splits_panel(sv, 1440, 48, 480, 64, timer)


def screen_speedrun_embedded():
    sv = Svg("13-speedrun-embedded", "Embedded Run with splits panel")
    run_view(sv, "13:58.42")
    sv.save()


def screen_pause_run():
    sv = Svg("14-pause-menu-run", "Pause Menu during a Run")
    run_view(sv, "14:06.15")
    dim(sv, 0.6, w=1440)
    sv.add(rect(0, 0, 600, H, C["surface"], opacity=0.94), rect(600, 0, 1, H, C["line"]))
    sv.add(text(M, 150, "PAUSED", 15, 700, fill=C["accent"], ls=3), text(M, 206, SMW["title"], 40, 800),
           text(M, 244, "SNES  ·  11 Exit  ·  Attempt 144", 19, 500, fill=C["muted"]))
    groups = [[("Resume", None)],
              [("Undo split", "Numpad 8"), ("Skip split", "Numpad 2"), ("Reset", "Numpad 3"),
               ("Restart run", None), ("Hide timer", None)],
              [("Save state", None), ("Load state", "Resets the Run"), ("Quit to library", "Resets the Run")]]
    y, focus = 286, "Undo split"
    for g, items in enumerate(groups):
        if g:
            sv.add(rect(M, y + 8, 408, 1, C["line"]))
            y += 18
        for label, extra in items:
            if label == focus:
                sv.add(rect(M - 24, y, 456, 60, C["surface2"], rx=14), rect(M - 24, y + 12, 5, 36, C["accent"], rx=2))
            sv.add(text(M, y + 39, label, 24, 700 if label == focus else 500))
            arrow = label in ("Save state", "Load state")
            if extra:
                sv.add(text(M + (380 if arrow else 408), y + 38, extra, 16, 500, fill=C["dim"], anchor="end"))
            if arrow:
                sv.add(text(M + 408, y + 39, "›", 26, 600, fill=C["muted"], anchor="end"))
            y += 64
    sv.add(text(M, 934, "The timer keeps running while paused.", 16, 500, fill=C["muted"]),
           text(M, 962, "Opens with Guide, Select + Start, or Esc", 16, 500, fill=C["dim"]))
    hintbar(sv, [(["A"], "Choose"), (["B"], "Resume")])
    sv.save()


def screen_timer_window():
    ww, wh, bar = 480, 840, 40
    sv = Svg("15-timer-window", "Timer Window", ww, wh)
    sv.add(rect(0.5, 0.5, ww - 1, wh - 1, C["surface"], rx=12, stroke=C["line"], sw=1),
           rect(0.5, 0.5, ww - 1, bar, C["surface2"], rx=12), rect(0.5, bar - 12, ww - 1, 12, C["surface2"]),
           rect(0.5, bar, ww - 1, 1, C["line"]),
           text(ww / 2, 26, "Game37 Timer", 15, 600, fill=C["muted"], anchor="middle"),
           circle(ww - 22, 20, 11, C["line"]),
           f'<path d="M{ww - 26},16 l8,8 M{ww - 18},16 l-8,8" stroke="{C["text"]}" stroke-width="1.8"'
           f' stroke-linecap="round"/>')
    splits_panel(sv, 0, bar, ww, 44, "13:58.42")
    sv.add(f'<path d="M{ww - 8},{wh - 20} l-12,12 M{ww - 8},{wh - 13} l-5,5" stroke="{C["dim"]}"'
           f' stroke-width="1.8" stroke-linecap="round"/>')
    sv.save()


# ---------- Settings (M6) ----------

SECTIONS = ["Library", "Artwork", "BIOS", "Cores", "Controls", "Display", "About"]
RX = M + 448  # left edge of the rows column
RW = W - M - RX
TOP = 240  # first section and first row


def settings_page(sv, section):
    """Settings with `section` open and the focus in its rows: the section stays marked in accent."""
    background(sv)
    topbar(sv, crumb="Home")
    sv.add(text(M, 196, "Settings", 48, 800))
    for i, name in enumerate(SECTIONS):
        y = TOP + i * 76
        on = name == section
        if on:
            sv.add(rect(M - 24, y, 360, 64, C["surface"], rx=14))
        sv.add(text(M, y + 41, name, 24, 700 if on else 500, fill=C["accent"] if on else C["muted"]))
    sv.add(rect(M + 400, TOP, 1, 740, C["line"]))


def settings_row(sv, y, h, label, value=None, sub=None, focused=False, more=False, fill=None, mono=False):
    """A Settings row card; returns the x its right-aligned value ends at."""
    if focused:
        sv.add(rect(RX, y, RW, h, C["surface2"], rx=14), rect(RX, y + h / 2 - 20, 5, 40, C["accent"], rx=2))
    else:
        sv.add(rect(RX, y, RW, h, C["surface"], rx=14, opacity=0.6))
    base = y + h / 2 + (8 if sub is None else -6)
    sv.add(text(RX + 32, base, label, 22, 700 if focused else 600, fill=fill))
    if sub:
        sv.add(text(RX + 32, base + 30, sub, 16, 500, fill=C["muted"]))
    r = RX + RW - 32
    if more:
        sv.add(text(r, y + h / 2 + 9, "›", 26, 600, fill=C["muted"], anchor="end"))
        r -= 36
    if value:
        sv.add(text(r, y + h / 2 + 7, value, 18, 500, fill=C["muted"], anchor="end", font=MONO if mono else FONT))
    return r


def screen_settings_library():
    sv = Svg("17-settings-library", "Settings: Library")
    settings_page(sv, "Library")
    rows = [("Games Root location", "~/Games", True), ("Rescan library", "23 Games", False),
            ("Hidden games", "2 hidden", False)]
    for i, (label, value, mono) in enumerate(rows):
        settings_row(sv, TOP + i * 88, 76, label, value, focused=label == "Hidden games",
                     more=label == "Hidden games", mono=mono)
    settings_row(sv, TOP + 3 * 88 + 24, 92, "Reset Playtime and stats",
                 sub="Clears Playtime and last played for every Game. Favorites and saves stay.", fill=C["danger"])
    hintbar(sv, [(["A"], "Open"), (["B"], "Back")])
    sv.save()


def screen_settings_bios():
    sv = Svg("18-settings-bios", "Settings: BIOS")
    settings_page(sv, "BIOS")
    pitch, top, status_x = 41, TOP + 28, RX + 320
    sv.add(text(RX + 32, TOP + 13, "SYSTEM", 13, 700, fill=C["muted"], ls=1.5),
           text(status_x, TOP + 13, "BIOS", 13, 700, fill=C["muted"], ls=1.5))
    for i, (sysid, name, _, _) in enumerate(SYSTEMS):
        y = top + i * pitch
        focused = sysid == "ps1"
        if focused:
            sv.add(rect(RX, y, RW, pitch, C["surface2"], rx=10), rect(RX, y + pitch / 2 - 13, 5, 26, C["accent"], rx=2))
        elif i:
            sv.add(rect(RX + 16, y, RW - 32, 1, C["line"]))
        base = y + pitch / 2 + 7
        sv.add(text(RX + 32, base, name, 19, 700 if focused else 500))
        if sysid == "ps1":
            sv.add(circle(status_x + 11, base - 6, 11, WARN),
                   text(status_x + 11, base - 1, "!", 15, 800, fill=C["bg"], anchor="middle"),
                   text(status_x + 32, base, "Missing: put scph5500.bin, scph5501.bin or scph5502.bin in "
                                          "~/Games/bios/", 18, 500, fill=WARN))
        elif sysid in EMULATORS:
            sv.add(text(status_x, base, f"Handled by {EMULATORS[sysid]}", 18, 500, fill=C["muted"]))
        else:
            sv.add(text(status_x, base, "No BIOS needed", 18, 500, fill=C["muted"]))
    hintbar(sv, [(["B"], "Back")])
    sv.save()


# Each Embedded System's Core, by the System Order of its first System, with its state in the mockup.
CORES = [("Mesen", ["nes"], "Installed"), ("SameBoy", ["gb", "gbc"], "Installed"), ("bsnes", ["snes"], "Installed"),
         ("Mupen64Plus-Next", ["n64"], 45), ("mGBA", ["gba"], "Installed"),
         ("melonDS DS", ["nds"], "Not downloaded"), ("Beetle PSX HW", ["ps1"], "Failed"),
         ("PPSSPP", ["psp"], "Not downloaded")]


def screen_settings_cores():
    sv = Svg("19-settings-cores", "Settings: Cores")
    settings_page(sv, "Cores")
    installed = sum(state == "Installed" for _, _, state in CORES)
    settings_row(sv, TOP, 64, "Update all", f"Redownloads the {installed} installed Cores")
    for i, (name, systems, state) in enumerate(CORES):
        y, h = TOP + 78 + i * 78, 70
        r = settings_row(sv, y, h, name, sub=", ".join(SYS[k][1] for k in systems), focused=state == "Failed")
        base = y + h / 2 + 7
        if state == "Failed":
            sv.add(text(r, base, "Download failed · Retry", 18, 600, fill=WARN, anchor="end"))
        elif isinstance(state, int):
            bw = 240
            sv.add(rect(r - bw, y + h / 2 - 4, bw, 8, C["line"], rx=4),
                   rect(r - bw, y + h / 2 - 4, bw * state / 100, 8, C["accent"], rx=4),
                   text(r - bw - 20, base, f"Downloading {state}%", 18, 500, fill=C["text"], anchor="end"))
        else:
            sv.add(text(r, base, state, 18, 500, fill=C["muted"] if state == "Installed" else C["dim"],
                        anchor="end"))
    hintbar(sv, [(["A"], "Download"), (["B"], "Back")])
    sv.save()


if __name__ == "__main__":
    for fn in [screen_home, screen_home_empty, screen_carousel, screen_console_page, screen_details,
               screen_options, screen_pause, screen_ds, screen_console_missing_emulator,
               screen_details_missing_emulator, screen_speedrun_page, screen_splits_editor,
               screen_speedrun_embedded, screen_pause_run, screen_timer_window, screen_details_speedrun,
               screen_settings_library, screen_settings_bios,
               screen_settings_cores]:
        fn()
    print("\n".join(sorted(p.name for p in OUT.glob("*.svg"))))

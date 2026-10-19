#!/usr/bin/env python3
"""Generates the Game37 screen mockups as SVG files in design/mockups/.

Cover art is placeholder gradients: real art comes from SteamGridDB at runtime.
"""
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
    def __init__(self, name, title):
        self.name, self.title, self.defs, self.body, self.n = name, title, [], [], 0

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
        svg = (f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}">'
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
    # search pill + clock
    sx = W - M - 110 - 330
    sv.add(rect(sx, 44, 330, 48, C["surface"], rx=24, stroke=C["line"], sw=1),
           circle(sx + 30, 66, 8, "none", stroke=C["muted"], sw=2.5),
           f'<line x1="{sx + 36}" y1="{72}" x2="{sx + 42}" y2="{78}" stroke="{C["muted"]}" stroke-width="2.5"'
           f' stroke-linecap="round"/>',
           text(sx + 52, 74, "Search games", 18, 400, fill=C["muted"]))
    l3_x = sx + 330 - 14 - 40
    glyph(sv, l3_x, 68, "L3")
    glyph(sv, l3_x - 8 - (tw("Ctrl F", 14, 700) + 18), 68, "Ctrl F")
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
    emulator = {"3ds": "Azahar", "gc": "Dolphin", "wii": "Dolphin", "wiiu": "Cemu", "switch": "Ryubing",
                "ps2": "PCSX2", "ps3": "RPCS3", "ps4": "shadPS4"}.get(focus, "Game37")
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


def details_page(name, title, g, sysid, emulator, stats, path, favorite=False, missing=False):
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
    sv.add(rect(x + 290, 604, 236, 76, C["surface"], rx=38, stroke=C["line"], sw=2, opacity=0.85))
    glyph(sv, x + 322, 642, "Y")
    sv.add(text(x + 368, 651, "Options", 24, 700))
    if missing:
        sv.add(circle(x + 14, 731, 14, WARN), text(x + 14, 738, "!", 18, 800, fill=C["bg"], anchor="middle"),
               text(x + 40, 738, f"{emulator} isn't installed, so Play is off. The install command is on the "
                                 f"{SYS[sysid][1]} page.", 19, 500, fill=WARN))
        hintbar(sv, [(["B"], "Back"), (["Y"], "Options")])
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


if __name__ == "__main__":
    for fn in [screen_home, screen_home_empty, screen_carousel, screen_console_page, screen_details,
               screen_options, screen_pause, screen_ds, screen_console_missing_emulator,
               screen_details_missing_emulator]:
        fn()
    print("\n".join(sorted(p.name for p in OUT.glob("*.svg"))))

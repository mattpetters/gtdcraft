# Engraved elephant portrait for GTDCraft (original artwork, generated line by line).
import math, sys
out = sys.argv[1]
INK = "#1c1a17"; CREAM = "#f4eddc"; IVORY = "#fbf7ec"; BG = "#c79a5b"
L = []
add = L.append

def bez(segs, close=True):
    (sx, sy), rest = segs[0], segs[1:]
    d = f"M{sx:.1f} {sy:.1f} " + " ".join(f"C{a[0]:.1f} {a[1]:.1f} {b[0]:.1f} {b[1]:.1f} {c[0]:.1f} {c[1]:.1f}" for a, b, c in rest)
    return d + (" Z" if close else "")

def mir(segs):
    m = lambda p: (1024 - p[0], p[1])
    return [m(segs[0])] + [(m(a), m(b), m(c)) for a, b, c in segs[1:]]

def scale(segs, cx, cy, k):
    f = lambda p: (cx + (p[0] - cx) * k, cy + (p[1] - cy) * k)
    return [f(segs[0])] + [(f(a), f(b), f(c)) for a, b, c in segs[1:]]

def cubic(p0, p1, p2, p3, t):
    u = 1 - t
    return tuple(u**3 * a + 3 * u * u * t * b + 3 * u * t * t * c + t**3 * d for a, b, c, d in zip(p0, p1, p2, p3))

def poly(pts, close=True):
    return "M" + " L".join(f"{x:.1f} {y:.1f}" for x, y in pts) + (" Z" if close else "")

# ---- shapes ----
ear_l = [(400, 296), ((300, 196), (140, 226), (116, 396)), ((98, 540), (150, 664), (240, 724)), ((292, 758), (334, 744), (356, 702)), ((372, 670), (382, 640), (396, 606))]
ear_r = mir(ear_l)
head = [(512, 196), ((622, 192), (704, 258), (708, 370)), ((712, 470), (670, 548), (626, 606)), ((560, 600), (464, 600), (398, 606)), ((354, 548), (312, 470), (316, 370)), ((320, 258), (402, 192), (512, 196))]

# trunk: centreline S-curve with tapering width
C = [(512, 530), (522, 690), (452, 800), (548, 1010)]
def trunk_at(t):
    p = cubic(*C, t)
    q = cubic(*C, min(1, t + 0.01)) if t < 1 else p
    r = cubic(*C, max(0, t - 0.01))
    dx, dy = q[0] - r[0], q[1] - r[1]
    n = math.hypot(dx, dy) or 1
    nx, ny = -dy / n, dx / n
    w = 168 - 104 * t + 40 * max(0.0, 0.18 - t) * 3
    return p, (nx, ny), w
ts = [i / 80 for i in range(81)]
left = []; right = []
for t in ts:
    p, (nx, ny), w = trunk_at(t)
    left.append((p[0] - nx * w / 2, p[1] - ny * w / 2))
    right.append((p[0] + nx * w / 2, p[1] + ny * w / 2))
trunk_poly = left + right[::-1]
tusk_l = [(446, 590), ((424, 660), (394, 730), (356, 776)), ((384, 782), (428, 740), (470, 640))]
tusk_r = mir(tusk_l)

add('<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">')
add('<!-- GTDCraft: an elephant ("an elephant never forgets"), fine-line engraving on kraft tan. Original artwork. -->')
add('<defs><clipPath id="tile"><rect x="100" y="100" width="824" height="824" rx="184"/></clipPath>')
add(f'<clipPath id="earL"><path d="{bez(ear_l)}"/></clipPath><clipPath id="earR"><path d="{bez(ear_r)}"/></clipPath>')
add(f'<clipPath id="head"><path d="{bez(head)}"/></clipPath><clipPath id="trunk"><path d="{poly(trunk_poly)}"/></clipPath>')
add('<linearGradient id="fadeG" gradientUnits="userSpaceOnUse" x1="0" y1="560" x2="0" y2="630"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#fff"/></linearGradient>')
add('<mask id="trunkFade" maskUnits="userSpaceOnUse" x="0" y="0" width="1024" height="1024"><rect x="0" y="0" width="1024" height="1024" fill="url(#fadeG)"/></mask>')
add('<clipPath id="shadeL"><path d="M0 0 L470 0 L380 1024 L0 1024 Z"/></clipPath>')
add('</defs>')
add(f'<rect x="100" y="100" width="824" height="824" rx="184" fill="{BG}"/>')
add('<g clip-path="url(#tile)">')
for i in range(60):
    y = 106 + i * 14
    add(f'<path d="M100 {y} Q512 {y+5} 924 {y}" stroke="#b6884c" stroke-width="1.1" fill="none" opacity="0.4"/>')

def open_g(clip):
    add(f'<g clip-path="url(#{clip})" stroke="{INK}" fill="none" stroke-linecap="round">')

# ---- ears ----
for segs, cx, cy, side in ((ear_l, 270, 480, -1), (ear_r, 754, 480, 1)):
    add(f'<path d="{bez(segs)}" fill="{CREAM}"/>')
    open_g("earL" if side < 0 else "earR")
    k = 0.97
    while k > 0.03:
        add(f'<path d="{bez(scale(segs, cx, cy, k), close=False)}" stroke-width="{1.0 + 0.5 * (1 - k):.2f}" opacity="0.85"/>')
        k -= 0.024
    # shadow next to the head: cross-hatch
    hx = 400 if side < 0 else 624
    for j in range(60):
        x = hx - side * j * 3.2
        add(f'<path d="M{x:.1f} 260 L{x - side * 70:.1f} 760" stroke-width="0.9" opacity="{max(0.0, 0.85 - j * 0.014):.2f}"/>')
    for (a, b, c, d) in [(360, 380, 200, 330), (350, 470, 170, 470), (350, 560, 190, 610), (330, 640, 240, 700)]:
        if side > 0:
            a, c = 1024 - a, 1024 - c
        add(f'<path d="M{a} {b} Q{(a + c) / 2} {(b + d) / 2 - 22} {c} {d}" stroke-width="2.1"/>')
    add('</g>')
    add(f'<path d="{bez(segs)}" fill="none" stroke="{INK}" stroke-width="3.4" stroke-linejoin="round"/>')

# ---- head ----
add(f'<path d="{bez(head)}" fill="{CREAM}"/>')
open_g("head")
k = 0.985
while k > 0.05:  # contour lines following the skull, converging low toward the trunk
    add(f'<path d="{bez(scale(head, 512, 640, k), close=False)}" stroke-width="{0.9 + 0.4 * (1 - k):.2f}" opacity="0.7"/>')
    k -= 0.028
add('</g>')
add('<g clip-path="url(#head)"><g clip-path="url(#shadeL)" stroke="#1c1a17" fill="none" opacity="0.8">')
for j in range(90):  # light from the right: hatch the left side
    x = 300 + j * 2.4
    add(f'<path d="M{x:.1f} 190 Q{x - 30:.1f} 420 {x + 10:.1f} 640" stroke-width="0.8" opacity="{max(0.0, 1 - j / 90):.2f}"/>')
add('</g></g>')
# outline without the bottom edge, so the trunk grows out of the face
head_open = [head[3][2], head[4], head[5], head[1], head[2]]
add(f'<path d="{bez(head_open, close=False)}" fill="none" stroke="{INK}" stroke-width="3.6" stroke-linejoin="round"/>')
# ---- trunk (grows out of the face, faded in with a soft mask) ----
add('<g mask="url(#trunkFade)">')
add(f'<path d="{poly(trunk_poly)}" fill="{CREAM}"/>')
open_g("trunk")
for i, t in enumerate([j / 46 for j in range(47)]):
    if i < 4:
        continue  # the trunk fades into the face: no hard top edge
    p, (nx, ny), w = trunk_at(t)
    a = (p[0] - nx * w / 2 - 4, p[1] - ny * w / 2)
    b = (p[0] + nx * w / 2 + 4, p[1] + ny * w / 2)
    m = (p[0] + ny * 7, p[1] - nx * 7 + 9)
    add(f'<path d="M{a[0]:.1f} {a[1]:.1f} Q{m[0]:.1f} {m[1]:.1f} {b[0]:.1f} {b[1]:.1f}" stroke-width="{2.3 if i % 3 == 0 else 1.3}" opacity="{min(1.0, (i - 3) / 6):.2f}"/>')
for s in range(1, 9):  # long shading lines along the dark (left) side
    pts = []
    for t in ts:
        p, (nx, ny), w = trunk_at(t)
        f = 0.5 - s * 0.045
        pts.append((p[0] - nx * w * f, p[1] - ny * w * f))
    add(f'<path d="{poly(pts, close=False)}" stroke-width="1.0" opacity="{0.9 - s * 0.07:.2f}"/>')
add('</g>')
add(f'<path d="{poly(left[6:], close=False)}" fill="none" stroke="{INK}" stroke-width="3.4"/>')
add(f'<path d="{poly(right[6:], close=False)}" fill="none" stroke="{INK}" stroke-width="3.4"/>')
add('</g>')

# brow ridge and forehead folds
for y, w in [(330, 2.4), (352, 1.8), (300, 1.4)]:
    add(f'<path d="M418 {y} Q512 {y - 26} 606 {y}" fill="none" stroke="{INK}" stroke-width="{w}"/>')
# eyes: deep-set, heavy brow, wrinkle rings
for ex, f in ((448, 1), (576, -1)):
    ey = 432
    add(f'<path d="M{ex - 40} {ey - 14} Q{ex} {ey - 44} {ex + 40} {ey - 14}" fill="none" stroke="{INK}" stroke-width="3"/>')
    for r, op in ((46, 0.45), (38, 0.6), (30, 0.75), (23, 0.9)):
        add(f'<path d="M{ex - r} {ey + 4} Q{ex} {ey - r * 0.75} {ex + r} {ey + 4}" fill="none" stroke="{INK}" stroke-width="1.2" opacity="{op}"/>')
        add(f'<path d="M{ex - r + 6} {ey + 10} Q{ex} {ey + 10 + r * 0.55} {ex + r - 6} {ey + 10}" fill="none" stroke="{INK}" stroke-width="1.1" opacity="{op - 0.15:.2f}"/>')
    add(f'<path d="M{ex - 18} {ey + 4} Q{ex} {ey - 12} {ex + 18} {ey + 4} Q{ex} {ey + 14} {ex - 18} {ey + 4} Z" fill="{INK}"/>')
    add(f'<circle cx="{ex + 4 * f}" cy="{ey}" r="3.2" fill="{IVORY}"/>')

# ---- tusks ----
for t, cx in ((tusk_l, 420), (tusk_r, 604)):
    add(f'<path d="{bez(t)}" fill="{IVORY}" stroke="{INK}" stroke-width="3.2" stroke-linejoin="round"/>')
    for k in (0.82, 0.64, 0.46):
        add(f'<path d="{bez(scale(t, cx, 690, k), close=False)}" fill="none" stroke="{INK}" stroke-width="0.9" opacity="0.6"/>')
add('</g>')
add('<rect x="100" y="100" width="824" height="824" rx="184" fill="none" stroke="#9c7440" stroke-width="2" opacity="0.6"/>')
add('</svg>')
open(out, "w").write("\n".join(L))
print("wrote", out, len(L))

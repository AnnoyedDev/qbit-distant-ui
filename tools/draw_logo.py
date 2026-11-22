import math, random, sys
from PIL import Image, ImageDraw

random.seed(11)
S = 1024
INK = (26, 22, 44, 255)
WHITE = (255, 255, 255, 255)
GREEN = (74, 196, 82, 255)
GREEN_DARK = (46, 150, 58, 255)
METAL = (196, 200, 206, 255)

img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)

def jit(p, a):
    return (p[0] + random.uniform(-a, a), p[1] + random.uniform(-a, a))

def bez(p0, p1, p2, p3, n=60):
    out = []
    for i in range(n + 1):
        t = i / n
        a, b, c, e = (1 - t) ** 3, 3 * t * (1 - t) ** 2, 3 * t * t * (1 - t), t ** 3
        out.append((a * p0[0] + b * p1[0] + c * p2[0] + e * p3[0], a * p0[1] + b * p1[1] + c * p2[1] + e * p3[1]))
    return out

def wobble(pts, amp):
    ph = random.uniform(0, 6)
    return [(x + math.sin(i * 0.35 + ph) * amp + random.uniform(-amp, amp) * 0.4, y + math.cos(i * 0.3 + ph) * amp * 0.6) for i, (x, y) in enumerate(pts)]

def stroke(pts, width, color):
    d.line(pts, fill=color, width=width, joint="curve")
    r = width / 2
    for x, y in (pts[0], pts[-1]):
        d.ellipse((x - r, y - r, x + r, y + r), fill=color)

def sketch(pts, fill, width=10, a=5, passes=2):
    d.polygon([jit(p, a) for p in pts], fill=fill)
    for _ in range(passes):
        q = [jit(p, a) for p in pts]
        d.line(q + [q[0], q[1]], fill=INK, width=width, joint="curve")

def rounded_box(x0, y0, x1, y1, r, n=10):
    pts = []
    for cx, cy, a0 in [(x1 - r, y0 + r, -90), (x1 - r, y1 - r, 0), (x0 + r, y1 - r, 90), (x0 + r, y0 + r, 180)]:
        for i in range(n + 1):
            a = math.radians(a0 + 90 * i / n)
            pts.append((cx + r * math.cos(a), cy + r * math.sin(a)))
    return pts

def circle_pts(cx, cy, r, n=72):
    return [(cx + r * math.cos(2 * math.pi * i / n), cy + r * math.sin(2 * math.pi * i / n)) for i in range(n)]

CX, CY, R = 430, 430, 370
ring = circle_pts(CX, CY, R)
d.polygon([jit(p, 5) for p in circle_pts(CX, CY, R + 30)], fill=INK)
d.polygon([jit(p, 4) for p in circle_pts(CX, CY, R + 14)], fill=WHITE)
d.polygon([jit(p, 4) for p in circle_pts(CX, CY, R - 18)], fill=(52, 120, 222, 255))
for i in range(9):
    y = CY + 120 + i * 26
    half = math.sqrt(max((R - 40) ** 2 - (y - CY) ** 2, 0))
    d.line([jit((CX - half * 0.55 + 140, y), 4), jit((CX + half * 0.9, y - 30), 4)], fill=(40, 98, 196, 255), width=9)
for _ in range(2):
    q = [jit(p, 5) for p in circle_pts(CX, CY, R + 30)]
    d.line(q + [q[0], q[1]], fill=INK, width=10, joint="curve")
stroke(wobble([(CX + (R - 75) * math.cos(math.radians(a)), CY + (R - 75) * math.sin(math.radians(a))) for a in range(200, 251, 5)], 2), 24, (170, 205, 255, 255))

def letter(paths):
    for pts in paths:
        stroke(wobble(pts, 2.5), 92, INK)
    for pts in paths:
        stroke(wobble(pts, 2), 62, WHITE)

q_bowl = [(285 + 100 * math.cos(math.radians(a)), 470 + 110 * math.sin(math.radians(a))) for a in range(-10, 352, 12)]
q_stem = bez((392, 370), (395, 480), (390, 600), (398, 700), 20) + [(430, 690)]
b_stem = bez((500, 230), (505, 380), (498, 520), (503, 610), 20)
b_top = bez((503, 240), (690, 220), (690, 420), (505, 418), 30)
b_bot = bez((505, 410), (730, 400), (720, 625), (503, 605), 30)
letter([q_bowl, q_stem, b_stem, b_top, b_bot])

def cable(p0, p1, p2, p3):
    pts = bez(p0, p1, p2, p3)
    stroke(wobble(pts, 3), 62, INK)
    stroke(wobble(pts, 2), 44, GREEN)
    stroke([(x - 6, y - 7) for x, y in wobble(pts, 2)][4:-4], 9, (150, 230, 150, 255))

def b(x0, y0, x1, y1, fill, w=9):
    sketch([(x0, y0), (x1, y0 + 3), (x1 - 2, y1), (x0 + 3, y1 - 2)], fill, width=w, a=4)

cable((-40, 1060), (120, 940), (380, 1000), (575, 898))
cable((1080, 600), (980, 760), (1000, 880), (835, 898))
b(560, 850, 672, 948, GREEN_DARK)
b(578, 866, 600, 932, GREEN)
for y in (872, 906):
    b(668, y, 752, y + 20, METAL, 7)
b(712, 846, 850, 952, GREEN_DARK)
for y in (872, 906):
    d.line([(712, y + 2), (752, y + 4)], fill=METAL, width=12)
    d.line([(712, y + 18), (752, y + 19)], fill=INK, width=4)
for x0, y0, x1, y1 in [(684, 830, 694, 802), (700, 836, 716, 812), (686, 966, 698, 996), (703, 960, 722, 985)]:
    d.line([(x0, y0), (x1, y1)], fill=INK, width=7)

img.save(sys.argv[1])

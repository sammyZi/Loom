"""Rasterise the app's brand mark (Icons.tsx IconMark) into cli/icon.ico."""
import math
from PIL import Image, ImageDraw

S = 1024
K = S / 16.0 * 0.82              # the mark is authored in a 16x16 viewBox, inset a little
BG = (23, 23, 22, 255)           # --bg #171716
FG = (20, 184, 166, 255)         # --accent-hi #14b8a6

# One petal: "M8 8 A4 4 0 0 1 8 2 A4 4 0 0 1 8 8 Z" — a lens between (8,8) and
# (8,2) made of two r=4 arcs, centred sqrt(4^2 - 3^2) either side of x=8.
off = math.sqrt(16 - 9)
def arc(cx, cy, a0, a1, n=64):
    return [(cx + 4 * math.cos(a0 + (a1 - a0) * i / n),
             cy + 4 * math.sin(a0 + (a1 - a0) * i / n)) for i in range(n + 1)]

t = math.atan2(3, off)
petal = arc(8 - off, 5, t, -t) + arc(8 + off, 5, math.pi + t, math.pi - t)

img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
d.rounded_rectangle([0, 0, S - 1, S - 1], radius=int(S * 0.22), fill=BG)

for deg in range(0, 360, 60):
    r = math.radians(deg)
    pts = []
    for x, y in petal:
        dx, dy = x - 8, y - 8
        pts.append((S / 2 + (dx * math.cos(r) - dy * math.sin(r)) * K,
                    S / 2 + (dx * math.sin(r) + dy * math.cos(r)) * K))
    d.polygon(pts, fill=FG)

img.save("D:/projects/ide_ai/cli/icon.ico",
         sizes=[(256, 256), (128, 128), (64, 64), (48, 48), (32, 32), (16, 16)])
img.resize((256, 256), Image.LANCZOS).save(r"C:/Users/samarth/AppData/Local/Temp/claude/D--projects-ide-ai/1b427e68-1aa9-40e9-8730-20b10d697fd8/scratchpad/icon-preview.png")
img.resize((32, 32), Image.LANCZOS).resize((128, 128), Image.NEAREST).save(r"C:/Users/samarth/AppData/Local/Temp/claude/D--projects-ide-ai/1b427e68-1aa9-40e9-8730-20b10d697fd8/scratchpad/icon-32.png")

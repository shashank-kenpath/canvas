"""Render the code-native Canvas Studio mark at launcher sizes using Pillow.

Run from the project root. All paths stay within this project.
"""
from pathlib import Path
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent


def render(size):
    factor = 8
    scale = size * factor / 108
    im = Image.new('RGBA', (size * factor, size * factor))
    draw = ImageDraw.Draw(im)

    def rect(box, radius, color):
        draw.rounded_rectangle(tuple(round(v * scale) for v in box), round(radius * scale), fill=color)

    def poly(points, color):
        draw.polygon([(round(x * scale), round(y * scale)) for x, y in points], fill=color)

    def quad(a, control, b, steps=16):
        return [((1-t)**2*a[0] + 2*(1-t)*t*control[0] + t*t*b[0],
                 (1-t)**2*a[1] + 2*(1-t)*t*control[1] + t*t*b[1])
                for t in (n/steps for n in range(steps+1))]

    rect((0, 0, 108, 108), 25, '#6750d8')
    rect((28, 23, 80, 87), 8, '#ffffff')
    rect((38, 36, 61, 40), 2, '#e7e0ff')
    rect((38, 46, 51, 50), 2, '#e7e0ff')
    poly([(40, 61), (64, 37), (76, 49), (52, 73)], '#ff8966')
    cap = [(64, 37), (68, 33)] + quad((68, 33), (72, 29), (76, 33)) + [(80, 37)]
    cap += quad((80, 37), (84, 41), (80, 45)) + [(76, 49)]
    poly(cap, '#ffb39b')
    poly([(44, 65), (68, 41), (72, 45), (48, 69)], '#ffa184')
    poly([(36, 77), (40, 61), (52, 73)], '#ffd8aa')
    poly([(36, 77), (38, 69), (44, 75)], '#453587')
    return im.resize((size, size), Image.Resampling.LANCZOS)


for density, size in [('mdpi', 48), ('hdpi', 72), ('xhdpi', 96), ('xxhdpi', 144), ('xxxhdpi', 192)]:
    path = ROOT / f'app/android/app/src/main/res/mipmap-{density}/ic_launcher.png'
    path.parent.mkdir(parents=True, exist_ok=True)
    render(size).save(path)
render(1024).save(ROOT / 'branding/logo.png')
print('Generated five legacy launcher PNGs and branding/logo.png')

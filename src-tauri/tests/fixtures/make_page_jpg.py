"""Writes page.jpg, the reader tests' page: 600 x 800, one white balloon
with a few dark lines of "text" in it on a grey panel.

    python -I make_page_jpg.py
"""
from pathlib import Path

from PIL import Image, ImageDraw

img = Image.new("RGB", (600, 800), (150, 150, 150))
d = ImageDraw.Draw(img)
d.ellipse((60, 40, 300, 280), fill="white", outline="black", width=5)
for i in range(4):
    d.rectangle((120, 110 + i * 26, 240, 124 + i * 26), fill=(20, 20, 20))
img.save(Path(__file__).with_name("page.jpg"), quality=85)

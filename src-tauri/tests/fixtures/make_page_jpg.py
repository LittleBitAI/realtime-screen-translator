"""Writes page.jpg, the reader tests' page: 600 x 800, one white balloon
with four dark lines of letter-sized marks in it on a grey panel (a bar a
line wide is not lettering to the reader, and has nothing drawn over it).

    python -I make_page_jpg.py
"""
from pathlib import Path

from PIL import Image, ImageDraw

img = Image.new("RGB", (600, 800), (150, 150, 150))
d = ImageDraw.Draw(img)
d.ellipse((60, 40, 300, 280), fill="white", outline="black", width=5)
for i in range(4):
    for k in range(8):
        d.rectangle((120 + k * 15, 110 + i * 26, 132 + k * 15, 124 + i * 26), fill=(20, 20, 20))
img.save(Path(__file__).with_name("page.jpg"), quality=85)

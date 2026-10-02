import glob, os, re, sys
from PIL import Image, ImageChops
out = sys.argv[1]
for book in ("book", "pictures"):
    same, differ, coloured = [], [], []
    for f in sorted(glob.glob(f"{out}/ebk-test-{book}-epub.*.png"), key=lambda f: int(re.findall(r"\.(\d+)\.png", f)[0])):
        page = int(re.findall(r"\.(\d+)\.png", f)[0])
        g = f.replace("-epub.", "-ebk.")
        if not os.path.exists(g):
            differ.append(page); continue
        a, b = Image.open(f).convert("RGB"), Image.open(g).convert("RGB")
        (same if a.size == b.size and ImageChops.difference(a, b).getbbox() is None else differ).append(page)
        r, gr, bl = b.split()
        if ImageChops.difference(r, gr).getbbox() or ImageChops.difference(gr, bl).getbbox(): coloured.append(page)
    print(f"{book}: {len(same)} pages identical {same}, differ {differ}, with pictures {coloured}")

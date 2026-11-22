"""Export the persona chooser images.

usage: python scripts/export-personas.py <source dir> <dest dir>

Reads <source>/angel/source.png and <source>/devil/source.png (transparent PNGs), trims fully transparent
margins, scales to 900 px height and writes <dest>/angel.webp and <dest>/devil.webp (WebP with alpha, q82, <= 400 KB).
"""
import sys
from pathlib import Path

from PIL import Image

HEIGHT = 900
QUALITY = 82
MAX_BYTES = 400 * 1024


def export(src: Path, dst: Path) -> None:
    im = Image.open(src).convert('RGBA')
    box = im.getchannel('A').point(lambda a: 255 if a > 8 else 0).getbbox()
    if box is None:
        raise SystemExit(f'{src}: fully transparent')
    im = im.crop(box)
    w = max(1, round(im.width * HEIGHT / im.height))
    im = im.resize((w, HEIGHT), Image.LANCZOS)
    dst.parent.mkdir(parents=True, exist_ok=True)
    im.save(dst, 'WEBP', quality=QUALITY, method=6)
    size = dst.stat().st_size
    assert size <= MAX_BYTES, f'{dst} is {size} bytes (> {MAX_BYTES})'
    print(f'{dst}: {w}x{HEIGHT}, {size // 1024} KB')


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    src, dst = Path(sys.argv[1]), Path(sys.argv[2])
    for name in ('angel', 'devil'):
        export(src / name / 'source.png', dst / f'{name}.webp')


if __name__ == '__main__':
    main()

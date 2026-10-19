#!/usr/bin/env python3
"""Explicit download for public screenshot fixtures; never run by Rphotos/CI.

Usage: python3 scripts/fetch-showcase-photos.py /tmp/opencode/rphotos-showcase-photos
Photo attribution and separate licenses: docs/screenshots/README.md.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from urllib.request import urlopen

COMMIT = "26e2d4d27113a86b9dd99dc7e559ab25aa782436"
BASE = f"https://raw.githubusercontent.com/scikit-image/skimage-tutorials/{COMMIT}/images/"
PHOTOS = {
    "01-coast.jpg": "Bells-Beach.jpg",
    "02-forest.jpg": "forest.jpg",
    "03-balloon.jpg": "balloon.jpg",
    "04-arches.jpg": "pano/JDW_0302.jpg",
    "05-arches.jpg": "pano/JDW_0303.jpg",
    "06-arches.jpg": "pano/JDW_9518.jpg",
    "07-arches.jpg": "pano/JDW_9519.jpg",
    "08-petra.jpg": "pano/DFM_4209.jpg",
    "09-petra.jpg": "pano/DFM_4210.jpg",
    "10-petra.jpg": "pano/DFM_4211.jpg",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    directory = parser.parse_args().directory
    directory.mkdir(parents=True, exist_ok=True)

    def download(item):
        name, source = item
        with urlopen(BASE + source, timeout=30) as response:
            data = response.read(16 * 1024 * 1024 + 1)
        if len(data) > 16 * 1024 * 1024:
            raise ValueError("Showcase image exceeds download limit")
        # Exclusive create: do not overwrite any existing user file.
        with (directory / name).open("xb") as output:
            output.write(data)
        return name

    with ThreadPoolExecutor(max_workers=4) as pool:
        for name in pool.map(download, PHOTOS.items()):
            print(name)


if __name__ == "__main__":
    main()

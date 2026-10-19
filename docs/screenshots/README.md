# Public UI captures

These PNGs are actual offscreen wgpu renders of the current Rphotos UI, not mockups.
They use a deliberately prepared public demo library. Album titles, favorites and
dates are demonstration data, not inferred EXIF or AI results. No user photographs,
tablet reference captures, desktop windows, databases or credentials are published.

## Photography credits and licenses

The application code license does **not** replace the photographs' licenses.
Photographs are displayed fitted/cropped by Rphotos; no authorship is claimed.
Source: [scikit-image tutorials](https://github.com/scikit-image/skimage-tutorials/tree/26e2d4d27113a86b9dd99dc7e559ab25aa782436/images), pinned commit
`26e2d4d27113a86b9dd99dc7e559ab25aa782436`.

| Photos visible in captures | Attribution | License |
| --- | --- | --- |
| `Bells-Beach.jpg`, `forest.jpg`, `balloon.jpg` | scikit-image tutorial contributors; individual photographer not identified in the repository | [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/), per [repository declaration](https://github.com/scikit-image/skimage-tutorials/blob/26e2d4d27113a86b9dd99dc7e559ab25aa782436/README.md#usage) |
| `JDW_0302.jpg`, `JDW_0303.jpg`, `JDW_9518.jpg`, `JDW_9519.jpg` — Arches National Park | © 2014 Joshua D. Warner | [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) |
| `DFM_4209.jpg`, `DFM_4210.jpg`, `DFM_4211.jpg` — Petra | © 2014 Francois Malan | [CC BY 3.0](https://creativecommons.org/licenses/by/3.0/) |

The panorama-specific licenses override the repository's general CC0 statement:
[original photo license notice](https://github.com/scikit-image/skimage-tutorials/blob/26e2d4d27113a86b9dd99dc7e559ab25aa782436/images/pano/LICENSE.txt).
Screenshots embed resized/cropped versions of these photographs. Viewer images are
fitted; album/card covers are center-cropped. UI artwork remains MIT OR Apache-2.0.

Regenerate with:

```sh
python3 scripts/fetch-showcase-photos.py /tmp/opencode/rphotos-showcase-photos
RPHOTOS_SHOWCASE_INPUT=/tmp/opencode/rphotos-showcase-photos \
RPHOTOS_CAPTURE_DIR=/tmp/opencode/rphotos-showcase \
  cargo test --locked capture_showcase -- --ignored --nocapture
```

Copy only the reviewed collection-light/dark, albums and viewer-light/dark PNGs.
The ordinary regression captures still use generated gradients; their behavior is
unchanged. This opt-in showcase never downloads anything itself, never reads the
user's index, and uses an isolated temporary SQLite library/cache.

These captures show layout, not native compositor blur, real touchscreen behavior
or image-analysis quality. No fake desktop blur or external window is composited.

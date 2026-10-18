# Public UI captures

These three PNG files are offscreen wgpu renders from Rphotos capture tests.
All visible images are generated RGB gradients; album names and filenames are
test fixtures. No user photographs, tablet reference captures, desktop windows,
library database, credentials or model output are included.

Regenerate with:

```sh
RPHOTOS_CAPTURE_DIR=/tmp/opencode/rphotos-captures cargo test --locked capture_ -- --ignored --nocapture
```

These captures show layout, not native compositor blur, real touchscreen behavior
or image-analysis quality. Artwork and generated fixtures follow the project's
MIT OR Apache-2.0 licensing.

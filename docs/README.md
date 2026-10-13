# LanLink Documentation Site

This directory contains the LanLink public documentation. It is written as
Markdown files that can be read directly or built into a static HTML site.

## Structure

- `SUMMARY.md` — mdBook-style table of contents.
- `intro.md` — project overview.
- `protocol.md` — wire protocol and control-plane API.
- `ops.md` — operator runbooks (install, configure, troubleshoot).
- `threat-model.md` — security model and privacy notes.
- `api.md` — coordinator REST API reference.
- `build.sh` — generates a static HTML site into `docs/_site/`.

## Build

The build script uses only Python 3 standard-library modules, so it works on a
fresh checkout without installing `mdbook` or `pandoc`:

```bash
bash docs/build.sh
```

The generated site will be in `docs/_site/`. Open `docs/_site/index.html` in a
browser, or serve it with any static file server.

To build with mdBook instead (when installed):

```bash
mdbook build docs
```

Both methods use the same `SUMMARY.md` and Markdown source files.

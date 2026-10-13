#!/usr/bin/env bash
# Build the LanLink documentation site into docs/_site/.
# Uses only Python 3 standard-library modules.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC_DIR="$SCRIPT_DIR"
OUT_DIR="$SCRIPT_DIR/_site"

mkdir -p "$OUT_DIR"
mkdir -p "$OUT_DIR/static"

python3 - "$SRC_DIR" "$OUT_DIR" <<'PY'
import html
import os
import re
import sys
from pathlib import Path

SRC = Path(sys.argv[1])
OUT = Path(sys.argv[2])

# Minimal markdown-to-HTML converter (stdlib only).
def md_to_html(text: str) -> str:
    # Escape HTML first.
    text = html.escape(text)

    # Code blocks (``` ... ```).
    def code_block(m):
        code = m.group(1).strip('\n')
        return f'<pre><code>{code}</code></pre>'
    text = re.sub(r'```\n?(.*?)```', code_block, text, flags=re.S)

    # Inline code.
    text = re.sub(r'`([^`]+)`', r'<code>\1</code>', text)

    # Headers.
    text = re.sub(r'^###### (.*)$', r'<h6>\1</h6>', text, flags=re.M)
    text = re.sub(r'^##### (.*)$', r'<h5>\1</h5>', text, flags=re.M)
    text = re.sub(r'^#### (.*)$', r'<h4>\1</h4>', text, flags=re.M)
    text = re.sub(r'^### (.*)$', r'<h3>\1</h3>', text, flags=re.M)
    text = re.sub(r'^## (.*)$', r'<h2>\1</h2>', text, flags=re.M)
    text = re.sub(r'^# (.*)$', r'<h1>\1</h1>', text, flags=re.M)

    # Bold/italic.
    text = re.sub(r'\*\*\*(.+?)\*\*\*', r'<strong><em>\1</em></strong>', text)
    text = re.sub(r'\*\*(.+?)\*\*', r'<strong>\1</strong>', text)
    text = re.sub(r'\*(.+?)\*', r'<em>\1</em>', text)

    # Links [text](url); local .md links become .html.
    def link_repl(m):
        label = m.group(1)
        url = m.group(2)
        if url.endswith('.md') and not url.startswith(('http://', 'https://', 'mailto:', '#')):
            url = url[:-3] + '.html'
        return f'<a href="{url}">{label}</a>'
    text = re.sub(r'\[([^\]]+)\]\(([^)]+)\)', link_repl, text)

    # Paragraphs / line breaks.
    blocks = text.split('\n\n')
    out_blocks = []
    in_list = False
    list_items = []
    for block in blocks:
        block = block.strip()
        if not block:
            continue

        # Unordered list.
        if re.match(r'^[-*] ', block, flags=re.M):
            items = re.findall(r'^[-*] (.*)$', block, flags=re.M)
            list_html = '<ul>\n' + ''.join(f'<li>{item}</li>\n' for item in items) + '</ul>'
            out_blocks.append(list_html)
            continue

        # Tables (simple pipe tables, no alignment).
        if '|' in block and '\n' in block:
            lines = block.splitlines()
            if all('|' in line for line in lines) and len(lines) >= 2:
                rows = []
                for i, line in enumerate(lines):
                    if i == 1:
                        # separator line
                        continue
                    cells = [c.strip() for c in line.split('|')]
                    cells = [c for c in cells if c or c == '']
                    tag = 'th' if i == 0 else 'td'
                    rows.append('<tr>' + ''.join(f'<{tag}>{c}</{tag}>' for c in cells) + '</tr>')
                out_blocks.append('<table>' + ''.join(rows) + '</table>')
                continue

        # Already HTML-wrapped headings/code blocks/lists/tables stay as-is.
        if block.startswith(('<h', '<pre', '<ul', '<ol', '<table')):
            out_blocks.append(block)
            continue

        # Single line break inside a paragraph becomes <br>.
        block = block.replace('\n', '<br>\n')
        out_blocks.append(f'<p>{block}</p>')

    return '\n\n'.join(out_blocks)


def parse_summary(path: Path) -> list:
    """Return list of (title, filename) from SUMMARY.md."""
    entries = []
    for line in path.read_text().splitlines():
        m = re.search(r'\[([^\]]+)\]\(([^)]+)\)', line)
        if m:
            entries.append((m.group(1), m.group(2)))
    return entries


def build_nav(entries, current_file):
    links = []
    for title, filename in entries:
        cls = 'active' if filename == current_file else ''
        nav_filename = filename.replace('.md', '.html')
        links.append(f'<a href="{nav_filename}" class="{cls}">{html.escape(title)}</a>')
    return '\n'.join(links)


def build_page(title, body, nav, rel_root):
    css = f'{rel_root}static/style.css'
    return f'''<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{html.escape(title)} — LanLink Docs</title>
  <link rel="stylesheet" href="{css}">
</head>
<body>
  <div class="sidebar">
    <div class="brand">LanLink Docs</div>
    <nav>
{nav}
    </nav>
  </div>
  <main>
{body}
  </main>
</body>
</html>
'''


entries = parse_summary(SRC / 'SUMMARY.md')

for title, filename in entries:
    md_path = SRC / filename
    out_path = OUT / filename.replace('.md', '.html')
    md_text = md_path.read_text()
    body = md_to_html(md_text)
    rel_root = './' if filename.count('/') == 0 else '../' * filename.count('/')
    nav = build_nav(entries, filename)
    html_page = build_page(title, body, nav, rel_root)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(html_page)

print(f"Generated {len(entries)} pages in {OUT}")
PY

# Copy CSS and any other static assets.
cp -R "$SRC_DIR/static/"* "$OUT_DIR/static/" 2>/dev/null || true

# Provide an index page that redirects to the introduction.
ln -sf intro.html "$OUT_DIR/index.html" 2>/dev/null || cp "$OUT_DIR/intro.html" "$OUT_DIR/index.html"

echo "Done: $OUT_DIR/index.html"

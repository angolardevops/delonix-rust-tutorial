#!/usr/bin/env python3
"""Gerador do site «Delonix Rust Tutorial».

    python3 build.py            # escreve em site/
    python3 build.py --check    # falha se algum {{directiva}} ou link interno estiver partido

Markdown (content/*.md) → HTML estático, sem framework: uma dependência (`markdown`) e nada de
Node. Directivas, uma por linha:

    {{snippet:id}}            excerto do delonix-runtime (content/snippets.json, commit fixo)
    {{file:caminho[:a-b]}}    ficheiro do PRÓPRIO tutorial (código testado em CI)
    {{out:nome}}              saída real de um comando (content/outputs/nome.txt)
    {{svg:nome}}              diagrama inline (content/svg/nome.svg)
"""
import html, json, re, shutil, sys, unicodedata
from html.parser import HTMLParser
from pathlib import Path

import markdown

ROOT = Path(__file__).parent
OUT = ROOT / "site"
SITE_NAME = "Delonix Rust Tutorial"
REPO = "https://github.com/angolardevops/delonix-rust-tutorial"
SNIPPETS = json.loads((ROOT / "content/snippets.json").read_text())

PARTS = {
    0: "Começar",
    1: "Fundamentos",
    2: "Linux e containers",
    3: "Rust para sistemas",
    4: "Projecto: minicontainer",
    5: "Contribuir",
}

errors: list[str] = []


def slugify(s: str) -> str:
    s = unicodedata.normalize("NFD", s.lower())
    s = "".join(c for c in s if unicodedata.category(c) != "Mn")
    return re.sub(r"[^a-z0-9]+", "-", s).strip("-")


def front_matter(text: str):
    m = re.match(r"^---\n(.*?)\n---\n", text, re.S)
    meta = {}
    if m:
        for line in m.group(1).splitlines():
            k, _, v = line.partition(":")
            meta[k.strip()] = v.strip().strip(chr(34))
        text = text[m.end():]
    return meta, text


# ─── directivas → blocos HTML ──────────────────────────────────────────────────────────────────

def code_block(code: str, lang: str, title: str, link: str | None = None, collapsed=False) -> str:
    esc = html.escape(code.rstrip("\n"))
    head = f'<span class="code-title">{html.escape(title)}</span>'
    if link:
        head += f'<a class="code-src" href="{link}" target="_blank" rel="noopener">ver no GitHub ↗</a>'
    body = f'<pre><code class="language-{lang}">{esc}</code></pre>'
    if collapsed:
        n = code.count("\n") + 1
        return (f'<details class="code-fig"><summary><span class="chev"></span>{head}'
                f'<span class="code-lines">{n} linhas</span></summary>{body}</details>')
    return f'<figure class="code-fig"><figcaption>{head}</figcaption>{body}</figure>'


def render_directive(kind: str, arg: str, blocks: list[str], page: str) -> str:
    if kind == "snippet":
        s = SNIPPETS.get(arg)
        if not s:
            errors.append(f"{page}: snippet desconhecido {arg!r}")
            return "<!-- snippet em falta -->"
        meta = SNIPPETS["_meta"]
        title = f'delonix-runtime · {s["path"]} · linhas {s["lines"][0]}–{s["lines"][1]}'
        return code_block(s["code"], s["lang"], title, s["url"])
    if kind == "file":
        # `caminho`, `caminho:a-b` (linhas) ou `caminho#regiao` (// region: regiao … // endregion)
        region = None
        if "#" in arg:
            arg, _, region = arg.partition("#")
        path, _, rng = arg.partition(":")
        f = ROOT / path
        if not f.exists():
            errors.append(f"{page}: ficheiro em falta {path}")
            return "<!-- ficheiro em falta -->"
        lines = f.read_text().split("\n")
        lo, note = 1, ""
        if region:
            try:
                a = next(i for i, l in enumerate(lines) if l.strip() == f"// region: {region}")
                b = next(i for i in range(a + 1, len(lines)) if lines[i].strip() == "// endregion")
            except StopIteration:
                errors.append(f"{page}: região {region!r} não existe em {path}")
                return "<!-- região em falta -->"
            lo, lines, note = a + 2, lines[a + 1 : b], f" · {region}"
        elif rng:
            lo, hi = (int(x) for x in rng.split("-"))
            lines, note = lines[lo - 1 : hi], f" · linhas {lo}–{hi}"
        lang = {"rs": "rust", "sh": "bash", "toml": "toml", "py": "python", "yml": "yaml", "json": "json"}.get(f.suffix[1:], "plaintext")
        tail = f"#L{lo}-L{lo + len(lines) - 1}" if (rng or region) else ""
        return code_block("\n".join(lines), lang, f"{path}{note}", f"{REPO}/blob/main/{path}{tail}", collapsed=len(lines) > 80)
    if kind == "out":
        f = ROOT / "content/outputs" / f"{arg}.txt"
        if not f.exists():
            errors.append(f"{page}: saída em falta {arg}")
            return "<!-- saída em falta -->"
        lines = []
        for ln in f.read_text().rstrip("\n").split("\n"):
            e = html.escape(ln)
            if ln.startswith("$ "):
                e = f'<span class="t-prompt">$</span> <span class="t-cmd">{html.escape(ln[2:])}</span>'
            elif re.match(r"^\[exit \d+\]$", ln):
                e = f'<span class="t-exit">{e}</span>'
            lines.append(e)
        return ('<figure class="term"><figcaption><span class="dots"><i></i><i></i><i></i></span>'
                '<span class="code-title">saída real · medida neste host</span></figcaption>'
                f'<pre>{chr(10).join(lines)}</pre></figure>')
    if kind == "svg":
        f = ROOT / "content/svg" / f"{arg}.svg"
        if not f.exists():
            errors.append(f"{page}: svg em falta {arg}")
            return "<!-- svg em falta -->"
        return f'<figure class="diagram">{f.read_text()}</figure>'
    errors.append(f"{page}: directiva desconhecida {kind}")
    return ""


DIRECTIVE = re.compile(r"^\{\{(snippet|file|out|svg):([^}]+)\}\}\s*$", re.M)


# ─── índice de pesquisa ───────────────────────────────────────────────────────────────────────

class Sectionizer(HTMLParser):
    """Parte o HTML em secções (h1-h3) para o índice de pesquisa."""

    def __init__(self):
        super().__init__()
        self.sections = []  # {a, h, x}
        self.cur = {"a": "", "h": "", "x": []}
        self.in_head = None
        self.skip = 0

    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        if tag in ("h1", "h2", "h3"):
            self.flush()
            self.in_head = tag
            self.cur = {"a": a.get("id", ""), "h": "", "x": []}
        if tag == "svg":
            self.skip += 1

    def handle_endtag(self, tag):
        if tag == self.in_head:
            self.in_head = None
        if tag == "svg":
            self.skip -= 1

    def handle_data(self, data):
        if self.skip:
            return
        if self.in_head:
            self.cur["h"] += data
        else:
            self.cur["x"].append(data)

    def flush(self):
        text = re.sub(r"\s+", " ", " ".join(self.cur["x"])).strip()
        if self.cur["h"] or text:
            self.sections.append({"a": self.cur["a"], "h": self.cur["h"].strip().rstrip("¶#").strip(), "x": text[:900]})


def sectionize(body_html: str):
    s = Sectionizer()
    s.feed(body_html)
    s.flush()
    return s.sections


# ─── build ─────────────────────────────────────────────────────────────────────────────────────

def load_pages():
    pages = []
    for f in sorted((ROOT / "content").glob("*.md")):
        meta, text = front_matter(f.read_text())
        if "title" not in meta:
            errors.append(f"{f.name}: falta 'title' no front matter")
            continue
        pages.append({
            "src": f.name, "slug": meta.get("slug", f.stem.split("-", 1)[-1] if f.stem[0].isdigit() else f.stem),
            "title": meta["title"], "summary": meta.get("summary", ""), "part": int(meta.get("part", 0)),
            "order": int(meta.get("order", 0)), "time": meta.get("time", ""), "text": text,
        })
    pages.sort(key=lambda p: (p["part"], p["order"]))
    return pages


def render(page, md_ext):
    blocks: list[str] = []

    def stash(m):
        blocks.append(render_directive(m.group(1), m.group(2).strip(), blocks, page["src"]))
        return f"\n@@BLOCK{len(blocks) - 1}@@\n"

    text = DIRECTIVE.sub(stash, page["text"])
    md = markdown.Markdown(extensions=md_ext, extension_configs={"toc": {"permalink": "#", "slugify": lambda v, s: slugify(v), "toc_depth": "2-3"}})
    body = md.convert(text)
    for i, b in enumerate(blocks):
        body = body.replace(f"<p>@@BLOCK{i}@@</p>", b)
    if "@@BLOCK" in body:
        errors.append(f"{page['src']}: directiva não substituída (deve estar sozinha numa linha)")
    return body, md.toc_tokens


def toc_html(tokens):
    def li(t):
        kids = "".join(li(c) for c in t.get("children", []))
        return f'<li><a href="#{t["id"]}">{html.escape(t["name"])}</a>' + (f"<ul>{kids}</ul>" if kids else "") + "</li>"
    inner = "".join(li(c) for t in tokens for c in ([t] if t["level"] >= 2 else t.get("children", [])))
    return f"<ul>{inner}</ul>" if inner else ""


TEMPLATE = (ROOT / "site-src/template.html").read_text()


def nav_html(pages, current):
    out, last = [], None
    for p in pages:
        if p["part"] != last:
            if last is not None:
                out.append("</ul></section>")
            out.append(f'<section class="nav-part"><h4>{PARTS[p["part"]]}</h4><ul>')
            last = p["part"]
        cls = ' class="active" aria-current="page"' if p is current else ""
        out.append(f'<li><a href="{p["slug"]}.html"{cls}>{html.escape(p["title"])}</a></li>')
    out.append("</ul></section>")
    return "".join(out)


def main():
    check = "--check" in sys.argv
    pages = load_pages()
    # Esvazia em vez de apagar `site/`: quem tiver um servidor local com esse cwd não fica com um directório morto.
    if OUT.exists():
        for child in OUT.iterdir():
            shutil.rmtree(child) if child.is_dir() else child.unlink()
    (OUT / "assets").mkdir(parents=True, exist_ok=True)
    for f in (ROOT / "site-src/assets").iterdir():
        shutil.copy(f, OUT / "assets" / f.name)

    ext = ["fenced_code", "tables", "toc", "admonition", "attr_list", "sane_lists"]
    index, slugs = [], {p["slug"] for p in pages}
    for i, p in enumerate(pages):
        body, tokens = render(p, ext)
        # links internos: `algo.html#ancora` ou `algo.html`
        for m in re.finditer(r'href="([a-z0-9-]+)\.html(?:#[^"]*)?"', body):
            if m.group(1) not in slugs and m.group(1) != "404":
                errors.append(f"{p['src']}: link interno partido → {m.group(1)}.html")
        prev = pages[i - 1] if i > 0 else None
        nxt = pages[i + 1] if i + 1 < len(pages) else None
        pn = ""
        if prev:
            pn += f'<a class="pn prev" href="{prev["slug"]}.html"><small>← Anterior</small><span>{html.escape(prev["title"])}</span></a>'
        if nxt:
            pn += f'<a class="pn next" href="{nxt["slug"]}.html"><small>Seguinte →</small><span>{html.escape(nxt["title"])}</span></a>'
        meta_line = f'<p class="meta">{PARTS[p["part"]]}' + (f' · {p["time"]}' if p["time"] else "") + "</p>" if p["slug"] != "index" else ""
        page_html = (TEMPLATE
                     .replace("{{TITLE}}", html.escape(p["title"] + " · " + SITE_NAME if p["slug"] != "index" else SITE_NAME))
                     .replace("{{DESC}}", html.escape(p["summary"]))
                     .replace("{{SITE}}", SITE_NAME)
                     .replace("{{NAV}}", nav_html(pages, p))
                     .replace("{{TOC}}", toc_html(tokens))
                     .replace("{{META}}", meta_line)
                     .replace("{{BODY}}", body)
                     .replace("{{PREVNEXT}}", pn)
                     .replace("{{REPO}}", REPO)
                     .replace("{{EDIT}}", f"{REPO}/edit/main/content/{p['src']}")
                     .replace("{{BODYCLASS}}", "home" if p["slug"] == "index" else "chapter"))
        (OUT / f"{p['slug']}.html").write_text(page_html)
        for s in sectionize(body):
            index.append({"p": p["slug"], "t": p["title"], "h": s["h"], "a": s["a"], "x": s["x"], "g": PARTS[p["part"]]})
    (OUT / "search-index.json").write_text(json.dumps(index, ensure_ascii=False, separators=(",", ":")))
    nf = (ROOT / "site-src/404.html")
    if nf.exists():
        shutil.copy(nf, OUT / "404.html")
    (OUT / ".nojekyll").write_text("")
    print(f"{len(pages)} páginas, {len(index)} secções indexadas, {(OUT / 'search-index.json').stat().st_size // 1024} KiB de índice")
    if errors:
        print("\n".join("ERRO: " + e for e in errors), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()

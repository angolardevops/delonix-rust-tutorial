#!/usr/bin/env python3
"""Extrai excertos do delonix-runtime para content/snippets.json, a partir de um commit FIXO.

Os capítulos citam código real do motor com `{{snippet:id}}`. Ler de um commit (git show) e não
da árvore de trabalho garante que o excerto é reprodutível e que o link «ver no GitHub» é um
permalink que não muda debaixo do leitor.

    scripts/extract-snippets.py --repo ~/workspace/ngolacloud/delonix-runtime --sha a7b017c7
"""
import argparse, json, subprocess, sys
from pathlib import Path

# id: (ficheiro, primeira linha, última linha, linguagem)
SNIPPETS = {
    "typestate":        ("crates/foundation/delonix-model/src/typestate.rs", 1, 60, "rust"),
    "verify-digest":    ("crates/adapters/delonix-oci/src/registry.rs", 978, 1003, "rust"),
    "safe-bind-target": ("crates/adapters/delonix-linux/src/lib.rs", 1215, 1244, "rust"),
    "exit-codes":       ("crates/foundation/delonix-model/src/exitcode.rs", 142, 166, "rust"),
    "store-update":     ("crates/adapters/delonix-state/src/store.rs", 538, 561, "rust"),
    "write-atomic":     ("crates/adapters/delonix-state/src/store.rs", 182, 227, "rust"),
    "arch-layers":      ("scripts/arch_fitness.py", 51, 96, "python"),
    "vm-backend":       ("crates/adapters/delonix-vm/src/lib.rs", 815, 850, "rust"),
    "cri-cgroup":       ("crates/interfaces/delonix-cri/src/runtime_svc.rs", 535, 560, "rust"),
    "reconcile-plan":   ("crates/contexts/delonix-stack/src/reconcile.rs", 343, 392, "rust"),
    "reconcile-doc":    ("crates/contexts/delonix-stack/src/reconcile.rs", 1, 36, "rust"),
}

# excertos que terminam a meio de um bloco: acrescenta-se uma reticência visível
TRUNCATED = {"reconcile-plan", "vm-backend", "store-update"}

ap = argparse.ArgumentParser()
ap.add_argument("--repo", required=True)
ap.add_argument("--sha", required=True)
a = ap.parse_args()
repo = str(Path(a.repo).expanduser())
full = subprocess.check_output(["git", "-C", repo, "rev-parse", a.sha], text=True).strip()

out = {"_meta": {"repo": "angolardevops/delonix-runtime", "sha": full}}
for sid, (path, lo, hi, lang) in SNIPPETS.items():
    text = subprocess.check_output(["git", "-C", repo, "show", f"{full}:{path}"], text=True).split("\n")
    code = "\n".join(text[lo - 1 : hi])
    if sid in TRUNCATED:
        code += "\n    // … (excerto)"
    out[sid] = {
        "path": path, "lines": [lo, hi], "lang": lang, "code": code,
        "url": f"https://github.com/angolardevops/delonix-runtime/blob/{full}/{path}#L{lo}-L{hi}",
    }
    print(f"{sid:18} {path}:{lo}-{hi} ({hi-lo+1} linhas)")
Path("content").mkdir(exist_ok=True)
Path("content/snippets.json").write_text(json.dumps(out, indent=1, ensure_ascii=False))

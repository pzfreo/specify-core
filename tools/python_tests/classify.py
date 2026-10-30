"""Per test function: the specify_core names (and build123d) it reaches, via module helpers."""
import ast, sys, pathlib, json
root = pathlib.Path(sys.argv[1])
OURS = {"analyse", "questions", "parts", "load"}
out = {}
for f in sorted(root.glob("test_*.py")):
    tree = ast.parse(f.read_text())
    imported = {}  # local name -> module.name
    for n in tree.body:
        if isinstance(n, ast.ImportFrom) and n.module and (n.module.startswith("specify_core") or n.module.split(".")[0] in ("build123d", "quiddity", "OCP")):
            for a in n.names:
                imported[a.asname or a.name] = f"{n.module}.{a.name}"
        elif isinstance(n, ast.Import):
            for a in n.names:
                if a.name.split(".")[0] in ("specify_core",):
                    imported[a.asname or a.name] = a.name
    funcs = {n.name: n for n in tree.body if isinstance(n, ast.FunctionDef)}
    consts = set()
    def uses(fn, seen):
        found = set()
        for node in ast.walk(fn):
            if isinstance(node, ast.Name):
                if node.id in imported:
                    found.add(imported[node.id])
                elif node.id in funcs and node.id not in seen:
                    seen.add(node.id); found |= uses(funcs[node.id], seen)
            if isinstance(node, ast.Attribute) and isinstance(node.value, ast.Name) and node.value.id in imported and imported[node.value.id] in ("specify_core.api","specify_core.api.api","specify_core.general","specify_core.standards") :
                found.add(imported[node.value.id] + "." + node.attr)
            if isinstance(node, ast.Name) and node.id in ("subprocess",):
                found.add("subprocess")
        return found
    for name, fn in funcs.items():
        if not name.startswith("test_"): continue
        u = uses(fn, {name})
        u = {x.replace("specify_core.api.api.", "specify_core.api.") for x in u}
        u.discard("specify_core.api")
        args = [a.arg for a in fn.args.args]
        out[f"{f.name}::{name}"] = {"uses": sorted(u), "fixtures": args}
json.dump(out, sys.stdout, indent=0)

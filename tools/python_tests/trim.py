"""Copy specify-core's tests that the Rust-backed package can run into tests/python, trimmed
to those tests, with the ports PYTHON_ONLY.md lists.

    git -C /path/to/specify-core archive cc341dd | tar -x -C /tmp/specify-core-cc341dd
    python3 -I tools/python_tests/classify.py /tmp/specify-core-cc341dd/tests > /tmp/classify.json
    python3 -I tools/python_tests/trim.py /tmp/specify-core-cc341dd/tests tests/python /tmp/classify.json

conftest.py and PYTHON_ONLY.md in tests/python are written by hand and left as they are.
Each kept file has its docstring, its imports (specify_core names the Rust-backed package
lacks dropped, build123d/quiddity/OCP dropped), its module constants, the helpers and
fixtures the kept tests reach, and the kept tests, all verbatim.
"""
import ast, json, pathlib, sys

src, dest, cls = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), json.load(open(sys.argv[3]))
CORE = {"specify_core.api.analyse", "specify_core.api.questions", "specify_core.api.parts",
        "specify_core.rules.questions", "specify_core.api.apply", "specify_core.api.preview",
        "specify_core.load.load", "specify_core.api.write", "specify_core.api.write_parts",
        "specify_core.api.mesh", "specify_core.existing.read_existing",
        "specify_core.existing.part_settings"}
OK = CORE | {"specify_core.rules.ATTENTION", "specify_core.rules.LOCATING",
             "specify_core.rules.Intent", "specify_core.rules.IncompleteError",
             "specify_core.rules.with_all_defaults"}
# Reached inside the test (not seen by classify.py): the CLI, private rules, OpenCascade,
# Python's own internals (each listed in tests/python/PYTHON_ONLY.md).
EXCLUDE = {
    "test_writer.py::test_the_command_line_says_why_it_cannot_write",
    "test_load.py::test_index_of_round_trips",
    "test_mesh.py::test_a_face_that_does_not_triangulate_is_reported",
    "test_writer.py::test_a_failed_write_leaves_the_destination_and_the_source",
    "test_writer.py::test_a_failed_merge_leaves_a_part_with_pmi_as_it_was",
}
# Ported onto the public API (PRELUDE and PATCHES below; PYTHON_ONLY.md lists each).
INCLUDE = {
    "test_datums.py::" + t for t in (
        "test_nist_datum_scheme_round_trips", "test_a_datum_hole_keeps_its_size_but_leaves_its_set",
        "test_frames_are_offered_only_with_extra_datums", "test_a_frame_that_is_not_offered_is_refused",
        "test_frame_answers_lapse_when_the_extra_datum_goes",
        "test_without_a_there_are_no_positions_or_controls",
        "test_without_b_a_hole_c_has_no_control_but_keeps_a_position",
        "test_a_datum_hole_without_a_control_keeps_its_position")
} | {
    "test_same_as.py::test_datum_holes_are_never_merged",
    "test_mesh.py::test_triangles_face_outward",
    "test_mesh.py::test_each_face_range_covers_that_face",
    "test_mesh.py::test_a_placed_part_is_meshed_in_the_frame_the_analysis_uses",
    "test_writer.py::test_pmi_is_written_on_an_assembly_holding_one_solid",
    "test_writer.py::test_tolerances_without_any_dimension_keep_their_size",
    "test_resume.py::test_without_answers_nothing_is_stored",
    "test_load.py::test_face_indices_are_stable_across_processes",
    "test_general.py::test_listed_but_not_written",
    "test_choices.py::test_a_choice_answer_that_is_not_text_is_refused",
    "test_materials.py::test_a_long_material_name_is_written_and_verified",
    "test_load.py::test_binding_identifies_the_file",
    "test_load.py::test_an_assembly_holding_one_solid_is_numbered_on_the_solid",
}
_BORES = """

def _bores(face_ids, diameter, faces):
    # Ported: specify-core's rules._bores, the cylinder faces among face_ids at diameter.
    return tuple(
        i
        for i in face_ids
        if faces[i]["kind"] == "cylinder" and abs(2 * faces[i].get("radius", 0) - diameter) < 1e-6
    )
"""
_FACES = """

def describe_faces(loaded):
    # Ported: specify-core's features.describe_faces, which the analysis's faces are.
    from types import SimpleNamespace

    from specify_core.api import analyse

    return [SimpleNamespace(**f) for f in analyse(loaded)["faces"]]
"""
_SAVED = """

def saved(path, part=0):
    # Ported: specify-core's resume.saved, as far as the API says: None when no answers are
    # stored for the part (parts' "written").
    from specify_core.api import parts

    return True if parts(path)[part]["written"] else None
"""
PRELUDE = {"test_datums.py": _BORES, "test_same_as.py": _BORES, "test_mesh.py": _FACES,
           "test_writer.py": _FACES, "test_resume.py": _SAVED, "test_merge.py": _SAVED}
HAVE = {"specify_core.api": {"analyse", "questions", "parts", "apply", "preview", "load",
                              "write", "write_parts", "mesh"},
        "specify_core.existing": {"read_existing", "part_settings", "ExistingPmi"},
        "specify_core.writer": {"WriteReport", "VerificationError"},
        "specify_core.rules": {"questions", "ATTENTION", "LOCATING", "Question", "Intent",
                               "IncompleteError", "with_all_defaults"},
        "specify_core.load": {"load", "LoadedPart", "Binding"},
        "specify_core": {"api"}}
# Parts specify-core's tests build inline with build123d: the committed file of the same part
# (tools/make_parts.py), named after the test.
PATCHES = {
    "test_features.py": [(
        """    from build123d import Box, BuildPart, Cylinder, Locations, Mode, export_step

    with BuildPart() as bp:
        Box(100, 60, 10)
        with Locations(*locations):
            Cylinder(2.75, 10, mode=Mode.SUBTRACT)
    path = tmp_path / "plate.step"
    export_step(bp.part, str(path))
""",
        """    # The part specify-core builds here with build123d, committed by tools/make_parts.py
    # under the calling test's name (with the same holes at the same locations).
    import inspect

    from conftest import PARTS

    path = PARTS / f"{inspect.stack()[1].function.removeprefix('test_')}.step"
""")],
    "test_choices.py": [(
        """    with pytest.raises(SpecError, match=qid):""",
        """    # Ported: the engine's refusal is a ValueError, of which specify-core's SpecError is one.
    with pytest.raises(ValueError, match=qid):""")],
    "test_materials.py": [(
        """    longest = max((name for name, _, _ in MATERIALS), key=len)""",
        """    # Ported: the material question's suggestions are specify-core's MATERIALS.
    from specify_core.api import questions

    material = next(q for q in questions(analysis, {}) if q.id == "part.material")
    longest = max((s["value"] for s in material.suggestions), key=len)""")],
    "test_same_as.py": [("    from specify_core.rules import _bores\n\n", "")],
    "test_merge.py": [("    from specify_core.resume import saved\n\n", "")],
    "test_conformance.py": [
        (r'r"= MEASURE_WITH_UNIT\(', r'r"=\s*MEASURE_WITH_UNIT\('),
        (r'r"= CIRCULAR_RUNOUT_TOLERANCE\(', r'r"=\s*CIRCULAR_RUNOUT_TOLERANCE\('),
        (r'rf"= {kind}_TOLERANCE\(', r'rf"=\s*{kind}_TOLERANCE\('),
    ],
    "test_mesh.py": [(
        """    from build123d import Box, BuildPart, Compound, Cylinder, Location, Mode, export_step

    with BuildPart() as bp:
        Box(100, 60, 10)
        Cylinder(4, 10, mode=Mode.SUBTRACT)
    part = bp.part.moved(Location((100, 200, 300)))
    part.label = "plate"
    path = tmp_path / "placed.step"
    export_step(Compound(children=[part], label="assembly"), str(path))
""",
        """    # The file specify-core builds here with build123d, committed by tools/make_parts.py.
    from conftest import PARTS

    path = PARTS / "a_placed_part_is_meshed_in_the_frame_the_analysis_uses.step"
"""), (
        """@pytest.mark.parametrize("part", ["plate", "spool"])
def test_every_triangle_winds_outward(part, request):
    from OCP.BRepGProp import BRepGProp
    from OCP.GProp import GProp_GProps

    loaded = load(request.getfixturevalue(part))
    props = GProp_GProps()
    BRepGProp.VolumeProperties_s(loaded.shape, props)
    assert _volume(mesh(loaded)) == pytest.approx(props.Mass(), rel=0.01)
""",
        """@pytest.mark.parametrize("part", ["plate"])
def test_every_triangle_winds_outward(part, request):
    # Ported: the plate's exact volume (100 x 60 x 10, less six Ø6.6, one Ø8 and one Ø4.2
    # through holes) stands for OpenCascade's volume of the solid; the spool's is not known
    # here (PYTHON_ONLY.md).
    import math

    loaded = load(request.getfixturevalue(part))
    exact = 100 * 60 * 10 - 10 * math.pi * (6 * 3.3**2 + 4**2 + 2.1**2)
    assert _volume(mesh(loaded)) == pytest.approx(exact, rel=0.01)
""")],
    "test_load.py": [(
        """from OCP.BRepGProp import BRepGProp
from OCP.GProp import GProp_GProps
from specify_core.load import load
lp = load(sys.argv[1]); out = []
for i in range(lp.binding.face_count):
    p = GProp_GProps(); BRepGProp.SurfaceProperties_s(lp.face(i), p); c = p.CentreOfMass()
    out.append(f"{i}:{p.Mass():.6f}:{c.X():.5f},{c.Y():.5f},{c.Z():.5f}")
print(hashlib.sha256("|".join(out).encode()).hexdigest())""",
        """# Ported: each face's area and centroid from the analysis, not OpenCascade's.
from specify_core.api import analyse
out = [f"{f['id']}:{f['area']:.6f}:{f['centroid']}" for f in analyse(sys.argv[1])["faces"]]
print(hashlib.sha256("|".join(out).encode()).hexdigest())"""), (
        """    assert a.binding.face_count == a.faces.Extent()""",
        """    # Ported: the faces numbered are the analysis's, not OpenCascade's face map.
    from specify_core.api import analyse

    assert a.binding.face_count == len(analyse(a)["faces"])"""), (
        """    assert loaded.shape.ShapeType().name == "TopAbs_SOLID"
    assert all(loaded.index_of(loaded.face(i)) == i for i in range(7))""",
        """    # Ported: the analysis numbers the solid's seven faces (not OpenCascade's shape type).
    from specify_core.api import analyse

    assert len(analyse(loaded)["faces"]) == 7"""), (
        """    from build123d import Box, Compound, Location, export_step

    a, b = Box(1, 1, 1), Box(1, 1, 1).moved(Location((5, 0, 0)))
    path = tmp_path / "two.step"
    export_step(Compound(children=[a, b]), str(path))
""",
        """    # The file specify-core builds here with build123d, committed by tools/make_parts.py.
    from conftest import PARTS

    path = PARTS / "a_file_with_two_parts_must_say_which.step"
""")],
}
keep = {}
for k, v in cls.items():
    if k in EXCLUDE: continue
    u = set(v["uses"])
    if k in INCLUDE or (u and u <= OK and u & CORE):
        f, t = k.split("::"); keep.setdefault(f, []).append(t)
for fname, tests in keep.items():
    text = (src / fname).read_text(); lines = text.splitlines(keepends=True)
    tree = ast.parse(text)
    funcs = {n.name: n for n in tree.body if isinstance(n, (ast.FunctionDef, ast.ClassDef))}
    need, todo = set(), list(tests)
    while todo:
        n = todo.pop()
        if n in need or n not in funcs: continue
        need.add(n)
        fn = funcs[n]
        names = {x.id for x in ast.walk(fn) if isinstance(x, ast.Name)}
        if isinstance(fn, ast.FunctionDef): names |= {a.arg for a in fn.args.args}
        todo += [x for x in names if x in funcs]
    used = {x.id for n in need for x in ast.walk(funcs[n]) if isinstance(x, ast.Name)}
    for node in tree.body:  # constants the kept code uses, and what they use
        if isinstance(node, (ast.Assign, ast.AnnAssign)):
            targets = node.targets if isinstance(node, ast.Assign) else [node.target]
            if {x.id for t in targets for x in ast.walk(t) if isinstance(x, ast.Name)} & used:
                used |= {x.id for x in ast.walk(node) if isinstance(x, ast.Name)}
    out = []
    def seg(node):
        start = min([d.lineno for d in getattr(node, "decorator_list", [])] + [node.lineno]) - 1
        return "".join(lines[start:node.end_lineno])
    for node in tree.body:
        if isinstance(node, ast.Expr) and isinstance(node.value, ast.Constant) and node is tree.body[0]:
            out.append(seg(node))
        elif isinstance(node, ast.ImportFrom):
            mod = node.module or ""
            if mod.split(".")[0] in ("build123d", "quiddity", "OCP"): continue
            if mod.startswith("specify_core"):
                names = [a for a in node.names if a.name in HAVE.get(mod, set())]
                if not names: continue
                out.append(f"from {mod} import {', '.join(a.name + (f' as {a.asname}' if a.asname else '') for a in names)}\n")
            else:
                names = [a for a in node.names if (a.asname or a.name) in used]
                if names:
                    out.append(f"from {mod} import {', '.join(a.name + (f' as {a.asname}' if a.asname else '') for a in names)}\n")
        elif isinstance(node, ast.Import):
            if any((a.asname or a.name).split(".")[0] in used for a in node.names):
                out.append(seg(node))
        elif isinstance(node, (ast.Assign, ast.AnnAssign)):
            targets = node.targets if isinstance(node, ast.Assign) else [node.target]
            names = {x.id for t in targets for x in ast.walk(t) if isinstance(x, ast.Name)}
            if names & used:
                out.append(seg(node))
        elif isinstance(node, (ast.FunctionDef, ast.ClassDef)) and node.name in need:
            if fname in PRELUDE:
                out.append(PRELUDE.pop(fname))
            out.append("\n\n" + seg(node))
    body = "".join(x if x.endswith("\n") else x + "\n" for x in out)
    for old, new in PATCHES.get(fname, []):
        assert old in body, (fname, old)
        body = body.replace(old, new)
    (dest / fname).write_text(body)
    print(fname, len(tests))

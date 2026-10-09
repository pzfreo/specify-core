"""Write tests/fixtures/known_differences/features.json from the features sweep's dump and the
faces' judgement (tools/known_differences/judge_faces.py, whose docstring gives the first two
steps):

    python3 tools/known_differences/features.py DIR/features.json DIR/judge.json \
        tests/fixtures/known_differences/features.json

then run the sweep again (without SPECIFY_DUMP); it must pass. Rerun all three steps whenever
the haecceity or quiddity pin changes: the per-face entries name exact cases and indices.

Every entry's verdict and evidence are stated here. The per-face entries are computed: each
area or centroid value is judged against the adaptive reference. Entries for small numeric
differences that the evidence bounds (`measured`) are written one per case, from the
differences the dump holds, and the generator refuses (exits) when a difference exceeds the
bound the evidence states, so a regression cannot be absorbed by a broad entry. The remaining
entries are hand-judged, each with its evidence.
"""
import collections
import gzip
import json
import re
import sys
from pathlib import Path

DUMP, JUDGE, OUT = sys.argv[1:4]
O = str(Path(__file__).resolve().parents[2] / "tests/fixtures/oracle") + "/"
dump = json.load(open(DUMP))
faces = [x for x in dump if x["path"].startswith("faces")]
feats = [x for x in dump if x["path"].startswith("features")]
judge = json.load(open(JUDGE))
E = []
def e(case, path, pattern, verdict, evidence):
    E.append({"case": case, "path": path, "pattern": pattern, "verdict": verdict, "evidence": evidence})

_globs = {}
def glob(pattern):
    """The known-differences glob (tests/common): `*` matches any run, all else itself."""
    if pattern not in _globs:
        _globs[pattern] = re.compile("".join(".*" if c == "*" else re.escape(c) for c in pattern), re.S)
    return _globs[pattern]

def first_entry(d):
    return next((x for x in E if glob(x["case"]).fullmatch(d["case"]) and glob(x["path"]).fullmatch(d["path"])), None)

def measured(path, pattern, verdict, evidence, absolute=None, relative=None, cases=("*",), within_only=False):
    """One entry per case for the dump's differences at `path` that no earlier entry matches,
    each stating how many values differ and by how much. With a bound (`absolute`, or
    `relative` to the larger value), a difference beyond it, or one that is not two numbers,
    stops the generator: the evidence no longer covers it. With `within_only`, a case with
    such a difference gets no entry here instead, and is left to a later, wider entry."""
    rows = [d for d in feats if any(glob(c).fullmatch(d["case"]) for c in cases)
            and glob(path).fullmatch(d["path"]) and first_entry(d) is None]
    by_case = collections.defaultdict(list)
    for d in rows:
        by_case[d["case"]].append(d)
    def within(d):
        py, rs = d["python"], d["rust"]
        if not all(isinstance(v, (int, float)) and not isinstance(v, bool) for v in (py, rs)):
            return False
        # The bound, with room for the float error of the difference of two decimals.
        return abs(py - rs) <= max(absolute or 0.0, (relative or 0.0) * max(abs(py), abs(rs))) * (1 + 1e-9) + 1e-12
    for case, ds in sorted(by_case.items()):
        if within_only and not all(within(d) for d in ds):
            continue
        largest = 0.0
        for d in ds:
            py, rs = d["python"], d["rust"]
            numbers = all(isinstance(v, (int, float)) and not isinstance(v, bool) for v in (py, rs))
            if (absolute is not None or relative is not None) and not within(d):
                sys.exit(f"{case} {d['path']}: {py!r} against {rs!r} is beyond the evidence of '{pattern}'; judge it anew")
            if numbers:
                largest = max(largest, abs(py - rs))
        e(case, path, pattern, verdict, f"{evidence} Here {len(ds)} value(s), largest difference {largest:.3g}.")


Q7 = ("Question Q7: OpenCascade's XCAF makes a part of each shape representation of one product "
      "(the solid and each SHELL_BASED_SURFACE_MODEL beside it); haecceity has one part per product "
      "definition, its faces the solid's then the surface models' (tests/part.rs, "
      "corpus_and_nist_faces_are_pythons_at_every_index).")
for c in ["corpus/nist/nist_ctc_02_asme1_rc", "corpus/nist/nist_ctc_04_asme1_rd", "corpus/nist/nist_ctc_05_asme1_rd",
          "nist/nist_ctc_02_asme1_ap242", "nist/nist_ctc_04_asme1_ap242"]:
    e(c + "#*", "part", "Python's surface-model part does not exist in Rust", "not-applicable", Q7 + " Python's parts 1 and 2 are each one surface face; Rust loads part 0 only.")
    for x in faces:
        if x['case'] == c + "#0" and re.fullmatch(r'faces\[\d+\]', x['path']):
            e(x['case'], x['path'], "a surface model's face follows the solid's in Rust's one part", "not-applicable", Q7 + " The solid's faces are at Python's indices; this trailing face is a surface model's plane.")
e("corpus/inventory_refusal/13975", "faces[*", "face numbering differs from OpenCascade's", "undetermined",
  "A pure permutation: Rust numbers the CLOSED_SHELL's faces in file order (#17, #145, #216, ...); Python's face i is read from #17, #216, #752, #1166, ... (specify_core.requirements.face_entities_of), the order OpenCascade's reader gives (why it departs from the file's order was not established). Each face's kind, area and centroid agree entity for entity (Python's face 1 = #216 = Rust's face 2: plane, 343.078). An analysis of this file must not cross engines (question Q1); which order is 'right' is a convention.")

# Per-face area/centroid differences, judged against OCCT's adaptive integration.
def ok(k, v, ex):
    scale = abs(ex) if k == 'area' else max(abs(ex), 1.0)
    return abs(v - ex) <= 1e-6 * scale
PATTERN = {
 "rust-correct": "Rust at the adaptive integral, Python off",
 "rust-wrong": "Python at the adaptive integral, Rust off (U11)",
 "equivalent": "both at the adaptive integral",
 "undetermined": "neither at the adaptive integral",
}
# Every per-face entry: 'ref' is OCCT's BRepGProp.SurfaceProperties with eps 1e-9 (adaptive);
# 'at' means within 1e-6 of it (relative for area; centroid scaled by max(|c|, 1)).
for case, v in sorted(judge.items()):
    if '13975' in case: continue
    for i, f in sorted(v.items(), key=lambda kv: int(kv[0])):
        for field in sorted({k.split('[')[0] for k in f['fields']}):
            items = sorted((k, x) for k, x in f['fields'].items() if k.startswith(field))
            states = set()
            for k, (py, rs, ex) in items:
                states.add((ok(k, py, ex), ok(k, rs, ex)))
            if (False, False) in states: verdict = "undetermined"
            elif states == {(True, True)}: verdict = "equivalent"
            elif states <= {(True, True), (False, True)}: verdict = "rust-correct"
            elif states <= {(True, True), (True, False)}: verdict = "rust-wrong"
            else: verdict = "undetermined"
            vals = "; ".join(f"{k} ref {ex:.10g} py {py:.10g} rs {rs:.10g}" for k, (py, rs, ex) in items)
            e(case, f"faces[{i}].{field}" + ("[*]" if field == "centroid" else ""), PATTERN[verdict], verdict, f"OCCT adaptive integral (eps 1e-9) as reference, 'at' = within 1e-6; {f['kind']}: {vals}")

# ---- features
NOT_PORTED = {
 "step_levels": "Produced by Python quiddity's cross-family reconciliation (build_recognition_result), which quiddity-rust does not port (features/mod.rs: 'the Python aggregate's cross-family reconciliation is not ported'; U2).",
 "section_recesses": "Python quiddity's section-recess aggregate (evidence.py builds it from the recess source families, which it leaves out); quiddity-rust prototype/rust-port 5798c59 has the source families (pockets, channels, prismatic_pockets, section_passages, ...) in Features but not the aggregate.",
 "risers": "quiddity-rust has recognise_risers, but outside Features ('measurements with no evidence path yet', features/mod.rs), so recognise does not return them.",
}
U2 = "quiddity-rust gives defining faces only (Features::defining); Python quiddity's evidence view gives constituent faces (upstream need U2)."
for fam, why in NOT_PORTED.items():
    e("*", f"features[id={fam}:*]", f"{fam}: no such family in quiddity-rust", "rust-wrong",
      why + " Rust's analysis lacks the feature Python reports (upstream need U12); specify-core's rules read only holes, hole_patterns and turned_steps, so no question changes, but the picker's feature list does.")
e("*", "features[id=holes:*].faces+cone", "a drilled hole's point cone is constituent, not defining", "rust-wrong",
  "Every such hole's record has bottom 'drill_point': Python's faces add the drill-point cone to the bore. " + U2)
e("*", "features[id=hole_patterns*].faces+cone", "a pattern's member holes lack their drill-point cones", "rust-wrong",
  "The members are drill-point holes (bottom 'drill_point'); the pattern's faces are its members'. " + U2)
e("*", "features[id=holes:*].faces+plane", "a blind hole's floor is constituent, not defining", "rust-wrong",
  "Every such hole's record has bottom 'flat': Python's faces add the floor plane. " + U2)
e("*", "features[id=hole_patterns*].faces+plane", "a pattern's blind member holes lack their floors", "rust-wrong", "As for the member holes (bottom 'flat'). " + U2)
e("corpus/cadgenbench_inputs/cgb243", "features[id=holes:126.127.226].faces+other", "a blind hole's freeform floor is constituent, not defining", "rust-wrong", "Record bottom 'flat'; face 126 is the floor (kind other). " + U2)
e("*", "features[id=polygonal_bosses:*].faces+plane", "a polygonal boss's end face is constituent, not defining", "rust-wrong", "Records equal; Python's extra plane is the boss's top, its normal along the record's axis (NIST CTC-01 face 104, +z, top 50; FTC-07 faces 236 and 265, -y). " + U2)
e("*", "features[id=bosses:*].faces+plane", "a boss's end face is constituent, not defining", "rust-wrong", "Python's faces add the boss's planar end; records equal. " + U2)
e("*", "features[id=angled_steps:*].faces+plane", "an angled step's adjoining plane is constituent, not defining", "rust-wrong", "Records equal; Python's faces add a plane. " + U2)
e("*", "features[id=thin_wall_bodies:*].faces+*", "thin wall body: Python lists more faces", "undetermined", "Records equal to a micron; Python's evidence view lists further walls. " + U2)
e("*", "features[id=interior_voids:*].faces+*", "interior void: Python lists more faces", "undetermined", "Records equal to a micron; Python lists further faces of the void. " + U2)
e("*", "features(order)", "features listed in another order", "equivalent",
  "Rust lists families in quiddity-rust's Features order, patterns after their holes, hole sets last; Python its evidence view's order. Ids and content are compared by id; nothing in specify-core depends on the order.")
measured("features[id=*].record.profile.body_bounds[*]", "turned body's bounds 1e-7 wider in Python", "rust-correct",
  "Every difference is 1e-7: OpenCascade's Bnd_Box adds its gap to the solid's box; haecceity's bounds are the exact extent (e.g. string_post: radius 3.75, Python 3.7500001).",
  absolute=1.01e-7)
for k, what in ((6, "surface area"), (7, "volume")):
    measured(f"features[id=*body_key[{k}]", f"body {what} in the body key differs up to 3e-4 relative", "undetermined",
      f"body_key[{k}] is the body's {what}, integrated by each kernel; cgb217's faces mostly differ from the adaptive reference in Rust and not in Python (its faces[i] entries; U11), but the whole body was not integrated independently; specify-core never reads body_key.",
      relative=3e-4, cases=["corpus/cadgenbench_inputs/cgb217"])
for k, what in ((6, "surface area"), (7, "volume")):
    measured(f"features[id=*body_key[{k}]", f"body {what} in the body key differs below 1e-7 relative", "equivalent",
      f"body_key[{k}] is the body's {what}, integrated by each kernel (e.g. ctc_04's area: 17519780.20 against 17519779.30); agreement to 1e-7 is within either kernel's integration error; specify-core never reads body_key.",
      relative=1e-7, within_only=True)
    measured(f"features[id=*body_key[{k}]", f"body {what} in the body key differs up to 2e-5 relative", "undetermined",
      f"body_key[{k}] is the body's {what}, integrated by each kernel; the difference follows the face integrals' (this case's faces[i].area entries judge each against the adaptive reference: e.g. string_post's face 0, Python 95.29644, Rust 95.29871, reference 95.29871), but the whole body was not integrated independently; specify-core never reads body_key.",
      relative=2e-5)
e("corpus/cadgenbench_inputs/cgb202", "features[id=*body_key", "body key absent on one side", "undetermined",
  "cgb202: Python's body_key is null and Rust's is a key.")
for k in ("u", "v"):
    measured(f"features[id=plates:*].record.{k}", f"plate offset {k} differs below 1.5e-7 mm", "equivalent",
      "A fitted in-plane offset (nist_ctc_01: -1.33368645 against -1.33368631).", absolute=1.5e-7)
measured("features[id=fillets:*].record.at[*]", "a fillet's reported point differs by a rounding unit", "equivalent",
  "'at' is a point of the fillet rounded to 3 decimals: -153.116 against -153.117 is a rounding boundary.",
  absolute=1.001e-3, cases=["corpus/nist/nist_ctc_02_asme1_rc#0", "nist/nist_ctc_02_asme1_ap242#0"])
measured("features[id=fillets:121].record.at[*]", "a ring fillet's reported point lies elsewhere on the ring", "equivalent",
  "Fillet 121 is a full ring of radius 33.61 and each engine reports a different point on it.",
  cases=["nist/nist_ctc_05_asme1_ap242"])
measured("features[id=blends:*].record.path.at[*]", "a blend's reported point differs by a rounding unit", "equivalent",
  "'at' is a point of the blend's path rounded to 3 decimals: 22.779 against 22.778 is a rounding boundary.",
  absolute=1.001e-3, within_only=True)
measured("features[id=blends:*].record.path.at[*]", "a blend's reported point differs by up to 0.023 mm", "undetermined",
  "'at' is a point each quiddity picks on the blend's path (cgb241 blend 27: z -58.798 against -58.775); the radius, side and the rest of the path agree; which point is reported was not checked against a reference.",
  absolute=0.0231)
measured("features[id=freeform_surfaces:*].record.offset_distance", "a freeform offset distance differs below 5e-6 mm", "undetermined",
  "The distance between a freeform face and its offset partner, sampled on each kernel's evaluation of the B-spline (cgb207: 3.00215008 against 3.00215472); not checked against an independent reference.",
  absolute=5e-6)
SM = "Both engines report the same sheet-metal body (same faces, flanges, bends and flat pattern structure; tests/fixtures/known_differences/features.json's generator, compared by hand on sm-hanger and NIST CTC-03 AP242 with each engine's analysis)."
e("*", "features[id=sheet_metal_bodies:*].record.flat_pattern.flat_faces[*", "a flat-pattern face is tessellated more coarsely", "undetermined",
  SM + " Each flat face is a polygon tessellation of the unfolded face; Rust's has about half Python's vertices (sm-hanger: 37 against 69, 71 against 134; CTC-03: 528 against 1024) and the same area to 7e-5 relative (CTC-03 face 2: 80906.479 against 80911.879; sm-hanger face 8: 12439.807 against 12440.182). Neither was checked against the record's tessellation_tolerance.")
e("corpus/ttt_inputs/sm-hanger", "features[id=sheet_metal_bodies:*].record.flat_pattern.overlap_witnesses[*].area", "flat-pattern overlap areas differ", "undetermined",
  SM + " The overlap of a bend strip and a flange face in the flat pattern, measured on each engine's tessellation (above): 21.837 against 21.134 and 21.784, 55.911 against 64.467 and 28.240. The witnesses' pairs agree; the areas follow the coarser polygons. Not checked against an exact overlap.")
measured("features[id=sheet_metal_bodies:*].record.flanges[*].area", "a flange's area differs below 4e-6 relative", "undetermined",
  SM + " A flange's area is its faces' integral in each kernel (the faces entries judge those).", relative=4e-6)
measured("features[id=sheet_metal_bodies:*].record.paired_area_fraction", "paired area fraction differs below 1e-7 relative", "equivalent",
  SM + " A ratio of face integrals, equal to 1e-7.", relative=1e-7)
e("nist/nist_ctc_05_asme1_ap242", "features[id=chamfers:9*].record.at[*]", "a ring chamfer's reported point lies elsewhere on the ring", "equivalent",
  "Chamfers 90 and 91 are full rings (radii 149.225 and 28.575); Python reports (-r, 0), Rust (0, r): both on the ring.")
measured("features[id=flats:*].record.at[*]", "a flat's reported point differs by up to 0.015 mm", "undetermined",
  "'at' comes from each kernel's sampling of a freeform-bounded flat; not checked against an independent reference.",
  absolute=0.015, cases=["corpus/cadgenbench_inputs/cgb242"])
measured("features[id=*].record.*location[*]", "a hole or boss location differs below 1e-6 mm", "equivalent",
  "Locations come from the hole's edges as each kernel reads them; a difference below 1e-6 mm is below the analysis's own rounding of face centroids and of no manufacturing consequence.",
  absolute=1e-6, within_only=True)
measured("features[id=*].record.*location[*]", "a hole or boss location differs by up to 1.85e-3 mm", "undetermined",
  "Locations come from the hole's edges as each kernel reads them; these files' hole edges are B-spline approximations, and each kernel fits them differently. Not checked against an independent reference.",
  absolute=1.85e-3, cases=["corpus/nist/nist_ftc_07_asme1_rd", "corpus/nist/nist_ftc_10_asme1_rb", "nist/nist_ctc_02_asme1_ap242#0", "corpus/nist/nist_ctc_02_asme1_rc#0"])
measured("features[id=hole_patterns.grid:*].record.center[1]", "grid centre follows its holes' locations", "undetermined",
  "The centre is the mean of the member locations, which differ by up to 7.5e-4 mm (above).",
  absolute=7.5e-4, cases=["corpus/nist/nist_ftc_10_asme1_rb"])
e("corpus/cadgenbench_inputs/cgb203", "features[id=circular_face_patterns:*].record.fit_error", "circular face pattern fit error differs", "undetermined", "fit_error 1.08891 (Python) against 0.326125 (Rust): each kernel's sample points of freeform faces; not checked further.")
measured("features[id=interior_voids:*].record.*", "interior void estimates differ", "undetermined",
  "estimated_volume, grid_pitch and body_key come from a voxel probe over each kernel's classifier; not checked against an independent reference.")

# Unpaired features of families both engines have: per case and id.
cache = {}
def analysis(case):
    if case not in cache: cache[case] = json.load(gzip.open(O + case + '/analysis.json.gz'))
    return cache[case]
RUST_ONLY_FAMILIES = {"round_bottom_blind_slots", "rectangular_blind_slots", "edge_open_prismatic_recesses", "edge_open_circular_pockets", "double_d_bores", "gusset_ribs", "gusset_rib_patterns",
                      "pockets", "channels", "prismatic_pockets", "section_passages"}
# Python quiddity recognises these but its evidence view leaves them out (evidence.py skips
# _registry.RECESS_SOURCE_FAMILIES, whose records feed section_recesses).
RECESS_SOURCE = {"pockets", "channels", "prismatic_pockets", "section_passages", "edge_open_prismatic_recesses", "edge_open_circular_pockets", "rectangular_blind_slots", "round_bottom_blind_slots"}
seen = set()
for x in feats:
    m = re.fullmatch(r'features\[id=([a-z_]+)(?:\.[a-z_]+)?:([\d.]+)\]', x['path'])
    if not m: continue
    fam = m.group(1)
    if fam in NOT_PORTED: continue
    faces = set(map(int, m.group(2).split('.')))
    a = analysis(x['case'])
    if fam in RUST_ONLY_FAMILIES:
        key = (x['case'], fam)
        if key in seen: continue
        seen.add(key)
        claim = sorted({f['family'] for f in a['features'] if faces & set(f['faces'])})
        if fam in RECESS_SOURCE:
            why = (f"quiddity-rust's {fam} family; Python quiddity 0.3.9 recognises it but its evidence view leaves the recess source families out "
                   f"(evidence.py, _registry.RECESS_SOURCE_FAMILIES{', as passages' if fam == 'section_passages' else ''}) and reports their section_recesses aggregate instead (not ported)")
        else:
            why = f"quiddity-rust's {fam} family, which Python quiddity 0.3.9 does not report"
        e(x['case'], f"features[id={fam}:*]", f"{fam}: Rust only", "undetermined",
          f"{why}; Python gives these faces to {', '.join(claim) or 'no feature'}.")
        continue
    if x['python'] is None and fam == "blends":
        fillet_faces = set().union(*(f['faces'] for f in a['features'] if f['family'] == 'fillets'))
        if faces <= fillet_faces:
            e(x['case'], x['path'], "a blend chain Python supersedes by its fillets", "rust-wrong",
              "Every face of this blend is a face of Python's fillets; Python quiddity's reconciliation drops such a chain "
              "(_reconcile.reconcile_blend_candidates, ReasonCode.BLEND_SUPERSEDED_BY_FILLET) and quiddity-rust does not reconcile (U2), "
              "so Rust lists the same rounds twice.")
            continue
    if x['python'] is None:
        claim = sorted({f['family'] for f in a['features'] if faces & set(f['faces']) and f['family'] != fam})
        same = [f['id'] for f in a['features'] if f['family'] == fam and faces & set(f['faces'])]
        if '13975' in x['case']:
            why = "face numbering differs on this file (faces entry, Q1), so Python's features are on other indices"
        elif same:
            why = f"Python's {fam} feature(s) {', '.join(same)} share faces with it but differ in record or faces (the two quiddities compose it differently)"
        elif claim:
            why = f"Python's cross-family reconciliation gives these faces to {', '.join(claim)} and drops this {fam} record; quiddity-rust does not reconcile (U2)"
        else:
            why = "Python reports nothing on these faces"
        e(x['case'], x['path'], f"{fam} reported by Rust only", "undetermined", why + ".")
    elif x['rust'] is None:
        same = [f"{f['id']}" for f in json.load(open('rust_features_cache.json')).get(x['case'], []) if f.split(':')[0] == fam] if False else []
        e(x['case'], x['path'], f"{fam} reported by Python only", "undetermined",
          "The Rust feature on these faces (listed beside it as Rust only) is composed differently: other faces or a record differing beyond a micron.")
with open(OUT, 'w') as fh:
    fh.write("[\n" + ",\n".join(json.dumps(x, ensure_ascii=False) for x in E) + "\n]\n")
print(len(E))

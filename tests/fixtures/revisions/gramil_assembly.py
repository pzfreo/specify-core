# Two revisions of gramil's assembly (pzfreo/gramil 9171ef6, Open Community License v1), for the
# assembly diff. Run from a gramil checkout with its venv (build123d 0.10.0):
#   .venv/bin/python gramil_assembly.py OUTDIR
# then gzip -9 OUTDIR/*.step. The production build (process.prototype=False, as cnc_params() in
# scripts/build_production.py does): the prototype build's helical threads don't export.
# new: gramil's defaults. old: the shaft without its end chamfer and with a 4 deep mount tap, and
# the drive plate's mount hole Ø2.2. Read as a change: GRM-02 and GRM-04 change, nothing else.
import sys
from build123d import export_step
from gramil.assembly import build_assembly
from gramil.parameters import PurflingCutterParams

def params(old=False):
    p = PurflingCutterParams()
    p = p.model_copy(update={"process": p.process.model_copy(update={"prototype": False})})
    if old:
        p = p.model_copy(update={
            "shaft": p.shaft.model_copy(update={"end_chamfer": 0.0, "drive_plate_mount_depth": 4.0}),
            "drive_plate": p.drive_plate.model_copy(update={"mount_hole_diameter": 2.2}),
        })
    return p

out = sys.argv[1]
export_step(build_assembly(params(old=True)), f"{out}/gramil_assembly.old.step")
export_step(build_assembly(params()), f"{out}/gramil_assembly.step")

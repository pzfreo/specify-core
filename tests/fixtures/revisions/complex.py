# Revision pairs with hard changes, for Specify's diff. Each case: (name, before, after, what changed).
from build123d import *

def plate(holes=4, hole_d=6.0, pocket_depth=4.0, slot=False, chamfered=True, fillet_outer=False, counterbore=False,
          boss=True, split=False, pattern_r=30.0, pocket_w=30.0):
    with BuildPart() as p:
        if split:  # the same block, made of two halves: its faces split differently
            with Locations((-25, 0, 5)): Box(50, 80, 10)
            with Locations((25, 0, 5)): Box(50, 80, 10)
        else:
            with Locations((0, 0, 5)): Box(100, 80, 10)
        if fillet_outer:
            fillet(p.edges().filter_by(Axis.Z), radius=5)
        top = p.faces().sort_by(Axis.Z)[-1]
        with BuildSketch(Plane.XY.offset(10)):
            with PolarLocations(pattern_r, holes, start_angle=45):
                Circle(hole_d / 2)
        extrude(amount=-10, mode=Mode.SUBTRACT)
        if counterbore:
            with BuildSketch(Plane.XY.offset(10)):
                with PolarLocations(pattern_r, holes, start_angle=45):
                    Circle(hole_d)
            extrude(amount=-3, mode=Mode.SUBTRACT)
        with BuildSketch(Plane.XY.offset(10)):
            Rectangle(pocket_w, 14)
        extrude(amount=-pocket_depth, mode=Mode.SUBTRACT)
        if slot:
            with BuildSketch(Plane.XY.offset(10)):
                with Locations((38, 0)): SlotOverall(16, 6, rotation=90)
            extrude(amount=-10, mode=Mode.SUBTRACT)
        if boss:
            with BuildSketch(Plane.XY):
                with Locations((-40, 28)): Circle(5)
            extrude(amount=-6)
        if chamfered:
            chamfer_edges = p.edges().filter_by(Axis.X).group_by(Axis.Z)[-1]
            chamfer(chamfer_edges.filter_by(lambda e: abs(e.center().Y) > 39), length=1)
    return p.part

def baked(part, location):
    from OCP.BRepBuilderAPI import BRepBuilderAPI_Transform
    return Solid(BRepBuilderAPI_Transform(part.wrapped, location.wrapped.Transformation(), True).Shape())

base = plate()
cases = {
    # Many changes at once: pattern 4 → 6 and Ø6 → Ø8, pocket deeper and wider, a new slot, chamfers gone, a boss removed.
    "many": plate(holes=6, hole_d=8, pocket_depth=6, pocket_w=36, slot=True, chamfered=False, boss=False),
    # Every edge round the outline filleted: the side faces' neighbours all change.
    "fillet-all": plate(fillet_outer=True),
    # The whole part moved and turned, and one change besides (holes Ø6 → Ø7).
    # Its geometry moved, not just its placement in the file (a placement Specify reads past):
    # copied through the move, so the file's coordinates are the moved ones.
    "moved": baked(plate(hole_d=7), Rot(0, 0, 90) * Pos(15, -8, 3)),
    # Remodelled: the same shape, built another way (its faces split differently). Nothing changed.
    "remodelled": plate(split=True),
    # A feature that becomes another: through holes become counterbored.
    "counterbore": plate(counterbore=True),
    # The hole pattern on a larger circle, and one more hole: every hole moves.
    "pattern": plate(holes=5, pattern_r=24),
}
export_step(base, "base.step")
for name, part in cases.items():
    export_step(part, f"{name}.step")
    print(name, len(part.faces()), "faces (base", len(base.faces()), ")")

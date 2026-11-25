# Sheet metal or not

Made for `tests/process.rs` with build123d (25 November 2026), by the script below; no other source. Covered by
this repository's licence. `bent-bracket` is the only one Specify should suggest sheet metal for: the others are thin
parts a machinist would make (a flat plate, an L with no bend radius, trays, a bracket-shaped block) and tubes (`split-ring` could be rolled sheet; it is not recognised).

```python
from build123d import *
import sys
d=sys.argv[1]
def save(n,p): export_step(p,f'{d}/{n}.step')
# machined thin parts: no bend radius, or a shelled pocket, or a tray milled from solid
save('machined-flat-plate', Box(80,50,3)-Pos(20,10,0)*Cylinder(3,3)-Pos(-20,-10,0)*Cylinder(4,3))
save('machined-L-sharp', Box(60,40,3)+Pos(28.5,0,20)*Box(3,40,43))
b=Box(80,60,20); save('machined-tray-sharp', b-Pos(0,0,3)*Box(74,54,20))
b=fillet(Box(80,60,20).edges().filter_by(Axis.Z),6); p=Pos(0,0,3)*Box(74,54,20); save('machined-tray-round-corners', b-fillet(p.edges().filter_by(Axis.Z),3))
# machined to a constant wall with concentric inner and outer radii: the shape a bend has
def ell(r,t=3,l=40,w=40):
    with BuildPart() as bp:
        with BuildSketch(Plane.XZ):
            with BuildLine():
                Polyline((0,r+t+l),(0,0),(r+t+l,0)) if False else None
                l1=Line((0,r+t+l),(0,r+t)); a=RadiusArc((0,r+t),(r+t,0),-(r+t)); l2=Line((r+t,0),(r+t+l,0)); l3=Line((r+t+l,0),(r+t+l,t)); l4=Line((r+t+l,t),(r+t,t)); a2=RadiusArc((r+t,t),(t,r+t),r); l5=Line((t,r+t),(t,r+t+l)); l6=Line((t,r+t+l),(0,r+t+l))
            make_face()
        extrude(amount=w)
    return bp.part
save('bent-bracket', ell(3))
save('machined-like-bracket-thick-wall', ell(12,t=12,l=30))
save('round-tube', Cylinder(20,80)-Cylinder(18,80))
o=fillet(Box(40,40,100).edges().filter_by(Axis.Z),5); i=fillet(Box(34,34,100).edges().filter_by(Axis.Z),2); save('square-tube', o-i)
save('split-ring', Cylinder(20,30)-Cylinder(18,30)-Pos(20,0,0)*Box(10,4,30))
```

import pytest

from specify_core.api import mesh
from specify_core.features import describe_faces
from specify_core.load import load


@pytest.fixture(scope="module")
def plate_mesh(plate):
    return load(plate), mesh(plate)


def test_mesh_is_bound_to_the_file(plate_mesh):
    loaded, m = plate_mesh
    assert m["format"] == "pmi-assist-mesh"
    assert m["binding"] == loaded.binding.to_dict()


def test_every_face_owns_a_contiguous_run_of_triangles(plate_mesh):
    loaded, m = plate_mesh
    assert len(m["faces"]) == loaded.binding.face_count
    expected = 0
    for start, count in m["faces"]:
        assert start == expected and count > 0
        expected += count
    assert expected * 3 == len(m["indices"])
    assert max(m["indices"]) < len(m["positions"]) // 3


def test_triangles_face_outward(plate_mesh):
    """A face's triangles wind with its outward side, reversed faces included."""
    loaded, m = plate_mesh
    p, ix = m["positions"], m["indices"]
    for face in describe_faces(loaded):
        if face.kind != "plane":
            continue
        start, count = m["faces"][face.id]
        for t in range(start, start + count):
            a, b, c = (p[3 * ix[3 * t + k] : 3 * ix[3 * t + k] + 3] for k in range(3))
            u = [b[i] - a[i] for i in range(3)]
            v = [c[i] - a[i] for i in range(3)]
            n = (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])
            assert sum(n[i] * face.direction[i] for i in range(3)) > 0


def test_a_part_inside_an_assembly_is_meshed_in_its_own_numbering(wrapped):
    loaded = load(wrapped)
    m = mesh(loaded)
    assert len(m["faces"]) == loaded.binding.face_count == 7
    assert m["edges"] and all(len(line) % 3 == 0 for line in m["edges"])


def _volume(m):
    """Signed volume of the mesh: positive and near the solid's only if every
    triangle, on curved and reversed faces too, winds outward."""
    p, ix = m["positions"], m["indices"]
    total = 0.0
    for t in range(0, len(ix), 3):
        a, b, c = (p[3 * ix[t + k] : 3 * ix[t + k] + 3] for k in range(3))
        total += (
            a[0] * (b[1] * c[2] - b[2] * c[1])
            - a[1] * (b[0] * c[2] - b[2] * c[0])
            + a[2] * (b[0] * c[1] - b[1] * c[0])
        )
    return total / 6


def _area(m, face):
    p, ix = m["positions"], m["indices"]
    start, count = m["faces"][face]
    total = 0.0
    for t in range(start, start + count):
        a, b, c = (p[3 * ix[3 * t + k] : 3 * ix[3 * t + k] + 3] for k in range(3))
        u = [b[i] - a[i] for i in range(3)]
        v = [c[i] - a[i] for i in range(3)]
        n = (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])
        total += sum(x * x for x in n) ** 0.5 / 2
    return total


@pytest.mark.parametrize("part", ["plate", "spool"])
def test_every_triangle_winds_outward(part, request):
    from OCP.BRepGProp import BRepGProp
    from OCP.GProp import GProp_GProps

    loaded = load(request.getfixturevalue(part))
    props = GProp_GProps()
    BRepGProp.VolumeProperties_s(loaded.shape, props)
    assert _volume(mesh(loaded)) == pytest.approx(props.Mass(), rel=0.01)


def test_each_face_range_covers_that_face(plate):
    """The copy that is meshed must number faces as the analysis does: each
    range's triangles have the area of the face the analysis gives that index."""
    loaded = load(plate)
    m = mesh(loaded)
    for face in describe_faces(loaded):
        assert _area(m, face.id) == pytest.approx(face.area, rel=0.02)


def test_meshing_is_independent_of_what_ran_before(plate):
    """Quiddity tessellates the same faces during analysis; neither may change
    the other, in either order, and meshing twice gives the same mesh."""
    from specify_core.api import analyse

    fresh = mesh(load(plate))
    analysed = load(plate)
    before = analyse(analysed)
    assert mesh(analysed) == fresh
    assert mesh(analysed) == fresh
    meshed = load(plate)
    mesh(meshed)
    assert analyse(meshed) == before


def test_a_face_that_does_not_triangulate_is_reported(plate, monkeypatch):
    import specify_core.mesh as module

    real = module._triangulation
    calls = iter(range(10**6))
    monkeypatch.setattr(
        module, "_triangulation", lambda face, loc: None if next(calls) == 2 else real(face, loc)
    )
    m = module.mesh(load(plate))
    assert m["missing"] == [2]
    assert m["faces"][2][1] == 0


def test_seams_are_not_drawn_as_edges(wrapped):
    """The plate's 12 box edges and the bore's two circles; not the bore's seam."""
    assert len(mesh(load(wrapped))["edges"]) == 14


def test_a_placed_part_is_meshed_in_the_frame_the_analysis_uses(tmp_path):
    """A solid placed inside an assembly: the mesh's planes are the analysis's."""
    from build123d import Box, BuildPart, Compound, Cylinder, Location, Mode, export_step

    with BuildPart() as bp:
        Box(100, 60, 10)
        Cylinder(4, 10, mode=Mode.SUBTRACT)
    part = bp.part.moved(Location((100, 200, 300)))
    part.label = "plate"
    path = tmp_path / "placed.step"
    export_step(Compound(children=[part], label="assembly"), str(path))
    loaded = load(path)
    m = mesh(loaded)
    p = m["positions"]
    for face in describe_faces(loaded):
        if face.kind != "plane":
            continue
        start, count = m["faces"][face.id]
        for v in m["indices"][3 * start : 3 * (start + count)]:
            offset = [p[3 * v + i] - face.centroid[i] for i in range(3)]
            assert abs(sum(offset[i] * face.direction[i] for i in range(3))) < 1e-3

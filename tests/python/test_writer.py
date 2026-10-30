import pytest
from specify_core.api import analyse, apply, write
from specify_core.existing import read_existing
from specify_core.load import load
from specify_core.rules import Intent, questions


def describe_faces(loaded):
    # Ported: specify-core's features.describe_faces, which the analysis's faces are.
    from types import SimpleNamespace

    from specify_core.api import analyse

    return [SimpleNamespace(**f) for f in analyse(loaded)["faces"]]


@pytest.fixture(scope="module")
def spool_written(spool, tmp_path_factory):
    """Every kind of requirement v1 writes, including a g6 shaft fit."""
    analysis = analyse(spool)
    ids = [q.id for q in questions(analysis, {})]
    pattern = next(i for i in ids if i.startswith("hole.function:hole_patterns"))
    bore = next(i for i in ids if i.startswith("hole.function:holes"))
    turned = [i for i in ids if i.startswith("diameter.fit")]
    answers = {
        "part.material": "6082-T6",
        pattern: "fit:H7",
        bore: "fit:H7",
        turned[0]: "p6",
        turned[1]: "g6",
        "datum.A.flatness": "0.02",
    }
    intent = apply(analysis, answers, accept_defaults=True)
    out = tmp_path_factory.mktemp("write") / "spool-pmi.step"
    return intent, write(spool, intent, out), out


def test_write_verifies_and_reports(spool_written):
    intent, report, _ = spool_written
    # Datums, requirements, and the part's material, general tolerance and notes
    # (surface finish and edges at their defaults; no coating, no heat treatment).
    assert len(report.written) == len(intent.datums) + len(intent.requirements) + 4
    assert "surface finish Ra 3.2" in report.written
    assert "material 6082-T6" in report.written and report.not_written == []
    assert not report.warnings


def test_existing_pmi_is_read_back_onto_the_same_faces(spool_written):
    intent, _, out = spool_written
    found = read_existing(load(out))
    positions = [e for e in found if e.type == "Position"]
    assert {e.datums for e in positions} == {("A", "B")}
    assert {e.faces for e in positions} == {
        tuple(r["faces"]) for r in intent.requirements if r["kind"] == "position"
    }


def test_existing_pmi_suppresses_questions(spool_written, spool):
    """Re-analysing the enriched file must not ask again for sizes it states."""
    _, _, out = spool_written
    before = {q.id.split(":")[0] for q in questions(analyse(spool), {})}
    after = questions(analyse(out), {})
    assert "hole.function" in before
    assert not any(q.id.startswith("hole.function:holes") for q in after)


def test_intent_for_another_file_is_refused(spool_written, plate, tmp_path):
    intent, _, _ = spool_written
    with pytest.raises(ValueError, match="different file"):
        write(plate, intent, tmp_path / "x.step")


def test_unreferenced_datums_are_warned(plate, tmp_path):
    """Nothing to position proposes no datums; one added anyway is warned about."""
    analysis = analyse(plate)
    answers = {"part.material": "steel"}
    for q in questions(analysis, {}):
        if q.id.startswith("hole.function"):
            answers[q.id] = "general"
    report = write(plate, apply(analysis, answers, accept_defaults=True), tmp_path / "p.step")
    assert report.warnings == []
    top = max(
        (f for f in analysis["faces"] if f["kind"] == "plane"), key=lambda f: f["centroid"][2]
    )
    answers["datum.A"] = [top["id"]]
    report = write(plate, apply(analysis, answers, accept_defaults=True), tmp_path / "q.step")
    assert len(report.warnings) == 1


def _datum_and_position(loaded) -> Intent:
    faces = describe_faces(loaded)
    plane = max((f for f in faces if f.kind == "plane"), key=lambda f: f.area)
    bore = next(f for f in faces if f.kind == "cylinder")
    intent = Intent(loaded.binding.to_dict())
    intent.datums = [{"letter": "A", "faces": [plane.id]}]
    intent.requirements = [
        {
            "kind": "position",
            "feature": "bore",
            "faces": [bore.id],
            "tolerance": 0.1,
            "diametral": True,
            "datums": ["A"],
        }
    ]
    return intent


def test_pmi_is_written_on_an_assembly_holding_one_solid(wrapped, tmp_path):
    loaded = load(wrapped)
    report = write(loaded, _datum_and_position(loaded), tmp_path / "out.step")
    assert report.written == ["datum A", "position 0.1 on bore"]


def test_tolerances_without_any_dimension_keep_their_size(plate, tmp_path):
    """With no dimension in the document OCCT writes tolerances in bare metres;
    the value read back must still be 0.1 mm, not 100."""
    loaded = load(plate)
    out = tmp_path / "out.step"
    write(loaded, _datum_and_position(loaded), out)
    (position,) = [e for e in read_existing(load(out)) if e.type == "Position"]
    assert position.value == pytest.approx(0.1)


def _plate_intent(plate):
    return apply(analyse(plate), {"part.material": "steel"}, accept_defaults=True)


def test_intent_from_another_loader_version_is_refused(plate, tmp_path):
    intent = _plate_intent(plate)
    intent.binding = {**intent.binding, "loader_version": intent.binding["loader_version"] - 1}
    with pytest.raises(ValueError, match="loader version"):
        write(plate, intent, tmp_path / "out.step")
    assert not list(tmp_path.iterdir())


def test_the_output_keeps_its_permissions_and_its_symlink(plate, tmp_path):
    import os
    import stat

    target = tmp_path / "real.step"
    target.write_text("old")
    target.chmod(0o600)
    link = tmp_path / "link.step"
    link.symlink_to(target)
    report = write(plate, _plate_intent(plate), link)
    assert report.output == link and link.is_symlink()
    assert "DATUM(" in target.read_text()
    assert stat.S_IMODE(os.stat(target).st_mode) == 0o600
    assert sorted(p.name for p in tmp_path.iterdir()) == ["link.step", "real.step"]

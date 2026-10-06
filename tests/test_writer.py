import pytest

from specify_core.api import analyse, apply, write
from specify_core.existing import read_existing
from specify_core.features import describe_faces
from specify_core.load import load
from specify_core.rules import Intent, questions
from specify_core.writer import _file_limits


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


def test_limits_are_stated_correctly_in_the_file(spool_written):
    """OCCT's writer negates the lower deviation; the file must carry true limits,
    including a shaft fit wholly below nominal (g6) and one wholly above (p6)."""
    intent, _, out = spool_written
    limits = _file_limits(out.read_text())
    for req in intent.requirements:
        if req["kind"] == "size":
            assert (pytest.approx(req["lower"]), pytest.approx(req["upper"])) in limits
    assert any(lo < 0 and hi < 0 for lo, hi in limits)  # g6
    assert any(lo > 0 and hi > 0 for lo, hi in limits)  # p6


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


def test_a_failed_write_leaves_the_destination_and_the_source(plate, tmp_path, monkeypatch):
    import shutil

    from specify_core import writer

    source = tmp_path / "part.step"
    shutil.copy(plate, source)
    destination = tmp_path / "out.step"
    destination.write_text("what was there before")
    before = source.read_bytes()
    intent = _plate_intent(source)

    def broken(*_args, **_kwargs):
        raise writer.VerificationError("injected")

    monkeypatch.setattr(writer, "verify", broken)
    for target in (destination, source):
        with pytest.raises(writer.VerificationError, match="injected"):
            write(source, intent, target)
    assert destination.read_text() == "what was there before"
    assert source.read_bytes() == before
    assert sorted(p.name for p in tmp_path.iterdir()) == ["out.step", "part.step"]


def test_writing_over_the_source_is_safe(plate, tmp_path):
    import shutil

    source = tmp_path / "part.step"
    shutil.copy(plate, source)
    report = write(source, _plate_intent(source), source)
    assert report.output == source and source.read_text().count("DATUM(") >= 1
    assert [p.name for p in tmp_path.iterdir()] == ["part.step"]

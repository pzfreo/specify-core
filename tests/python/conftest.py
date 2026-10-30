"""specify-core's test fixtures (its tests/conftest.py at cc341dd), for the Rust-backed package.

specify-core builds its synthetic parts with build123d at run time; the Rust-backed package
has no build123d, so they are the parts tools/make_parts.py built from the same code and
committed under tests/fixtures/parts/, the files the oracle was captured from. The corpus
parts are Quiddity's, as in specify-core.
"""

import os
from pathlib import Path

import pytest

#: The committed synthetic parts (tools/make_parts.py).
PARTS = Path(__file__).resolve().parents[1] / "fixtures" / "parts"

#: Quiddity's STEP corpus, which is not redistributed here.
CORPUS = Path(os.environ.get("QUIDDITY_CORPUS", "/app/workspaces/pzfreo/quiddity/tests/corpus"))


def corpus_part(subdir: str, name: str) -> Path:
    path = CORPUS / subdir / name
    if not path.is_file():
        pytest.skip(f"corpus not available at {path}")
    return path


@pytest.fixture(scope="session")
def spool() -> Path:
    """A turned part: bolt circles, a main bore, turned diameters."""
    return corpus_part("cadgenbench", "flanged_spool_132.step")


@pytest.fixture(scope="session")
def ctc03() -> Path:
    """NIST PMI test case 3, geometry only: an inch part stored as an assembly."""
    return corpus_part("nist", "nist_ctc_03_asme1_rc.stp")


@pytest.fixture(scope="session")
def plate() -> Path:
    """A 100x60x10 plate: 2x3 grid of M6 clearance holes, one Ø8 bore, one M5 tap drill."""
    return PARTS / "plate.step"


@pytest.fixture(scope="session")
def wrapped() -> Path:
    """A plate with one bore, stored as an assembly holding the one solid, as
    the NIST test parts are."""
    return PARTS / "wrapped.step"


@pytest.fixture(scope="session")
def pin() -> Path:
    """A turned pin: Ø6 x 12 (an M6 thread), Ø10 x 20 (a knurl), and a blind
    Ø4.2 x 8 M5 tap drill in its top face."""
    return PARTS / "pin.step"


@pytest.fixture(scope="session")
def assembly() -> Path:
    """Two parts: a plate with M6 clearance holes and an M5 tap drill, and a pin
    with a blind M5 tap drill placed twice -- one part, two instances."""
    return PARTS / "assembly.step"


@pytest.fixture(scope="session")
def stack() -> Path:
    """A cover screwed to a base and pinned: the base has four M5 tap drills and
    an Ø8 bore, the cover four M5 clearance holes over them and an Ø8 hole, and
    a headed pin, Ø8 through both, has its Ø10 head on the cover."""
    return PARTS / "stack.step"


#: Tests that fail on the Rust engine because of a recorded difference: test -> the ledger
#: entry (tests/fixtures/known_differences/) that explains it, or, marked NO LEDGER ENTRY, the
#: documented decision that does. Each is a strict xfail, so a test that starts passing is
#: reported.
KNOWN = {
    "test_datums.py::test_datums_are_proposed_only_when_something_is_measured_from_them": "rules.json, "
    "'measurement datums are proposed before dimensions, including in ± mode' (draftwright-rust "
    "#122): datums are proposed when no feature needs a geometric tolerance",
    "test_datums.py::test_a_datum_given_or_removed_is_always_asked": "rules.json, 'measurement "
    "datums are proposed before dimensions, including in ± mode' (draftwright-rust #122): A, B "
    "and C keep their defaults when no feature needs a geometric tolerance",
    "test_locating_problems_extras.py::"
    "test_an_added_orientation_brings_datums_where_nothing_else_needs_them": "rules.json, "
    "'measurement datums are proposed before dimensions, including in ± mode' (draftwright-rust "
    "#122): datums are proposed before any orientation needs them",
    "test_locating_problems_extras.py::test_only_the_datums_added_orientations_cite_are_proposed": (
        "rules.json, 'measurement datums are proposed before dimensions, including in ± mode' "
        "(draftwright-rust #122): in ± mode A, B and C are proposed, not only the cited B"
    ),
    "test_locating_problems_extras.py::test_holes_located_by_plus_minus_need_no_positions_or_datums": (
        "rules.json, 'measurement datums are proposed before dimensions, including in ± mode' "
        "(draftwright-rust #122): in ± mode A, B and C are still proposed"
    ),
    "test_preview.py::test_nothing_answered_shows_the_defaults": "rules.json, 'unanswered "
    "defaults remain suggestions until explicitly accepted' (specify-core-rust #20): preview "
    "holds only the answers given, so nothing answered previews no datums or requirements",
    "test_preview.py::test_an_answer_is_shown_as_decided": "rules.json, 'unanswered defaults "
    "remain suggestions until explicitly accepted' (specify-core-rust #20): an answered position "
    "waits for its datum frame, which is unanswered",
    'test_assembly.py::test_both_parts_are_written_and_verified': (
        "features.json, 'a blind hole's floor is constituent, not defining' (rust-wrong): the pin's tap drill is holes:3, not holes:3.5"
    ),
    'test_assembly.py::test_writing_one_part_again_alone_is_refused': (
        'NO LEDGER ENTRY: decision D3 (docs/design.md): one part of an assembly may be written alone, the others left as they are'
    ),
    'test_assembly.py::test_an_assembly_with_pmi_of_its_own_is_refused': (
        'NO LEDGER ENTRY: question Q6 (docs/questions.md, docs/python.md): an assembly with PMI from elsewhere is written in add mode'
    ),
    'test_conformance.py::test_callout_presentation_is_named': (
        "writer.json, 'an assertion on Python's text route or drawing, not ported': Python's own text route or drawing, which haecceity does not write"
    ),
    'test_drawn.py::test_a_note_is_its_leader_and_no_words': (
        "writer.json, 'an assertion on Python's text route or drawing, not ported': Python's own text route or drawing, which haecceity does not write"
    ),
    'test_requirements.py::test_the_text_route_and_its_callouts': (
        "writer.json, 'an assertion on Python's text route or drawing, not ported': Python's own text route or drawing, which haecceity does not write"
    ),
    'test_threads.py::test_a_typed_thread_depth_is_written_and_read_back': (
        "writer.json, 'an assertion on Python's text route or drawing, not ported': Python's own text route or drawing, which haecceity does not write"
    ),
    'test_requirements.py::test_the_standard_constructs': (
        "cross_engine.json, 'the part's default finish, written as a surface texture' and 'a thread written as AP242's thread feature' (Q3): notes, finish and threads are written in AP242's standard forms, not Python's text constructs"
    ),
    'test_requirements.py::test_a_finish_on_faces_is_not_a_second_default': (
        "cross_engine.json, 'the part's default finish, written as a surface texture' and 'a thread written as AP242's thread feature' (Q3): notes, finish and threads are written in AP242's standard forms, not Python's text constructs"
    ),
    'test_requirements.py::test_a_finish_written_in_the_old_form_still_reads_back': (
        "cross_engine.json, 'the part's default finish, written as a surface texture' and 'a thread written as AP242's thread feature' (Q3): notes, finish and threads are written in AP242's standard forms, not Python's text constructs"
    ),
    'test_locations.py::test_positioned_holes_get_basic_dimensions_from_their_datum_planes': (
        "writer.json, 'the report counts the dimensions written under the general tolerance' (#63): five more locations, nominal only; and, with NO LEDGER ENTRY, haecceity writes a 'theoretical' note per basic dimension where Python shares one"
    ),
    'test_merge.py::test_what_the_file_states_is_not_asked_again': (
        "rules.json, 'ordered free-text part notes are model PMI' (draftwright-rust #123): part.notes is asked"
    ),
    'test_merge.py::test_a_nist_part_keeps_all_its_pmi': (
        "rules.json, 'ordered free-text part notes are model PMI' (draftwright-rust #123): part.notes is asked"
    ),
    'test_profile_runout.py::test_a_runout_is_written_about_the_frame_and_read_back': (
        "rules.json, 'axis-first turned datum defaults and their dependent questions and intent': axial holes and runout use A alone"
    ),
    'test_writer.py::test_existing_pmi_is_read_back_onto_the_same_faces': (
        "rules.json, 'axis-first turned datum defaults and their dependent questions and intent': axial holes and runout use A alone"
    ),
    'test_writer.py::test_write_verifies_and_reports': (
        "writer.json, 'the report counts the dimensions written under the general tolerance' (#63): one more report line"
    ),
    'test_writer.py::test_unreferenced_datums_are_warned': (
        "rules.json, 'measurement datums are proposed before dimensions, including in ± mode' (draftwright-rust #122): A, B and C are written and warned about with nothing to position"
    ),
}


def pytest_collection_modifyitems(items):
    try:  # the Rust-backed package; specify-core's own engine runs these tests unmarked
        import specify_core._native  # noqa: F401
    except ImportError:
        return
    for item in items:
        name = f"{item.path.name}::{item.originalname}"
        if name in KNOWN:
            item.add_marker(pytest.mark.xfail(reason=KNOWN[name], strict=True))

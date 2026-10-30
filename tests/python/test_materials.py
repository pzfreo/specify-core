"""The material question offers common materials as it is typed."""
from specify_core.api import analyse, apply, questions


def test_the_material_question_suggests_common_materials(plate):
    analysis = analyse(plate)
    (material,) = [q.to_dict() for q in questions(analysis, {}) if q.id == "part.material"]
    assert material["kind"] == "text"
    found = {s["value"]: s for s in material["suggestions"]}
    assert found["Brass CZ121 (CW614N)"]["group"] == "Brass and copper"
    assert "Delrin" in found["POM-C acetal copolymer"]["also"]
    # A suggestion, not a whitelist: anything typed is the material.
    intent = apply(analysis, {"part.material": "Unobtainium 42"}, accept_defaults=True)
    assert intent.part["material"] == "Unobtainium 42"


def test_a_long_material_name_is_written_and_verified(plate, tmp_path):
    """OCCT puts a long name on a line of its own; the read-back must still find it."""
    from specify_core.api import write

    analysis = analyse(plate)
    # Ported: the material question's suggestions are specify-core's MATERIALS.
    from specify_core.api import questions

    material = next(q for q in questions(analysis, {}) if q.id == "part.material")
    longest = max((s["value"] for s in material.suggestions), key=len)
    for name in ("Stainless steel 2205 duplex (1.4462)", longest):
        intent = apply(analysis, {"part.material": name}, accept_defaults=True)
        write(plate, intent, tmp_path / "long.step")  # raises if the material is not read back

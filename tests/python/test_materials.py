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

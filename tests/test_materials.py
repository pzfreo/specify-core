"""The material question offers common materials as it is typed."""

from specify_core.api import analyse, apply, questions
from specify_core.materials import MATERIALS


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


def test_each_material_is_named_once():
    names = [name for name, _, _ in MATERIALS]
    assert len(names) == len(set(names))
    assert all(family and name for name, family, _ in MATERIALS)


def test_printed_materials_name_their_process_and_metals_carry_uns_numbers():
    by_name = {name: (family, also) for name, family, also in MATERIALS}
    printed = [name for name, (family, _) in by_name.items() if family.startswith("3D printing")]
    assert {"PA12 (MJF)", "PLA (FDM)", "Standard resin (SLA)", "AlSi10Mg (DMLS)"} <= set(printed)
    assert all(name.endswith(")") for name in printed)  # the process, in brackets
    assert "S30400" in by_name["Stainless steel 304 (1.4301)"][1]
    assert "C38500" in by_name["Brass CZ121 (CW614N)"][1]

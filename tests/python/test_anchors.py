"""Where labels land on the part, and which way their leaders leave it."""
from specify_core.api import analyse
from specify_core.load import load
from specify_core.serve import reply


def key(faces):
    # Ported: specify-core's anchors.key, the key a set of faces' anchor is found by.
    return ",".join(str(int(i)) for i in sorted(faces))


def test_preview_carries_anchors_for_its_labels_when_given_the_file(pin):
    analysis = analyse(pin)
    request = {"id": 1, "op": "preview", "analysis": analysis, "answers": {}}
    assert "anchors" not in reply(request)["result"]["intent"]
    intent = reply({**request, "step": str(pin)})["result"]["intent"]
    labelled = {key(r["faces"]) for r in intent["requirements"] + intent["datums"] if r["faces"]}
    assert labelled and labelled <= set(intent["anchors"])
    assert all(len(a["at"]) == 3 and len(a["out"]) == 3 for a in intent["anchors"].values())


def test_anchors_are_not_given_for_a_different_file(pin, plate):
    analysis = analyse(pin)
    result = reply(
        {"id": 1, "op": "preview", "analysis": analysis, "answers": {}, "step": str(plate)}
    )
    assert result["result"]["intent"]["anchors"] == {}

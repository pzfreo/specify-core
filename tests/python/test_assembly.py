"""An assembly of several parts, specified a part at a time and written together."""
import base64
import json
import re
import pytest
from specify_core import api
PLATE, PIN = 0, 1
ANSWERS = {PLATE: {"part.material": "steel"}, PIN: {"part.material": "brass"}}


def test_each_part_is_listed_once_with_where_its_instances_are(assembly):
    plate, pin = api.parts(assembly)
    assert (plate["name"], plate["faces"], plate["instances"]) == ("plate", 13, 1)
    assert (pin["name"], pin["faces"], pin["instances"]) == ("pin", 7, 2)
    assert [[row[3] for row in p] for p in pin["placements"]] == [[-30, 0, 5], [30, 0, 5]]
    assert not plate["written"] and not pin["written"]

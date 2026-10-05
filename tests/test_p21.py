"""Files written as Part 21's second edition, which they declare: ASCII only."""

from specify_core.api import analyse, apply, write
from specify_core.existing import part_settings
from specify_core.load import load
from specify_core.p21 import escape, unescape


def test_non_ascii_is_escaped_and_read_back():
    assert escape("Ra 3.2 µm") == "Ra 3.2 \\X2\\00B5\\X0\\m"
    assert escape("Güte – A") == "G\\X2\\00FC\\X0\\te \\X2\\2013\\X0\\ A"
    assert escape("plain") == "plain"
    for text in ("Ra 3.2 µm", "Güte – A", "😀 smile", "plain"):
        assert unescape(escape(text)) == text


def test_a_written_file_is_ascii_and_reads_back_as_given(pin, tmp_path):
    analysis = analyse(pin)
    answers = {
        "part.material": "Messing Güte – CZ121",
        "part.heat_treatment": "Harden and temper to 40–45 HRC",
        "part.coating": "Anodise noir (Type II) — café",
    }
    out = tmp_path / "escaped.step"
    write(pin, apply(analysis, answers, accept_defaults=True), out)  # verifies too
    raw = out.read_bytes()
    assert all(b < 0x80 for b in raw), "a non-ASCII byte in the file"
    assert b"'2;1'" in raw[:1000]  # the edition it declares
    settings = part_settings(load(out))
    assert settings["material"] == answers["part.material"]
    assert settings["heat_treatment"] == answers["part.heat_treatment"]
    assert settings["coating"] == answers["part.coating"]
    assert settings["surface_finish"] == "Ra 3.2"

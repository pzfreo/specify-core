"""Files written as Part 21's second edition, which they declare: ASCII only."""
from specify_core.api import analyse, apply, write
from specify_core.existing import part_settings
from specify_core.load import load


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

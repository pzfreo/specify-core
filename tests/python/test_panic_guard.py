"""A panic in the native extension is a SpecifyError (an Exception), not pyo3's PanicException."""

import pytest

import specify_pmi
from specify_pmi import _native


def test_panic_is_specify_error():
    with pytest.raises(specify_pmi.SpecifyError, match="boom") as raised:
        _native._panic_for_test("boom")  # test hook: panics inside a guarded call
    assert isinstance(raised.value, Exception)
    try:
        _native._panic_for_test("again")
    except Exception as e:  # what callers write; PanicException would escape this
        assert isinstance(e, specify_pmi.SpecifyError)

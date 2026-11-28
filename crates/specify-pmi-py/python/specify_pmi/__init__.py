"""specify-pmi: interview-driven PMI authoring."""


class SpecifyError(Exception):
    """An internal error of the engine (a panic in the native extension), raised as an
    ordinary `Exception` rather than pyo3's `PanicException`, which `except Exception` misses."""


from .api import bom, diff, pmi_scene  # noqa: E402

__all__ = ["SpecifyError", "bom", "diff", "pmi_scene"]

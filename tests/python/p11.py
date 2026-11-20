"""ADR P11: a file stores only the answers that leave no PMI behind (Rust's
`feature_table::storable`): "none" answers, and choices that are not requirements."""

NOT_REQUIREMENTS = {"part.locating"}


def leaves_nothing(value):
    if value is None:
        return True
    if isinstance(value, str):
        text = value.strip()
        return not text or text.lower().startswith("none")
    if isinstance(value, list):
        return not value
    return False


def is_requirement(key):
    return not (key.startswith("_") or key.startswith("bom.") or key in NOT_REQUIREMENTS)


def storable(answers):
    return {k: v for k, v in answers.items() if not is_requirement(k) or leaves_nothing(v)}


def stated(analysis):
    """The feature-and-kinds the analysis's feature table says the file states."""
    return {r["id"] for r in analysis["table"]["rows"] if r["state"] == "stated"}

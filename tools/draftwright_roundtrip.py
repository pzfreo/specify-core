"""Show what Draftwright reads from a specify-core output file.

Draftwright pins an older Quiddity than specify-core needs, so this
runs in an environment with Draftwright installed:

    uv run --frozen python /path/to/specify-core/tools/draftwright_roundtrip.py out.step
"""

import sys

from draftwright import extract_pmi

for record in extract_pmi(sys.argv[1]):
    d = vars(record)
    print(
        f"{d.get('kind'):<10} value={d.get('value')!s:<9} "
        f"upper={d.get('upper_tol')!s:<7} lower={d.get('lower_tol')!s:<7} "
        f"label={d.get('label')!r:<22} datums={d.get('datum_refs') or ''} "
        f"blockers={d.get('lowering_blockers') or ''}"
    )

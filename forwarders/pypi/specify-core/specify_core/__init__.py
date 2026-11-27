"""specify-core is now specify-pmi: this module forwards to specify_pmi."""

import warnings

warnings.warn(
    "specify-core is now specify-pmi; import specify_pmi instead. This package will not be updated.",
    DeprecationWarning,
    stacklevel=2,
)

from specify_pmi import *  # noqa: E402,F401,F403
from specify_pmi import __all__  # noqa: E402,F401

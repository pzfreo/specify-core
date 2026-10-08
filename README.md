# specify-core-rust

A pure-Rust port of [specify-core](https://github.com/pzfreo/specify-core): interview-driven
AP242 PMI authoring for STEP parts. No OpenCascade and no Python: features come from
[quiddity-rust](https://github.com/pzfreo/quiddity-rust)'s recognisers, and STEP reading,
meshing and PMI writing from its haecceity kernel.

**Status: starting.** Each part of specify-core is ported against what the Python
implementation produces, with every difference recorded and given a verdict. The plan is in
[docs/design.md](docs/design.md); what is needed from quiddity-rust in
[docs/upstream-needs.md](docs/upstream-needs.md); open decisions in
[docs/questions.md](docs/questions.md). Until the command line is ported, every command is
refused.

AGPL-3.0-or-later; see LICENSE and NOTICE.

# specify-core-rust

A pure-Rust port of [specify-core](https://github.com/pzfreo/specify-core): interview-driven
AP242 PMI authoring for STEP parts. No OpenCascade and no Python: features come from
[quiddity-rust](https://github.com/pzfreo/quiddity-rust)'s recognisers, and STEP reading,
meshing and PMI writing from its haecceity kernel.

**Status: stage 1 of 5.** The oracle (Python-captured fixtures and a comparison harness), the
part model on haecceity and the standards, materials and choices tables are ported; features,
rules, writing, the API and the command line are not yet. Each part of specify-core is ported
against what the Python implementation produces, with every difference recorded and given a
verdict. The plan is in [docs/design.md](docs/design.md); what is needed from quiddity-rust in
[docs/upstream-needs.md](docs/upstream-needs.md); open decisions in
[docs/questions.md](docs/questions.md). Until the command line is ported, every command is
refused.

AGPL-3.0-or-later; see LICENSE and NOTICE.

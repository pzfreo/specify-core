//! Specify's corpus selection. File stems (all parts of an assembly are selected).
//!
//! Start with every corpus input named by a behavioural test (including CORPUS_SAMPLE,
//! schema6's material-side regressions and the corpus-backed scenarios). Add 10092 for
//! pads and the alpha.1 semicircular-blind-slot family; add 13975 for inventory refusal.
//! Together the captures and feature ledger cover every recorded family, and each ledger
//! area with corpus entries has a selected case. oracle_harness checks that coverage.
//! Keep the complete oracle and ledgers: SPECIFY_CORPUS_ALL=1 opts into all inputs locally.

pub const SUBSET: &[&str] = &[
    "cadgenbench/flanged_spool_132",
    "cadgenbench/threaded_connector_109",
    "cadgenbench_inputs/cgb202",
    "cadgenbench_inputs/cgb203",
    "cadgenbench_inputs/cgb217",
    "cadgenbench_inputs/cgb241",
    "cadgenbench_inputs/cgb243",
    "gramel/GRM-03_thumbwheel_drive_screw",
    "gramel/GRM-05_depth_lock_bolt",
    "gramel/string_post",
    "inventory_refusal/13975",
    "mfcadpp/1000",
    "mfcadpp/10033",
    "mfcadpp/1004",
    "mfcadpp/10047",
    "mfcadpp/1007",
    "mfcadpp/10092",
    "mfcadpp/10103",
    "mfcadpp/10127",
    "mfcadpp/10156",
    "mfcadpp/10163",
    "mfcadpp/10224",
    "mfcadpp/10252",
    "mfcadpp_holdout/118",
    "mfcadpp_holdout/18016",
    "mfcadpp_holdout/275",
    "mfcadpp_holdout/363",
    "mfcadpp_holdout/408",
    "mfcadpp_holdout/467",
    "nist/nist_ctc_01_asme1_rd",
    "nist/nist_ctc_02_asme1_rc",
    "nist/nist_ctc_03_asme1_rc",
    "nist/nist_ctc_04_asme1_rd",
    "nist/nist_ctc_05_asme1_rd",
    "nist/nist_ftc_06_asme1_rd",
    "nist/nist_ftc_07_asme1_rd",
    "nist/nist_ftc_08_asme1_rc",
    "nist/nist_ftc_09_asme1_rd",
    "nist/nist_ftc_10_asme1_rb",
    "ttt_inputs/sm-hanger",
];

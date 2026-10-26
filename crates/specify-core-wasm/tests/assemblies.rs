//! The browser session on assemblies (specify-core-rust #2): a two-part assembly (`parts/
//! assembly.step`: a plate, and a pin placed twice) and a file of one part placed twice. The
//! session lists the parts as `specify-core parts` does, opens a chosen one, analyses it as the
//! command line's `--part` does and writes it into the original file as `api::write_bytes`
//! does, the other parts and placements kept.

use std::path::{Path, PathBuf};

use haecceity::p21::Document;
use haecceity::pmi::{self, PartPmi};
use haecceity::step::read_part_definitions;
use serde_json::{Map, Value, json};
use specify_core::api;
use specify_core::part::File;
use specify_core::resume;
use specify_core_wasm::session::Session;

const PLATE: usize = 0;
const PIN: usize = 1;
const PROJECT: &str = "draftwright editor project";

fn fixture(stem: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/parts")
        .join(format!("{stem}.step"))
}

fn assembly() -> Vec<u8> {
    std::fs::read(fixture("assembly")).unwrap()
}

/// `wrapped.step`'s plate with a second occurrence: one distinct part placed twice (the
/// same edit as `scripts/wasm-parity.mjs`'s one-placement refusal).
fn twice() -> Vec<u8> {
    let wrapped = std::fs::read_to_string(fixture("wrapped")).unwrap();
    assert_eq!(wrapped.matches("ENDSEC;\nEND-ISO").count(), 1);
    wrapped
        .replace(
            "ENDSEC;\nEND-ISO",
            "#500 = CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#501,#503);
#501 = ( REPRESENTATION_RELATIONSHIP('','',#32,#10)
REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#454)
SHAPE_REPRESENTATION_RELATIONSHIP() );
#503 = PRODUCT_DEFINITION_SHAPE('Placement','Placement of an item',#504);
#504 = NEXT_ASSEMBLY_USAGE_OCCURRENCE('2','plate','',#5,#27,$);
ENDSEC;\nEND-ISO",
        )
        .into_bytes()
}

fn answers(k: usize) -> Map<String, Value> {
    let material = if k == PLATE { "steel" } else { "brass" };
    json!({ "part.material": material })
        .as_object()
        .cloned()
        .unwrap()
}

fn written(out: &Value) -> Vec<u8> {
    serde_json::from_value(out["bytes"].clone()).unwrap()
}

fn pmi_of(bytes: &[u8]) -> Vec<PartPmi> {
    let defs = read_part_definitions(bytes).unwrap();
    let doc = Document::parse(bytes.to_vec()).unwrap();
    pmi::read(&doc, &defs).unwrap().parts
}

/// What the command line's writer gives for part `k` of `bytes` with these answers: the same
/// preview intent, written by `api::write_bytes` (the in-memory form of `write --part`).
fn cli_write(bytes: &[u8], k: usize, given: &Map<String, Value>) -> Vec<u8> {
    let file = File::of_bytes(Path::new("x.step"), bytes.to_vec()).unwrap();
    let analysis = api::analyse(&file.load(Some(k)).unwrap()).unwrap();
    let intent = api::preview(&analysis, given).unwrap();
    api::write_bytes(bytes.to_vec(), &[(intent, Some(given.clone()))])
        .unwrap()
        .0
}

/// Every instance of `before` is in `after` under the same number with the same text: the
/// part written was bare in `before`, so nothing of `before` was replaced, and every other
/// part, placement and earlier write is kept byte for byte (only the header's schema changes).
fn assert_kept(before: &[u8], after: &[u8]) {
    let (old, new) = (
        Document::parse(before.to_vec()).unwrap(),
        Document::parse(after.to_vec()).unwrap(),
    );
    for id in old.ids() {
        let text = |doc: &Document, bytes: &[u8]| doc.span(id).map(|span| bytes[span].to_vec());
        assert_eq!(
            text(&old, before),
            text(&new, after),
            "#{id} is not kept as it was"
        );
    }
}

#[test]
fn an_assembly_is_listed_and_refused_without_a_part_chosen() {
    let bytes = assembly();
    let listed = Session::parts("assembly.step", &bytes).unwrap();
    assert_eq!(listed, json!(api::parts(&fixture("assembly")).unwrap()));
    let got: Vec<(u64, &str, u64)> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["part"].as_u64().unwrap(),
                p["name"].as_str().unwrap(),
                p["instances"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(got, [(0, "plate", 1), (1, "pin", 2)]);

    let Err(refused) = Session::open("assembly.step", &bytes) else {
        panic!("an assembly opens with no part chosen");
    };
    assert!(refused.contains("one distinct part; found 2"), "{refused}");
    assert!(refused.contains("openPart"), "{refused}");
    let Err(refused) = Session::open_part("assembly.step", &bytes, 2) else {
        panic!("part 2 of two opens");
    };
    assert_eq!(refused, "assembly.step has 2 part(s); there is no part 2");
    assert!(Session::parts("a.step", &[0x1f, 0x8b, 0]).is_err());
}

/// Each part of the assembly opens as the command line loads it (`--part K`): the same
/// analysis (its mates with the other part, from these bytes) and mesh, its provenance naming
/// the part.
#[test]
fn each_part_of_an_assembly_is_analysed_as_the_command_line_does() {
    let bytes = assembly();
    for k in [PLATE, PIN] {
        let session = Session::open_part("assembly.step", &bytes, k).unwrap();
        let snapshot = session.snapshot();
        assert_eq!(
            snapshot["analysis"],
            api::analyse_file(&fixture("assembly"), Some(k)).unwrap(),
            "part {k}"
        );
        assert_eq!(
            snapshot["mesh"],
            api::mesh_file(&fixture("assembly"), Some(k)).unwrap()
        );
        assert_eq!(snapshot["provenance"]["part"], json!(k));
        assert_eq!(snapshot["analysis"]["binding"]["part"], json!(k));
        assert!(session.review(&answers(k)).is_ok());
    }
}

/// The pin written alone, then the plate on the result, each through its own session: the
/// bytes are the command line writer's, every instance of the file before is kept (the other
/// part, both of the pin's placements), and each part resumes its own answers.
#[test]
fn a_part_of_an_assembly_is_written_into_the_original_file() {
    let original = assembly();
    let pin = Session::open_part("assembly.step", &original, PIN).unwrap();
    let out = pin.write_pmi(&answers(PIN), None, None).unwrap();
    let after_pin = written(&out);
    assert_eq!(after_pin, cli_write(&original, PIN, &answers(PIN)));
    assert_eq!(
        out["source_sha256"],
        pin.snapshot()["provenance"]["source_sha256"]
    );
    assert!(!out["report"]["written"].as_array().unwrap().is_empty());
    assert_kept(&original, &after_pin);
    let read = pmi_of(&after_pin);
    assert_eq!(read[PLATE], PartPmi::default(), "the plate is untouched");
    assert_eq!(read[PIN].material.as_ref().unwrap().id, "brass");

    let plate = Session::open_part("assembly.step", &after_pin, PLATE).unwrap();
    let after_plate = written(&plate.write_pmi(&answers(PLATE), None, None).unwrap());
    assert_eq!(after_plate, cli_write(&after_pin, PLATE, &answers(PLATE)));
    assert_kept(&after_pin, &after_plate);
    let read_again = pmi_of(&after_plate);
    assert_eq!(read_again[PIN], read[PIN], "the pin's PMI is kept");
    assert_eq!(read_again[PLATE].material.as_ref().unwrap().id, "steel");

    let listed = Session::parts("both.step", &after_plate).unwrap();
    for (k, part) in listed.as_array().unwrap().iter().enumerate() {
        assert_eq!(part["written"], json!(true));
        let before = &Session::parts("assembly.step", &original).unwrap()[k];
        assert_eq!(part["placements"], before["placements"], "part {k}");
        let session = Session::open_part("both.step", &after_plate, k).unwrap();
        assert_eq!(
            session.snapshot()["analysis"]["resume"]["answers"],
            json!(answers(k)),
            "part {k}"
        );
    }
}

/// The editor's project written with a part of an assembly is that part's: it reads back from
/// that part's session, not the other's, and the original given back is the whole file.
#[test]
fn a_project_is_carried_on_its_part_of_an_assembly() {
    let original = assembly();
    // base64 of `{"revision":1}`, as the editor encodes its project.
    let project = "eyJyZXZpc2lvbiI6MX0=";
    let pin = Session::open_part("assembly.step", &original, PIN).unwrap();
    let after_pin = written(&pin.write_pmi(&answers(PIN), Some(project), None).unwrap());
    assert_kept(&original, &after_pin);
    let plate = Session::open_part("assembly.step", &after_pin, PLATE).unwrap();
    assert_eq!(
        plate.payload(PROJECT),
        None,
        "the pin's project is not the plate's"
    );
    assert_eq!(plate.original().as_deref(), Some(original.as_slice()));
    let payload = json!({ "name": "app", "value": { "sheet": 2 } });
    let after_plate = written(
        &plate
            .write_pmi(&answers(PLATE), None, Some(&payload))
            .unwrap(),
    );
    for (k, project, app) in [
        (PIN, Some(json!({ "revision": 1 })), None),
        (PLATE, None, Some(json!({ "sheet": 2 }))),
    ] {
        let session = Session::open_part("both.step", &after_plate, k).unwrap();
        assert_eq!(session.payload(PROJECT), project, "part {k}");
        assert_eq!(session.payload("app"), app, "part {k}");
        assert_eq!(session.original().as_deref(), Some(original.as_slice()));
    }
    assert_kept(&after_pin, &after_plate);
    assert_eq!(resume::stored(&after_plate).len(), 2);
}

/// A file of one part placed twice: refused without a part chosen, as before; listed with its
/// two placements; opened as part 0 as the command line loads it, and written as it writes.
#[test]
fn a_part_placed_twice_is_opened_and_written() {
    let bytes = twice();
    let Err(refused) = Session::open("twice.step", &bytes) else {
        panic!("a part placed twice opens with no part chosen");
    };
    assert!(refused.contains("one instance of the part"), "{refused}");
    assert!(refused.contains("openPart"), "{refused}");
    let listed = Session::parts("twice.step", &bytes).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["instances"], json!(2));

    let session = Session::open_part("twice.step", &bytes, 0).unwrap();
    let file = File::of_bytes(Path::new("twice.step"), bytes.clone()).unwrap();
    let loaded = file.load(None).unwrap();
    assert_eq!(loaded.placements.len(), 2);
    assert_eq!(
        session.snapshot()["analysis"],
        api::analyse(&loaded).unwrap()
    );
    assert_eq!(session.snapshot()["mesh"], api::mesh(&loaded));
    let given = answers(PLATE);
    let out = written(&session.write_pmi(&given, None, None).unwrap());
    assert_eq!(out, cli_write(&bytes, 0, &given));
    assert_kept(&bytes, &out);
    let back = Session::open_part("twice.step", &out, 0).unwrap();
    assert_eq!(
        back.snapshot()["analysis"]["resume"]["answers"],
        json!(given)
    );
    assert_eq!(
        Session::parts("twice.step", &out).unwrap()[0]["instances"],
        json!(2)
    );
}

/// A file of one part used once opens through `open_part(.., 0)` exactly as through `open`.
#[test]
fn part_zero_of_one_part_opens_as_before() {
    let bytes = std::fs::read(fixture("plate")).unwrap();
    let (open, chosen) = (
        Session::open("plate.step", &bytes).unwrap(),
        Session::open_part("plate.step", &bytes, 0).unwrap(),
    );
    assert_eq!(open.snapshot(), chosen.snapshot());
    assert_eq!(open.snapshot()["provenance"]["part"], json!(0));
    let given = answers(PLATE);
    assert_eq!(
        open.write_pmi(&given, None, None).unwrap(),
        chosen.write_pmi(&given, None, None).unwrap()
    );
}

//! Parse the real `.inp` assets in `tests/data` (from meshio's test suite).

use abaqus_inp::{Mesh, parse_file};

fn parse(name: &str) -> Mesh {
    parse_file(format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

#[test]
fn element_elset() {
    let mesh = parse("element_elset.inp");
    assert_eq!(mesh.node_ids, [1, 2, 3, 4]);
    assert_eq!(mesh.node_coords[2], [2.0, 0.5, 0.0]);
    assert_eq!(mesh.element_blocks.len(), 2);
    assert!(mesh.element_blocks.iter().all(|b| b.element_type == "S3"));
    // `all` is built from the set names `right` and `left`.
    assert_eq!(mesh.element_set("all").unwrap().ids, [1, 2]);
}

#[test]
fn nle1xf3c() {
    let mesh = parse("nle1xf3c.inp");
    assert_eq!(mesh.node_ids.len(), 12);
    assert_eq!(mesh.node_set("CF000").unwrap().ids.len(), 12);
    let b = &mesh.element_blocks[0];
    assert_eq!(
        (b.element_type.as_str(), b.ids.len(), b.nodes_per_element),
        ("CPS3", 12, 3)
    );
    assert_eq!(mesh.element_set("LOAD").unwrap().ids, [7, 9, 11]);
    // Final data line is `6, ` — the trailing comma must not swallow *SOLID SECTION.
    assert_eq!(mesh.element_set("PR").unwrap().ids, [6]);
}

#[test]
fn uuea() {
    let mesh = parse("UUea.inp");
    assert_eq!(mesh.node_ids.len(), 66);
    let b = &mesh.element_blocks[0];
    assert_eq!(
        (b.element_type.as_str(), b.ids.len(), b.nodes_per_element),
        ("CAX4P", 50, 4)
    );
    // Connectivity split across lines: element 10 continues onto the next line.
    let (id, conn) = b.elements().nth(9).unwrap();
    assert_eq!((id, conn), (10, &[11, 12, 18, 17][..]));
    // GENERATE sets.
    assert_eq!(
        mesh.node_set("_PickedSet2").unwrap().ids,
        (1..=66).collect::<Vec<_>>()
    );
    assert_eq!(
        mesh.element_set("__PickedSurf13_S2").unwrap().ids,
        (5..=50).step_by(5).collect::<Vec<_>>()
    );
}

#[test]
fn include() {
    let mesh = parse("wInclude_main.inp");
    assert_eq!(mesh.node_ids, [1, 2, 3]);
    let b = &mesh.element_blocks[0];
    assert_eq!((b.element_type.as_str(), &b.ids[..]), ("B31", &[3, 4][..]));
    assert_eq!(mesh.element_set("elBm_set").unwrap().ids, [3, 4]);
}

//! Parse the real `.inp` assets in `tests/data` (from meshio's test suite and
//! the MCNP 6.3.1 manual §8.7).

use abaqus_inp::{Mesh, parse_file};

fn parse(name: &str) -> Mesh {
    parse_file(format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

#[test]
fn element_elset() {
    let mesh = parse("element_elset.inp");
    let p = &mesh.parts[0];
    assert_eq!(p.name, "");
    assert_eq!(p.node_ids, [1, 2, 3, 4]);
    assert_eq!(p.node_coords[2], [2.0, 0.5, 0.0]);
    assert_eq!(p.element_blocks.len(), 2);
    assert!(p.element_blocks.iter().all(|b| b.element_type == "S3"));
    // `all` is built from the set names `right` and `left`.
    assert_eq!(p.element_set("all").unwrap().ids, [1, 2]);
}

#[test]
fn nle1xf3c() {
    let mesh = parse("nle1xf3c.inp");
    let p = &mesh.parts[0];
    assert_eq!(p.node_ids.len(), 12);
    assert_eq!(p.node_set("CF000").unwrap().ids.len(), 12);
    let b = &p.element_blocks[0];
    assert_eq!(
        (b.element_type.as_str(), b.ids.len(), b.nodes_per_element),
        ("CPS3", 12, 3)
    );
    assert_eq!(p.element_set("LOAD").unwrap().ids, [7, 9, 11]);
    // Final data line is `6, ` — the trailing comma must not swallow *SOLID SECTION.
    assert_eq!(p.element_set("PR").unwrap().ids, [6]);
    assert_eq!(mesh.materials[0].name, "M001P001");
}

#[test]
fn uuea() {
    let mesh = parse("UUea.inp");
    // Mesh data sits inside the *Instance block and lands in its part.
    let p = mesh.part("Part-1").unwrap();
    assert_eq!(p.node_ids.len(), 66);
    let b = &p.element_blocks[0];
    assert_eq!(
        (b.element_type.as_str(), b.ids.len(), b.nodes_per_element),
        ("CAX4P", 50, 4)
    );
    // Connectivity split across lines: element 10 continues onto the next line.
    let (id, conn) = b.elements().nth(9).unwrap();
    assert_eq!((id, conn), (10, &[11, 12, 18, 17][..]));
    // GENERATE sets, including assembly-level ones with instance=Part-1-1.
    assert_eq!(
        p.node_set("_PickedSet2").unwrap().ids,
        (1..=66).collect::<Vec<_>>()
    );
    assert_eq!(
        p.element_set("__PickedSurf13_S2").unwrap().ids,
        (5..=50).step_by(5).collect::<Vec<_>>()
    );
    assert_eq!(mesh.instances[0].name, "Part-1-1");
    assert_eq!(mesh.instances[0].part, "Part-1");
    assert_eq!(mesh.materials[0].name, "Material-1");
    assert_eq!(mesh.materials[0].density, Some(2.7e-06));
}

#[test]
fn include() {
    let mesh = parse("wInclude_main.inp");
    // The include sits inside *Part, so its mesh belongs to that part.
    let p = mesh.part("ParametricModel").unwrap();
    assert_eq!(p.node_ids, [1, 2, 3]);
    let b = &p.element_blocks[0];
    assert_eq!((b.element_type.as_str(), &b.ids[..]), ("B31", &[3, 4][..]));
    assert_eq!(p.element_set("elBm_set").unwrap().ids, [3, 4]);
}

/// The MCNP 6.3.1 manual's example unstructured mesh input file (§8.7.2.10).
#[test]
fn mcnp_example() {
    let mesh = parse("mcnp_example_um.inp");
    assert_eq!(mesh.parts.len(), 1);
    let p = mesh.part("MyCube").unwrap();
    assert_eq!(p.node_ids.len(), 120);
    assert_eq!(p.node_coords[0], [-3.33333333, -5.0, 8.0]);
    let b = &p.element_blocks[0];
    assert_eq!(
        (b.element_type.as_str(), b.ids.len(), b.nodes_per_element),
        ("C3D8R", 60, 8)
    );
    // Required MCNP material/statistic elset (here one set carries both tags).
    assert_eq!(
        p.element_set("Set_statistic_material_1").unwrap().ids,
        (1..=60).collect::<Vec<_>>()
    );
    assert_eq!(
        p.node_set("Set_statistic_material_1").unwrap().ids,
        (1..=120).collect::<Vec<_>>()
    );
    let inst = &mesh.instances[0];
    assert_eq!(
        (inst.name.as_str(), inst.part.as_str()),
        ("MyCube-1", "MyCube")
    );
    assert_eq!(inst.translation, [0.0, 0.0, -10.0]);
    assert_eq!(inst.rotation, None);
    assert_eq!(mesh.materials[0].name, "Uranium");
    assert_eq!(mesh.materials[0].density, Some(-18.7));
}

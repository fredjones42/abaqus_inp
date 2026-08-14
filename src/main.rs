#![warn(clippy::all)]

use std::process::exit;

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: abaqus_inp <file.inp>");
        exit(2);
    };
    let mesh = abaqus_inp::parse_file(&path).unwrap_or_else(|e| {
        eprintln!("{path}: {e}");
        exit(1);
    });
    println!(
        "{} nodes, {} elements",
        mesh.node_ids.len(),
        mesh.element_count()
    );
    for b in &mesh.element_blocks {
        println!("  {} x {}", b.ids.len(), b.element_type);
    }
    for (kind, sets) in [("nset", &mesh.node_sets), ("elset", &mesh.element_sets)] {
        for s in sets {
            println!("  {kind} {} ({} ids)", s.name, s.ids.len());
        }
    }
}

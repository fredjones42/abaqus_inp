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
    for p in &mesh.parts {
        println!(
            "part `{}`: {} nodes, {} elements",
            p.name,
            p.node_ids.len(),
            p.element_count()
        );
        for b in &p.element_blocks {
            println!("  {} x {}", b.ids.len(), b.element_type);
        }
        for (kind, sets) in [("nset", &p.node_sets), ("elset", &p.element_sets)] {
            for s in sets {
                println!("  {kind} {} ({} ids)", s.name, s.ids.len());
            }
        }
    }
    for i in &mesh.instances {
        println!(
            "instance `{}` of `{}`, translation {:?}{}",
            i.name,
            i.part,
            i.translation,
            if i.rotation.is_some() {
                ", rotated"
            } else {
                ""
            }
        );
    }
    for m in &mesh.materials {
        match m.density {
            Some(d) => println!("material `{}`, density {d}", m.name),
            None => println!("material `{}`", m.name),
        }
    }
}

# abaqus_inp

Abaqus mesh input file (`.inp`) parser. Zero dependencies.

Parses the mesh-defining keywords — `*NODE`, `*ELEMENT`, `*NSET`, `*ELSET`,
`*PART`, `*INSTANCE` (translation/rotation), `*MATERIAL`, `*DENSITY`, and
`*INCLUDE` (with continuation lines, `GENERATE`, and set-name references) —
and skips everything else (sections, steps, physics). Node/element numbering
is local to each part; flat files yield one part named `""`. This covers the
subset MCNP requires of Abaqus-formatted unstructured mesh files
(MCNP 6.3.1 §8.7).

```rust
let mesh = abaqus_inp::parse_file("model.inp")?;
for part in &mesh.parts {
    for block in &part.element_blocks {
        for (id, nodes) in block.elements() {
            // id: element label, nodes: &[u32] connectivity
        }
    }
}
```

There is also a small CLI: `cargo run --release -- model.inp` prints a mesh
summary.

Test files in `tests/data/` come from [meshio](https://github.com/nschloe/meshio)
(MIT license) and the MCNP 6.3.1 manual §8.7.2.10
(`example_unstructured_mesh.abaq.inp.txt`, stored here as
`mcnp_example_um.inp`; the manual is a US government work, LA-UR-24-24602),
stored as plain files (no git LFS).

## Develop

```sh
cargo clippy --all-targets --all-features -- -D warnings
cargo +nightly fmt
cargo test
cargo bench
```

`rustfmt.toml` uses unstable options, so formatting needs nightly.

## License

MIT OR Apache-2.0, at your option.

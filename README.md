# abaqus_inp

Abaqus mesh input file (`.inp`) parser. Zero dependencies.

Parses the mesh-defining keywords — `*NODE`, `*ELEMENT`, `*NSET`, `*ELSET`,
`*INCLUDE` (with continuation lines, `GENERATE`, and set-name references) —
and skips everything else (sections, materials, steps). Part/instance
structure is flattened into one node/element namespace.

```rust
let mesh = abaqus_inp::parse_file("model.inp")?;
for block in &mesh.element_blocks {
    for (id, nodes) in block.elements() {
        // id: element label, nodes: &[u32] connectivity
    }
}
```

There is also a small CLI: `cargo run --release -- model.inp` prints a mesh
summary.

Test files in `tests/data/` come from [meshio](https://github.com/nschloe/meshio)
(MIT license), stored as plain files (no git LFS).

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

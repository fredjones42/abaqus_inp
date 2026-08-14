# abaqus_inp

Abaqus mesh input file parser

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

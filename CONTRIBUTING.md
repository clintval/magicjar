# Developer Documentation

## Local Compilation

Build from source with Cargo with:

```bash
cargo build --release
./target/release/magicjar --help
```

## Local Testing

To ensure all tests pass, run:

```bash
cargo test --all --no-fail-fast --verbose
```

Tests that run a real JVM (prepending to the bundled `tests/fixtures/hello.jar`
and executing it) are skipped automatically when `java` is not on the `PATH`, so
the suite passes with or without a JDK installed.

## Local Linting and Formatting

To check the format and lint of the code, run:

```bash
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

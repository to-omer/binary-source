# `binary-source`

This tool generates source code with embedded Rust executable binaries.

## Installation
```sh
cargo install --git https://github.com/to-omer/binary-source
```

Install [UPX](https://upx.github.io/) to compress executables.

If cross compilation is required, [cross](https://github.com/cross-rs/cross) must be installed.

## Usage
```
# Override source code to binary embedded code
$ binary-source --output src/main.rs

# Select binary target
$ binary-source --bin bin_name

# Enable cross compilation
$ binary-source --use-cross

# Embed into Python
$ binary-source --output main.py --language python
```

## Options
```
USAGE:
    binary-source [FLAGS] [OPTIONS]

FLAGS:
    -h, --help            Prints help information
        --no-opt-size     Do not add opt-level="s"
        --no-upx          Do no use upx unless available
        --panic-unwind    If false, panic_abort
        --use-cross       Use `cross` to compile
    -V, --version         Prints version information

OPTIONS:
        --bin <NAME>              Name of the bin target to compile
        --language <language>     Output language [Rust|Python] [default: Rust]
        --manifest-path <PATH>    `cargo` Path to Cargo.toml
    -o, --output <PATH>           Output filename [default: main.rs]
        --target <TRIPLE>         target [default: x86_64-unknown-linux-gnu]
```

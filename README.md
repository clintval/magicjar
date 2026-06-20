# magicjar

[![Install with conda-forge](https://img.shields.io/badge/Install%20with-conda--forge-brightgreen.svg)](https://anaconda.org/conda-forge/magicjar)
[![Anaconda Version](https://anaconda.org/conda-forge/magicjar/badges/version.svg)](https://anaconda.org/conda-forge/magicjar)
[![Build Status](https://github.com/clintval/magicjar/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/clintval/magicjar/actions/workflows/ci.yml?query=branch%3Amain)
[![Coverage Status](https://coveralls.io/repos/github/clintval/magicjar/badge.svg?branch=main)](https://coveralls.io/github/clintval/magicjar?branch=main)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Language](https://img.shields.io/badge/language-rust-dea588.svg)](https://www.rust-lang.org/)

Make a Java JAR self-executing by prepending a shell preamble.

![magicjar](.github/img/cover.jpg)

Install with mamba, conda, or run directly with pixi:

```bash
pixi exec \
    -c conda-forge \
    magicjar --help
```

## Introduction

`magicjar` turns a Java `.jar` into a single self-executing file.
A JAR is a ZIP archive, and the JVM reads a ZIP's directory from the *end* of the file, so any bytes prepended to the front are ignored by `java -jar`.
`magicjar` exploits this by prepending a small shell preamble that re-launches the JVM on the file itself: the result runs as `./fgbio` and still works as `java -jar fgbio`.

The input can be a `.jar`, a symlink to one, a conda/bioconda wrapper script, or a shell alias; `magicjar` resolves any of these down to the underlying archive.
Pointed at the wrapper-script-plus-jar layout that conda ships (two files), it re-wraps them into one portable executable.

## Quick Start

Wrap a jar, then run it directly:

```bash
❯ magicjar fgbio.jar fgbio
wrote fgbio (executable)

❯ ./fgbio --version
```

Omit the output name and it defaults to the input without the `.jar` suffix:

```bash
❯ magicjar fgbio.jar      # writes ./fgbio
```

Re-wrap a conda-installed tool into a single portable file:

```bash
❯ magicjar "$(pixi exec -s fgbio which fgbio)" fgbio
```

JVM flags pass straight through to the JVM; everything else goes to the program:

```bash
❯ ./fgbio -Xmx8g -XX:+UseZGC -Dconfig=prod CallMolecularConsensusReads -i in.bam
```

## Features

- Prepends a portable shell preamble and marks the result executable in one step.
- Resolves a `.jar`, a symlink, a conda/bioconda wrapper script, or a shell alias to the underlying archive.
- Routes any `-D`/`-X`/`-XX` flag to the JVM and passes the rest through to the program.
- Refuses to overwrite an existing file unless `--force` is given.

## Development and Testing

See the [contributing guide](./CONTRIBUTING.md) for more information.

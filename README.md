# magicjar

[![Install with conda-forge](https://img.shields.io/badge/Install%20with-conda--forge-brightgreen.svg)](https://anaconda.org/conda-forge/magicjar)
[![Anaconda Version](https://anaconda.org/conda-forge/magicjar/badges/version.svg)](https://anaconda.org/conda-forge/magicjar)
[![Build Status](https://github.com/clintval/magicjar/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/clintval/magicjar/actions/workflows/ci.yml?query=branch%3Amain)
[![Coverage Status](https://coveralls.io/repos/github/clintval/magicjar/badge.svg?branch=main)](https://coveralls.io/github/clintval/magicjar?branch=main)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Language](https://img.shields.io/badge/language-rust-dea588.svg)](https://www.rust-lang.org/)

Make a Java JAR self-executing.

![magicjar](.github/img/cover.jpg)

Install with mamba, conda, or run directly with pixi:

```bash
pixi exec \
    -c conda-forge \
    magicjar --help
```

## Introduction

The command `magicjar` turns a Java JAR into a single self-executing file.

The input can be a JAR, a symlink to one, a wrapper script, or a shell alias.
The command `magicjar` resolves any of these down to the underlying JAR file.

## Quick Start

Make a JAR self-executing, then test it out:

```bash
❯ magicjar fgbio.jar fgbio

❯ ./fgbio --version
```

Omit the output name and it defaults to the input without the JAR suffix:

```bash
❯ magicjar fgbio.jar
```

Re-wrap a conda-installed tool into a single executable and portable file:

```bash
❯ magicjar "$(pixi exec -s fgbio which fgbio)" fgbio
```

JVM flags pass straight through to the JVM; everything else goes to the program:

```bash
❯ ./fgbio -Xmx8g -XX:+UseZGC -Dconfig=prod CallMolecularConsensusReads -i "in.bam"
```

## Use in a Dockerfile

Copy the binary out of the published image:

```dockerfile
COPY --from=ghcr.io/clintval/magicjar:0.1.0 /magicjar /usr/local/bin/magicjar
```

Or fetch the binary straight from the GitHub Release (swap the target for
`aarch64-unknown-linux-musl` on ARM):

```dockerfile
ADD --chmod=755 \
    https://github.com/clintval/magicjar/releases/download/0.1.0/magicjar-x86_64-unknown-linux-musl \
    /usr/local/bin/magicjar
```

## Development and Testing

See the [contributing guide](./CONTRIBUTING.md) for more information.

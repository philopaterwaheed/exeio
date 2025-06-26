# exeio

A process supervisor written in Rust to help server programmers run and monitor processes from outside the server through a REST API.

## Table of Contents

- [Features](#features)
- [Installation](#installation)
- [Usage](#usage)
## Features

- **Process Supervision:** Start, stop, and monitor multiple processes on your server.
- **REST API:** Control and query process states remotely via HTTP endpoints.
- **Written in Rust:** High performance and safety.

## Installation

> **Prerequisite:** Rust toolchain (https://rustup.rs/)

Clone the repository and build the project:

```bash
git clone https://github.com/philopaterwaheed/exeio.git
cd exeio
cargo build --release
cd ./target/release
sudo mv exeio /usr/bin/
```

## Usage

After building, run the supervisor:

```bash
cargo run --release
```

This starts the supervisor and exposes a REST API for process management.


> **Author:** [philopaterwaheed](https://github.com/philopaterwaheed)

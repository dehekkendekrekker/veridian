#!/bin/bash
cargo build --release

sudo cp ./target/release/veridian /usr/bin/

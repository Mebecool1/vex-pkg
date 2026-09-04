#!/bin/bash
set -e

if [ -z "$1" ]; then
  echo "usage: ./bump.sh <version>"
  exit 1
fi

sed -i "s/^version = \".*\"/version = \"$1\"/" Cargo.toml
git add Cargo.toml
git commit -m "bump to $1"
cargo publish --allow-dirty
cargo install vex-pkg
echo "done, now at $1"

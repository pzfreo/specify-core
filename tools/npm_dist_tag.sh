#!/usr/bin/env bash
# The caller supplies the package version and npm's current latest (empty if absent).
set -eu
version=$1
latest=$2

case "$version" in
  *-*) echo next ;;
  *)
    if [ -n "$latest" ] && [ "$(printf '%s\n' "$latest" "$version" | sort -V | tail -1)" != "$version" ]; then
      echo "release-$version"
    else
      echo latest
    fi
    ;;
esac

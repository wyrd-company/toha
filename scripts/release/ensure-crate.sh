#!/usr/bin/env bash
# ---
# relationships:
#   implements: architecture
# ---
# Publishes the toha crate at VERSION to crates.io, or verifies that
# crates.io already holds the same package bytes for VERSION.

set -euo pipefail

version="${1:?version is required}"
crate=toha
registry_url="https://index.crates.io/to/ha/$crate"

manifest_version="$(cargo metadata --locked --no-deps --format-version 1 |
  jq -r --arg crate "$crate" '.packages[] | select(.name == $crate) | .version')"
if [[ "$manifest_version" != "$version" ]]; then
  echo "Cargo.toml declares $crate $manifest_version, not $version." >&2
  exit 1
fi

cargo package --locked
archive="target/package/${crate}-${version}.crate"
test -f "$archive"
local_checksum="$(sha256sum "$archive" | cut -d' ' -f1)"

registry_response="$(mktemp)"
trap 'rm -f "$registry_response"' EXIT

lookup() {
  curl --silent --show-error \
    --header 'User-Agent: toha-release-workflow (https://github.com/wyrd-company/toha)' \
    --output "$registry_response" \
    --write-out '%{http_code}' \
    "$registry_url"
}

published_checksum() {
  jq -r --arg version "$version" \
    'select(.vers == $version) | .cksum' \
    "$registry_response" | tail -n 1
}

status="$(lookup)"
case "$status" in
  200)
    observed_checksum="$(published_checksum)"
    if [[ -n "$observed_checksum" ]]; then
      if [[ "$observed_checksum" != "$local_checksum" ]]; then
        echo "$crate $version is on crates.io with different package bytes." >&2
        exit 1
      fi
      echo "$crate $version is already on crates.io with matching package bytes; skipping publication."
      exit 0
    fi
    ;;
  404) ;;
  *)
    echo "crates.io lookup for $crate $version failed with HTTP $status." >&2
    exit 1
    ;;
esac

# cargo publish returns after the new version is available in the index.
cargo publish --locked
echo "$crate $version is published to crates.io."

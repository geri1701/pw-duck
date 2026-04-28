#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

# shellcheck disable=SC1091
source ./PKGBUILD

if [[ ${#source[@]} -ne 1 ]]; then
  echo "expected exactly one source entry in PKGBUILD" >&2
  exit 1
fi

source_url=${source[0]#*::}
tmp=$(mktemp)
trap 'rm -f "$tmp" "$tmp.pkgbuild" "$tmp.srcinfo"' EXIT

if ! curl -LfsS "$source_url" -o "$tmp"; then
  echo "failed to fetch release tarball:" >&2
  echo "  $source_url" >&2
  echo "Publish the GitHub tag first, then rerun this script." >&2
  exit 1
fi

sum=$(sha256sum "$tmp" | awk '{print $1}')

awk -v sum="$sum" '
  /^sha256sums=\047/ {
    print "sha256sums=(\047" sum "\047)"
    count++
    next
  }
  { print }
  END {
    if (count != 1) {
      print "expected exactly one sha256sums entry in PKGBUILD" > "/dev/stderr"
      exit 1
    }
  }
' PKGBUILD > "$tmp.pkgbuild"
mv "$tmp.pkgbuild" PKGBUILD

awk -v sum="$sum" '
  /^\tsha256sums = / {
    print "\tsha256sums = " sum
    count++
    next
  }
  { print }
  END {
    if (count != 1) {
      print "expected exactly one sha256sums entry in .SRCINFO" > "/dev/stderr"
      exit 1
    }
  }
' .SRCINFO > "$tmp.srcinfo"
mv "$tmp.srcinfo" .SRCINFO

bash -n PKGBUILD

echo "updated AUR checksum: $sum"
echo "files changed: PKGBUILD .SRCINFO"

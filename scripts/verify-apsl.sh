#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

cargo build --quiet -p apslc -p apsl-lint

mapfile -t files < <(find . -type f -name '*.apsl' -not -path './target/*' | sort)

for file in "${files[@]}"; do
  ./target/debug/apslc check "$file"
done

for file in "${files[@]}"; do
  if [[ "$file" != ./examples/bad_n_squared.apsl ]]; then
    ./target/debug/apsl-lint complex "$file"
  fi
done

if ./target/debug/apsl-lint complex ./examples/bad_n_squared.apsl; then
  exit 1
fi

for file in "${files[@]}"; do
  case "$file" in
    ./examples/dedupe.apsl|./proofs/soundness-phase-bound.apsl) ;;
    *) ./target/debug/apsl-lint pred "$file" ;;
  esac
done

if ./target/debug/apsl-lint pred ./examples/dedupe.apsl; then
  exit 1
fi

if ./target/debug/apsl-lint pred ./proofs/soundness-phase-bound.apsl; then
  exit 1
fi

./target/debug/apslc check ./tests/nominal/pass_distinct_types.apsl --nominal
if ./target/debug/apslc check ./tests/nominal/fail_type_confusion.apsl --nominal; then
  exit 1
fi

./target/debug/apslc check ./tests/restricted/pass_narrowing.apsl --restricted
if ./target/debug/apslc check ./tests/restricted/fail_widening.apsl --restricted; then
  exit 1
fi

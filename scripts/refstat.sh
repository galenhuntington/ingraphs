#! /usr/bin/env bash

SRC="$1"
TIMEOUT=${2:-60}

while read -r i; do
   echo -n "$i $(date -uIns) "
   timeout "$TIMEOUT" ./target/release/graphy-u256 cull 19 "$SRC" <(echo "$i") 2> /dev/null | grep -c Some
done

date -uIns


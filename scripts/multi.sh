#! /usr/bin/env bash

SIZE="$1"
MOD="$2"
BATCH="$3"

# 14:
#   105760086734720 and 105760105576384 are very slow (symmetric) so usually separate
#   recently refuted to use as controls: 105760631863104, 35391896139584, 207515662272, 105760095090496
#   last is hard, many runs without re-refutation
# Example:
#   time seq 1 18 | xargs -P6 -I{} ./scripts/multi.sh 14 5000000 std14 {}
# (final {} is unused but tracks progress in process command line)

# shellcheck disable=SC2046 disable=SC2005
GRAPHS=$(echo $(cat "output/batches/$BATCH"))

if [ -z "$GRAPHS" ]; then
  echo No graphs.
  exit 1
fi

DIR=output/multiruns/$BATCH-$SIZE-$MOD
mkdir -p "$DIR"

RES=$(( SRANDOM % MOD ))
LOG="$DIR/log-$RES.log"
MID=$(( (SIZE*(SIZE-1)+3)/4 ))

# XXX maybe each RES a directory?
SEEDS=$DIR/seeds-$RES.g6
FOUND=$DIR/found-$RES.csv

if [ -f "$FOUND" ]; then
  echo Already exists.
  exit
fi

{
  time {
    time GRAPHY_SUB="${GRAPHS//\ /,}" ./nauty-prune/gengf -q -X1 "$SIZE" $MID:$MID "$RES/$MOD" >"$SEEDS"
    : > "$FOUND"
    for g in $GRAPHS; do
       echo "# $g" >&2
       {
         echo "# $g"
         ./target/release/graphy free-close "$SIZE" "$g" <(./target/release/graphy free-scan "$SIZE" "$g" "$SEEDS") | grep false
       } >>"$FOUND"
    done
  }
} 2>>"$LOG"

true


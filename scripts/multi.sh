#! /usr/bin/env bash

SIZE="$1"
MOD="$2"
BATCH="$3"

# 14:
#   105760086734720 and 105760105576384 are very slow so usually separate
#   recently refuted to use as controls: 105760631863104, 35391896139584, 207515662272, 105760095090496

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

SEEDS=$DIR/seeds-$RES.csv
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
       {
         echo "# $g"
         ./target/release/graphy free-close "$SIZE" "$g" <(RAYON_NUM_THREADS=1 ./target/release/graphy free-scan "$SIZE" "$g" "$SEEDS") | grep false
       } >>"$FOUND"
    done
  }
} 2>>"$LOG"

true


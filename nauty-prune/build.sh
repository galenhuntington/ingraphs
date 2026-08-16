#!/bin/sh
# Build gengf: nauty's geng with a PRUNE hook that rejects any (partial)
# graph containing the subgraph given in $GRAPHY_SUB (graphy decimal
# encoding, trailing isolated vertices omitted).  So gengf enumerates
# exactly the GRAPHY_SUB-free graphs.
#
# Usage afterwards, e.g.:
#   GRAPHY_SUB=69539838912 ./gengf -q 13 39:39 123/600000
set -e
cd "$(dirname "$0")"
VER=2_8_9
SHA224=67f49e7f4152105926a509766b892327d15f76177088ada003ee61c5
FILE=nauty$VER.tar.gz
[ -f $FILE ] || curl -sLO https://pallini.di.uniroma1.it/$FILE
[ "$(sha224sum $FILE | cut -f1 -d' ')" = $SHA224 ] || { echo Checksum mismatch for tar file.; exit 1; }
[ -d nauty$VER ] || tar xzf $FILE
cp prune_sub.c nauty$VER/
cd nauty$VER
grep -q gtoolsW makefile || ./configure --quiet
make gtoolsW.o nautyW1.o nautilW1.o naugraphW1.o schreier.o naurng.o
gcc -o ../gengf -O3 -march=native -DWORDSIZE=32 -DMAXN=WORDSIZE \
    -DPRUNE=prune_sub geng.c prune_sub.c \
    gtoolsW.o nautyW1.o nautilW1.o naugraphW1.o schreier.o naurng.o
echo "built ./gengf"


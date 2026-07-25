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
[ -f nauty$VER.tar.gz ] || curl -sLO https://pallini.di.uniroma1.it/nauty$VER.tar.gz
[ -d nauty$VER ] || tar xzf nauty$VER.tar.gz
cp prune_sub.c nauty$VER/
cd nauty$VER
[ -f makefile ] || ./configure --quiet
make gtoolsW.o nautyW1.o nautilW1.o naugraphW1.o schreier.o naurng.o
gcc -o ../gengf -O3 -march=native -DWORDSIZE=32 -DMAXN=WORDSIZE \
    -DPRUNE=prune_sub geng.c prune_sub.c \
    gtoolsW.o nautyW1.o nautilW1.o naugraphW1.o schreier.o naurng.o
echo "built ./gengf"

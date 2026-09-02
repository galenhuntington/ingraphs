This is code for my article [Dense Universal
Ingraphs](https://galen.xyz/ingraphs/).  It provides several tools
for working on this problem.  It is written in Rust, and is quite
rough, but usable.

## Problem

A universal ingraph for _n_ is a graph G such that for any graph H
on _n_ vertices, G is a subgraph of either H or the complement of H.

For example, a universal ingraph for 6 is the five-vertex graph
looking like ☐_.

The goal is to find universal ingraphs with the most number of edges.

## Code

This code has a smorgasbord of operations aimed towards finding and
analyzing ingraphs.  Some of it could be better documented.

Graphs are represented by decimal numbers (perhaps not the best system
but adequate).

## Some graphs

Here are numeric representations of some graphs mentioned in the
article:

| Graph | Number |
| :--- | ---: |
| 2 DUI | 1 |
| 3 DUI | 3 |
| 5 DUI | 13 |
| 6 DUI | 94 |
| 7 DUI | 1118 |
| 8 DUI | 3448 |
| 9X | 101752 |
| 9 DUI | 36280 |
| 9 DUI | 36216 |
| 9 DUI | 101736 |
| 9 DUI | 101744 |
| 10 DUI | 2202040 |
| 10 DUI | 6395248 |
| 11 DUI | 6732736 |
| 12 DUI | 816167872 |
| 13 DUI | 206974663616 |
| 13 DUI | 207515663232 |

### 14 DUI candidates

These are the 32 candidates for a 14-DUI, with their properties:

| Graph | Vertices | Planar | \|&zwj;Sym&zwj;\| | ⊃ 11-DUI | ⊃ 12-DUI | ⊃ 2nd 13-DUI |
| ---: | ---: | :---: | ---: | :---: | :---: | :---: |
| 208052533120 | 10 | | 4 | 💎 | | |
| 208052598656 | 10 | | 4 | 💎 | | ✅ |
| 208061529088 | 10 | | 4 | 💎 | ✅ | |
| 208065657856 | 10 | | 2 | 💎 | ✅ | |
| 208065756160 | 10 | | 1 | 💎 | ✅ | |
| 208069916672 | 10 | | 4 | 💎 | ✅ | |
| 208090921984 | 10 | | 2 | 💎 | ✅ | |
| 208095050752 | 10 | | 4 | 💎 | | |
| 208095082496 | 10 | | 4 | 💎 | | |
| 209697274880 | 10 | | 1 | 💎 | ✅ | |
| 210234109952 | 10 | | 12 | 💎 | | |
| 210234143744 | 10 | | 2 | 💎 | ✅ | |
| 482968729600 | 10 | | 8 | 💎 | | |
| 35391346720704 | 11 | ✅ | 4 | | | |
| 35391348817856 | 11 | ✅ | 4 | | | |
| 35391350912960 | 11 | ✅ | 2 | | | |
| 35391350913984 | 11 | ✅ | 2 | | | |
| 35391361432512 | 11 | | 4 | 💎 | | |
| 35391363529600 | 11 | | 16 | | | |
| 35392164576128 | 11 | ✅ | 4 | | | |
| 35392424655744 | 11 | ✅ | 4 | | | |
| 35598312519616 | 11 | | 12 | 💎 | | |
| 35598323006336 | 11 | ✅ | 8 | | | |
| 35599401008000 | 11 | ✅ | 8 | | | |
| 105760090896320 | 11 | ✅ | 6 | | | |
| 105760090897344 | 11 | ✅ | 6 | | | |
| 105760090929024 | 11 | | 12 | | | |
| 105760090930048 | 11 | | 12 | | | |
| 105760095090560 | 11 | ✅ | 12 | | | |
| 105760642480000 | 11 | | 4 | | | |
| 105761168831296 | 11 | ✅ | 8 | | | |


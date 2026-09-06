# Min-Cost Flow Benchmark Datasets

Curated index of standard benchmark datasets and generators used to evaluate
minimum-cost flow solvers. Sourced primarily from the LEMON project's
[MinCostFlowData](https://lemon.cs.elte.hu/trac/lemon/wiki/MinCostFlowData)
page, which accompanies Péter Kovács' experimental evaluation paper.

All instances use the [DIMACS minimum-cost flow format](http://lpsolve.sourceforge.net/5.5/DIMACS_mcf.htm)
(integer data only).

## Reference paper

Péter Kovács. *Minimum-cost flow algorithms: an experimental evaluation.*
Optimization Methods and Software, 30:94–127, 2015.

- Published: <https://www.tandfonline.com/doi/full/10.1080/10556788.2014.895828>
- Preprint: <https://www.cs.elte.hu/egres/tr/egres-13-04.pdf>

## Generators

All four classic generators were collected for the 1st DIMACS Implementation
Challenge (1990–1991) and remain available from its FTP archive:

| Generator | Description | Source |
| --- | --- | --- |
| NETGEN | Random transportation/transshipment networks | <ftp://dimacs.rutgers.edu/pub/netflow/generators/network/netgen/> |
| GRIDGEN | Grid-structured networks | <ftp://dimacs.rutgers.edu/pub/netflow/generators/network/gridgen/> |
| GOTO | Grid On TOrus (hard for cost-scaling) | <ftp://dimacs.rutgers.edu/pub/netflow/generators/network/grid-on-torus/> |
| GRIDGRAPH | Layered grid graphs | <ftp://dimacs.rutgers.edu/pub/netflow/generators/network/gridgraph/> |

> The GOTO generator is already reimplemented in this repo
> (`src/problem_generators/goto/mod.rs`, CLI in `examples/gen_goto.rs`).

Index of the FTP tree: <ftp://dimacs.rutgers.edu/pub/netflow/>

## Pre-generated instance families

The LEMON authors published the exact instances used in the Kovács 2015
evaluation. Each family has a download directory hosted at
`lime.cs.elte.hu`. Parameter shell scripts (e.g. `goto_8.sh`) are attached
to the LEMON wiki page and describe how each family was generated.

### 1. NETGEN

Random networks of varying density and structure.

- Families: `NETGEN-8`, `NETGEN-SR`, `NETGEN-LO-8`, `NETGEN-LO-SR`, `NETGEN-DEG`
- Download: <http://lime.cs.elte.hu/~kpeter/data/mcf/netgen/>

### 2. GRIDGEN

Grid networks with random costs and capacities.

- Families: `GRIDGEN-8`, `GRIDGEN-SR`, `GRIDGEN-DEG`
- Download: <http://lime.cs.elte.hu/~kpeter/data/mcf/gridgen/>

### 3. GOTO (Grid On TOrus)

Designed by Goldberg & Tsioutsiouliklis to be difficult for cost-scaling
algorithms. 

- Families: `GOTO-8`, `GOTO-SR`
- Download: <http://lime.cs.elte.hu/~kpeter/data/mcf/goto/>

### 4. GRIDGRAPH

Layered grids with adjustable aspect ratio.

- Families: `GRID-WIDE`, `GRID-LONG`, `GRID-SQUARE`
- Download: <http://lime.cs.elte.hu/~kpeter/data/mcf/gridgraph/>

### 5. ROAD

Real US road networks (TIGER/Line) from the 9th DIMACS Implementation
Challenge. Arc costs are travel times; supply/demand nodes are chosen
randomly with values from a max-flow computation.

- Families:
  - `ROAD-PATHS` — unit arc capacities.
  - `ROAD-FLOW` — capacities of 40/60/80/100 by road category.
- Source data: <http://www.dis.uniroma1.it/challenge9/data/tiger/>
- Download: <http://lime.cs.elte.hu/~kpeter/data/mcf/road/>

### 6. VISION

Converted from large 3D-grid max-flow segmentation problems
(`bone_sub*_n6c100`) released by the Computer Vision Research Group at the
University of Western Ontario.

- Families:
  - `VISION-RND` — uniform random arc costs.
  - `VISION-PROP` — cost roughly proportional to capacity.
  - `VISION-INV` — cost roughly inversely proportional to capacity.
- Source data: <http://vision.csd.uwo.ca/data/maxflow/>
- Download: <http://lime.cs.elte.hu/~kpeter/data/mcf/vision/>

## Parameter scripts

The LEMON wiki attaches the exact generator parameter scripts:
`netgen_8.sh`, `netgen_sr.sh`, `netgen_lo_8.sh`, `netgen_lo_sr.sh`,
`netgen_deg.sh`, `gridgen_8.sh`, `gridgen_sr.sh`, `gridgen_deg.sh`,
`goto_8.sh`, `goto_sr.sh`, `grid_wide.sh`, `grid_long.sh`,
`grid_square.sh`. They are linked at the bottom of
<https://lemon.cs.elte.hu/trac/lemon/wiki/MinCostFlowData>.

## Related challenges and archives

- 1st DIMACS Implementation Challenge (Network Flows & Matching, 1990–1991):
  <http://archive.dimacs.rutgers.edu/Challenges/>
- 9th DIMACS Implementation Challenge (Shortest Paths, 2005–2006):
  <http://www.dis.uniroma1.it/challenge9/>
- DIMACS min-cost flow file format spec:
  <http://lpsolve.sourceforge.net/5.5/DIMACS_mcf.htm>

## Contact

Maintained by Péter Kovács (`kpeter [at] inf.elte.hu`). The canonical page,
which should be consulted for any link changes, is
<https://lemon.cs.elte.hu/trac/lemon/wiki/MinCostFlowData>.

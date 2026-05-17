// Diagram for testdata/sample.inp.
//
// Compile with: typst compile diagrams/sample.typ

#import "@preview/fletcher:0.5.8" as fletcher: diagram, edge, node

#set page(width: auto, height: auto, margin: 1cm)
#set text(size: 10pt)

= `testdata/sample.inp`: min-cost flow, 6 nodes, 8 arcs

Each edge is labeled $u\/c$, where $u$ is the upper capacity and $c$ is the
per-unit cost; the lower bound is $0$ on every arc in this instance.

Node $1$ has supply $+10$; node $6$ has demand $-10$.

#diagram(
  spacing: (5em, 3.5em),
  node-stroke: 0.8pt,
  node-shape: circle,
  node-fill: gradient.radial(white, blue, radius: 200%),
  label-size: 8pt,

  node((0, 1), align(center)[1], fill: green.lighten(70%), name: <1>),
  node((1, 0), align(center)[2]),
  node((2, 1), align(center)[3]),
  node((3, 0), align(center)[4]),
  node((3, 2), align(center)[5]),
  node((4, 1), align(center)[6], fill: red.lighten(70%), name: <2>),

  node(
    <1.north-east>,
    circle(fill: black, radius: 5pt, text(0.5em, $+ 10$, fill: white)),
    stroke: none,
    fill: none,
    inset: 0pt,
  ),
  node(
    <2.north-east>,
    circle(fill: black, radius: 5pt, text(0.5em, $- 10$, fill: white)),
    stroke: none,
    fill: none,
    inset: 0pt,
  ),

  edge((0, 1), (1, 0), "-|>", $4\/1$),
  edge((0, 1), (2, 1), "-|>", $8\/5$),
  edge((1, 0), (2, 1), "-|>", $5\/0$),
  edge((2, 1), (3, 2), "-|>", $10\/1$),
  edge((3, 2), (3, 0), "-|>", $8\/0$),
  edge((3, 2), (4, 1), "-|>", $8\/9$),
  edge((3, 0), (1, 0), "-|>", $8\/1$, bend: -40deg),
  edge((3, 0), (4, 1), "-|>", $8\/1$),
)

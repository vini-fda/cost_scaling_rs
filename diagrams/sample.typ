// Diagram for testdata/sample.inp.
//
// Compile with: typst compile diagrams/sample.typ

#import "@preview/fletcher:0.5.8" as fletcher: diagram, edge, node

#set page(width: auto, height: auto, margin: 1cm)
#set text(size: 10pt)

= `testdata/sample.inp`: min-cost flow, 6 nodes, 8 arcs

#let colred(x) = text(fill: red, $#x$)

Each edge shows the capacity range $ell <= c <= u$ above the line and the
per-unit cost below. Node $1$ has supply $+10$; node $6$ has demand $-10$.

#let capcost(lower, upper, cost) = $attach(ceil.r, tl: upper, bl: lower) cost$

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

  edge((0, 1), (1, 0), "-|>", $capcost(0, 4, 1)$, label-side: center),
  edge((0, 1), (2, 1), "-|>", $capcost(0, 8, 5)$, label-side: center),
  edge((1, 0), (2, 1), "-|>", $capcost(0, 5, 0)$, label-side: center),
  edge((2, 1), (3, 2), "-|>", $capcost(0, 10, 1)$, label-side: center),
  edge((3, 2), (3, 0), "-|>", $capcost(0, 8, 0)$, label-side: center),
  edge((3, 2), (4, 1), "-|>", $capcost(0, 8, 9)$, label-side: center),
  edge((3, 0), (1, 0), "-|>", $capcost(0, 8, 1)$, bend: -40deg, label-side: center),
  edge((3, 0), (4, 1), "-|>", $capcost(0, 8, 1)$, label-side: center),
)

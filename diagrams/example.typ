// Fletcher quick reference.
//
// Compile with: typst compile diagrams/example.typ
// Docs: https://typst.app/universe/package/fletcher

#import "@preview/fletcher:0.5.8" as fletcher: diagram, edge, node
#import fletcher.shapes: circle, diamond, hexagon

#set page(width: auto, height: auto, margin: 1cm)
#set text(size: 10pt)

= Fletcher quick reference

== 1. Minimal diagram

A `diagram(...)` is a container. `node(coord, content)` places a labeled
node at grid coordinate `(col, row)`. `edge(from, to, marks)` draws an arrow.

#diagram(
  node-stroke: 0.6pt,
  spacing: 3em,
  node((0, 0), [A]),
  node((1, 0), [B]),
  edge((0, 0), (1, 0), "->"),
)

== 2. Implicit endpoints

When an `edge` sits between two `node` calls, its endpoints default to
the previous and next node. Comma-separated direction strings
(`"r"`, `"d"`, `"ur"`, etc.) move one grid cell from the previous node.

#diagram(
  node-stroke: 0.6pt,
  spacing: 3em,
  node((0, 0), [A]), edge("->"),
  node((1, 0), [B]), edge("d", "->"),
  node((1, 1), [C]),
)

== 3. Labeled edges and arrow styles

Pass content as an extra positional argument to label an edge. Common
arrow shorthands: `"->"`, `"-|>"`, `"<->"`, `"->>"`, `"hook-->"`, `"-->"`.

#diagram(
  spacing: 3em,
  node-stroke: 0.6pt,
  node((0, 0), $X$),
  edge("-|>", $f$),
  node((1, 0), $Y$),
  edge("-->", $g$),
  node((2, 0), $Z$),
)

== 4. Shapes, fills, and bent edges

Import shape functions from `fletcher.shapes`. Use `bend:` (in degrees) for
curved edges. Positive bend curves left of the direction of travel; identical
`from`/`to` plus `bend:` draws a self-loop.

#diagram(
  spacing: 4em,
  node-stroke: 0.8pt,
  node((0, 0), [start], shape: circle, fill: green.lighten(70%)),
  edge("-|>", [run]),
  node((1, 0), [busy], shape: diamond, fill: yellow.lighten(70%)),
  edge((1, 0), (1, 0), [retry], "->", bend: 130deg),
  edge((1, 0), (2, 0), "-|>", [done], bend: -30deg),
  node((2, 0), [stop], shape: hexagon, fill: red.lighten(70%)),
)

== 5. Putting it together: a tiny flow network

A small foreshadowing of `sample.typ`. Each edge label is $u\/c$,
where $u$ is the upper capacity and $c$ is the per-unit cost.

#diagram(
  spacing: (4em, 3em),
  node-stroke: 0.8pt,
  node-shape: circle,
  node((0, 0), [s], fill: green.lighten(70%)),
  node((1, 0), [a]),
  node((2, 0), [t], fill: red.lighten(70%)),
  edge((0, 0), (1, 0), "-|>", $5\/2$),
  edge((1, 0), (2, 0), "-|>", $3\/4$),
  edge((0, 0), (2, 0), "-|>", $2\/9$, bend: -40deg),
)

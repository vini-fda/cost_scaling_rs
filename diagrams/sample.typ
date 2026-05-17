// Diagram for testdata/sample.inp.
//
// Compile with:
//   typst compile diagrams/sample.typ                            # light
//   typst compile diagrams/sample.typ --input theme=dark         # dark

#import "@preview/fletcher:0.5.8" as fletcher: diagram, edge, node

// Theme selection via `--input theme=dark`. Defaults to light so a bare
// `typst compile` still works. The page fill stays `none` so the diagram
// embeds cleanly on any background.
#let theme = sys.inputs.at("theme", default: "light")
#let (fg, bg, supply-fill, demand-fill) = if theme == "dark" {
  (white, rgb("#222"), green.darken(40%), red.darken(40%))
} else {
  (black, white, green.lighten(70%), red.lighten(70%))
}

#set page(width: auto, height: auto, margin: 1cm, fill: none)
#set text(size: 10pt, fill: fg)

#let colred(x) = text(fill: red, $#x$)

#let capcost(lower, upper, cost) = $attach(ceil.r, tl: upper, bl: lower) cost$

= Example: min-cost flow, 6 nodes, 8 arcs

Each edge shows the capacity lower and upper bounds ($ell <= c <= u$) to the left and the
per-unit cost $c$ to the right, denoted $capcost(ell, u, c)$.

Node $1$ has supply $+10$; node $6$ has demand $10$ (therefore a supply of $-10$), denoted with black circles on the upper right.

#set align(center)

#diagram(
  spacing: (5em, 3.5em),
  node-stroke: 0.8pt + fg,
  node-shape: circle,
  node-fill: gradient.radial(bg, blue, radius: 200%),
  edge-stroke: 0.6pt + fg,
  label-size: 8pt,

  node((0, 1), align(center)[1], fill: supply-fill, name: <1>),
  node((1, 0), align(center)[2]),
  node((2, 1), align(center)[3]),
  node((3, 0), align(center)[4]),
  node((3, 2), align(center)[5]),
  node((4, 1), align(center)[6], fill: demand-fill, name: <2>),

  node(
    <1.north-east>,
    circle(fill: fg, radius: 5pt, text(0.5em, $+ 10$, fill: bg)),
    stroke: none,
    fill: none,
    inset: 0pt,
  ),
  node(
    <2.north-east>,
    circle(fill: fg, radius: 5pt, text(0.5em, $- 10$, fill: bg)),
    stroke: none,
    fill: none,
    inset: 0pt,
  ),

  edge((0, 1), (1, 0), "-|>", $capcost(0, 4, 1)$, label-side: center, label-fill: bg),
  edge((0, 1), (2, 1), "-|>", $capcost(0, 8, 5)$, label-side: center, label-fill: bg),
  edge((1, 0), (2, 1), "-|>", $capcost(0, 5, 0)$, label-side: center, label-fill: bg),
  edge((2, 1), (3, 2), "-|>", $capcost(0, 10, 1)$, label-side: center, label-fill: bg),
  edge((3, 2), (3, 0), "-|>", $capcost(0, 8, 0)$, label-side: center, label-fill: bg),
  edge((3, 2), (4, 1), "-|>", $capcost(0, 8, 9)$, label-side: center, label-fill: bg),
  edge((3, 0), (1, 0), "-|>", $capcost(0, 8, 1)$, bend: -40deg, label-side: center, label-fill: bg),
  edge((3, 0), (4, 1), "-|>", $capcost(0, 8, 1)$, label-side: center, label-fill: bg),
)

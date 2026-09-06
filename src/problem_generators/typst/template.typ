
#import "@preview/fletcher:0.5.8" as fletcher: diagram, edge, node

// Theme and diagram defaults match diagrams/sample.typ exactly.
#let theme = sys.inputs.at("theme", default: "light")
#let (fg, bg, supply-fill, demand-fill) = if theme == "dark" {
  (white, rgb("#222"), green.darken(40%), red.darken(40%))
} else {
  (black, white, green.lighten(70%), red.lighten(70%))
}

#set page(width: auto, height: auto, margin: 1cm, fill: none)
#set text(size: 10pt, fill: fg)

#let capcost(lower, upper, cost) = $attach(ceil.r, tl: upper, bl: lower) cost$
#let view = sys.inputs.at("view", default: "overview")
#assert(view in ("overview", "details"), message: "view must be overview or details")

#let value-label(a) = if problem-kind == "Assignment" {
  $#a.cost$
} else if problem-kind == "Maximum flow" {
  $#a.cap$
} else {
  capcost($0$, $#a.cap$, $#a.cost$)
}

#let notation = if problem-kind == "Assignment" {
  [Each edge shows its assignment cost. Every assignment arc has unit capacity.]
} else if problem-kind == "Maximum flow" {
  [Each edge shows its upper capacity. Source and sink nodes are marked $S$ and $T$.]
} else {
  [Each edge shows the capacity lower and upper bounds to the left and the
   per-unit cost to the right, denoted $capcost(ell, u, c)$.]
}

#let network(selected, labels: false, ids: false) = {
  let marks = ()
  let captions = ()
  for n in vertices {
    let fill = if n.role == "source" { supply-fill }
      else if n.role == "sink" { demand-fill }
      else { gradient.radial(bg, blue, radius: 200%) }
    marks.push(node(n.pos, align(center)[#n.id], fill: fill, name: label("n" + str(n.id))))
  }
  for n in vertices.filter(n => n.badge != "") {
    marks.push(node(
      label("n" + str(n.id) + ".north-east"),
      circle(fill: fg, radius: 5pt, text(0.5em, $#n.badge$, fill: bg)),
      stroke: none, fill: none, inset: 0pt,
    ))
  }
  for a in selected {
    let from-pos = vertices.at(a.tail - 1).pos
    let to-pos = vertices.at(a.head - 1).pos
    // Stagger labels on crossing diagonals; keep vertical labels away from
    // the midpoint of long horizontal arcs that pass behind them.
    let label-pos = if from-pos.at(0) == to-pos.at(0) { 0.4 }
      else if a.bend != 0 { 0.25 }
      else if calc.abs(to-pos.at(0) - from-pos.at(0)) > 3 { 0.15 }
      else if to-pos.at(1) > from-pos.at(1) { 0.25 }
      else if to-pos.at(1) < from-pos.at(1) { 0.68 }
      else { 0.5 }
    let caption = if labels {
      if ids { [#text(0.7em)[#a.id:] #value-label(a)] } else { value-label(a) }
    } else { none }
    marks.push(edge(label("n" + str(a.tail)), label("n" + str(a.head)), "-|>",
      bend: a.bend * 1deg))
    if caption != none {
      // Draw all labels after the edges, so later crossing arcs cannot
      // strike through earlier labels. The visible edge keeps sample.typ's style.
      captions.push(edge(label("n" + str(a.tail)), label("n" + str(a.head)), "-", caption,
      stroke: none,
      bend: a.bend * 1deg, label-pos: label-pos,
      label-side: center, label-fill: bg))
    }
  }
  diagram(
    axes: (ltr, ttb),
    spacing: (5em, 3.5em),
    node-stroke: 0.8pt + fg,
    node-shape: circle,
    node-fill: gradient.radial(bg, blue, radius: 200%),
    edge-stroke: 0.6pt + fg,
    label-size: 8pt,
    ..marks, ..captions,
  )
}

#let title() = heading(level: 1)[#family: #problem-kind, #vertices.len() nodes, #arcs.len() arcs]

#let panel(index, selected) = context {
  let picture = network(selected, labels: family != "GOTO" and selected.len() <= 18)
  let width = calc.max(320pt, measure(picture).width)
  block(width: width, breakable: false)[
    #heading(level: 2)[#panels.at(index).title (#selected.len() arcs)]
    #panels.at(index).description
    #v(1em)
    #align(center, picture)
  ]
}

#if view == "overview" {
  title()
  block(width: if family == "GOTO" { 65em } else { 40em })[
    Seed: #seed. Green nodes supply flow; red nodes receive it.
    Supplies and demands appear in circles on the upper right.
    #if family == "GOTO" {
      [Every arc appears in exactly one view below. Node positions and parallel arcs are preserved.
       Capacity and cost values are provided in the companion arc details.]
    } else if arcs.len() <= 18 {
      notation
    } else {
      [Every arc is shown. Capacity and cost values are provided in the companion arc details.]
    }
  ]
  let diagrams = panels.enumerate().map(((i, _)) => panel(i, arcs.filter(a => a.group == i)))
  grid(columns: if family == "GOTO" { (auto, auto) } else { (auto,) },
    column-gutter: 3em, row-gutter: 2em, ..diagrams)
} else {
  let chunks = arcs.chunks(12)
  if chunks.len() == 0 {
    title()
    [This network has no arcs.]
  }
  for (page, selected) in chunks.enumerate() {
    if page > 0 { pagebreak() }
    title()
    block(width: 40em)[
      Seed: #seed. Arc detail #str(page + 1) of #chunks.len(): original arcs #selected.first().id–#selected.last().id.
      #notation A small arc ID precedes each edge label.
    ]
    align(center, network(selected, labels: true, ids: true))
    heading(level: 2)[Arc values]
    let headers = if problem-kind == "Assignment" {
      ([Arc], [From → to], [Cost])
    } else if problem-kind == "Maximum flow" {
      ([Arc], [From → to], [Capacity])
    } else { ([Arc], [From → to], [Lower], [Upper], [Cost]) }
    table(columns: headers.len(), stroke: 0.6pt + fg,
      table.header(..headers),
      ..selected.map(a => {
        let prefix = ([#a.id], [#a.tail → #a.head])
        if problem-kind == "Assignment" { (..prefix, [#a.cost]) }
        else if problem-kind == "Maximum flow" { (..prefix, [#a.cap]) }
        else { (..prefix, [0], [#a.cap], [#a.cost]) }
      }).flatten(),
    )
  }
}

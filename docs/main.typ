#import "@preview/lovelace:0.3.0": *
#import "@preview/showybox:2.0.4": showybox
#set heading(numbering: "1.")

// This Typst document serves as documentation for the cost_scaling_rs project.
//
// Most of the content here is a direct typst translation of
// the original paper:
// Goldberg, Andrew V. "An efficient implementation of a scaling minimum-cost flow algorithm." Journal of algorithms 22.1 (1997): 1-29.

= Core Definitions for the Minimum-Cost Flow Problem

== Network Model

Let $G = (V, E)$ be a directed graph. Each arc $a in E$ is associated with:
- a capacity $u(a) in RR$,
- a cost $c(a) in RR$.

Each node $v in V$ has a demand $d(v) in RR$.

Sometimes we refer to an arc $a$ by its endpoints, e.g., $(v, w)$. This is ambiguous if there
are several arcs from $v$ to $w$. An alternative is to refer to $v$ as the tail of $a$ and to $w$ as the
head of $a$, which is precise but inconvenient.

We assume:
$
sum_(v in V) d(v) = 0.
$

The graph is symmetric: if $a = (v, w) in E$, then $(w, v) in E$, and
$
c(v, w) = -c(w, v).
$

Let $n = |V|$, $m = |E|$, and $C = max_{a in E} c(a)$.

== Flows and Pseudoflows

A *pseudoflow* is a function $f: E -> RR$ satisfying:
$
f(a) <= u(a),  f(v, w) = -f(w, v).
$

The *excess* at node $v$ is defined as:
$
e_f (v) = [sum_((u, v) in E) f(u, v)] - d(v).
$

A node $v$ is *active* if:
$
e_f (v) > 0.
$

A pseudoflow $f$ is a *feasible flow* if:
$
forall v in V, e_f (v) = 0.
$

== Cost Function

The cost of a flow $f$ is:
$
"cost"(f) = 1/2 sum_(a in E) c(a) f(a).
$

The minimum-cost flow problem is to find a feasible flow minimizing $"cost"(f)$ (_optimal flow_).

== Residual Network

The residual capacity of arc $a$ is:
$
u_f(a) = u(a) - f(a).
$

An arc is *residual* if $u_f(a) > 0$.

The set of residual arcs is: $ E_f = \{ a in E | u_f(a) > 0 \} $

The residual graph is the graph induced by the residual arcs: $G_f = (V, E_f)$

== Price Function and Reduced Costs

A *price function* is $p: V arrow.r RR$.

The *reduced cost* of arc $(v, w)$ is:
$
c_p(v, w) = c(v, w) + p(v) - p(w).
$

An arc is *admissible* if it is a residual arc of negative reduced cost:
$
u_f(v, w) > 0  "and"  c_p(v, w) < 0.
$

The admissible graph is:
$
G_A = (V, E_A),  E_A = \{ a in E | a "is admissible" \}.
$

== Optimality Conditions

A flow $f$ is *optimal* if and only if there exists a price function $p$ such that:
$
forall a in E_f, c_p(a) >= 0.
$

== $epsilon$-Optimality

Let $epsilon > 0$. A pseudoflow $f$ is *$epsilon$-optimal* with respect to $p$ if:
$
forall a in E_f, c_p(a) >= -epsilon.
$

A pseudoflow is $epsilon$-optimal if such a price function exists.

If all data are integral and:
$
epsilon < 1/n,
$
then any $epsilon$-optimal flow is optimal.

//impl McmfGraph {
//pub fn min_cost(&mut self) {
// initialization
// epsilon <- C
// for all v: price(v) <- 0
// if exists(flow): f <- flow else return None
// while epsilon >= 1.0/n
//    (epsilon, f, price) <- refine(epsilon, f, price)
// return f
//}

//pub fn refine() {
// initialization
// epsilon <- epsilon / alpha
// for all (v, w) in self.arcs
//   if c_p(v, w) < 0.0
//     f(v, w) <- u(v,w)
// while exists(a push or a relabel operation that applies)
//   select such an operation and apply it
// return (epsilon, f, price)
//}

//pub fn push() {
// params: (v, w)
// applicability: v is active, u_f (v,w) > 0 and c_p (v,w) < 0
// action: send delta = min(e_f (v), u_f (v,w)) units os flow from v to w
//}

//pub fn relabel() {
// params: v
// applicability: v is active
//     and, for all w in arcs, u_f (v,w) > 0 => c_p(v, w) > 0
// action: replace price(v) by max_((v,w) in E_f) (price(w) - c(v,w) - epsilon)
//}

//pub fn discharge() {
// params: v
// applicability: v is active
// action: apply push/relabel operations to v until v becomes inactive
//}
//}


= The method <themethod>

#let showybox-code(..body) = showybox(frame: (
    border-color: blue.darken(50%),
    title-color: blue.lighten(60%),
    body-color: blue.lighten(80%)
  ), ..body)

#figure(
  showybox-code(
  pseudocode-list(title: smallcaps[Min-Cost($V,E,u,c$)])[
    + $epsilon <- C$
    + $forall v$, $p(v) <- 0$
    + *if* $exists$ a flow *then* $f <- "a flow"$ *else return* null
    + *while* $epsilon >= 1 / n$
      + $(epsilon,f,p) <- "refine"(epsilon,f,p)$
    + *return* $f$
  ]
  ),
  caption: [The successive approximation algorithm.]
) <mincost>



#figure(
  showybox-code(
    pseudocode-list(title: smallcaps[Refine($epsilon,f,p$)])[
  + $epsilon <- epsilon / alpha$
  + *for all* $(v,w) in E$ *do*
    + *if* $c_p (v,w) < 0$ *then*
      + $f(v,w) <- u(v,w)$
  + *while* $exists$ a _push_ or _relabel_ operation that applies
    + select such an operation and apply it
  + *return* $(epsilon,f,p)$
]
  ),
  caption: [The generic _refine_ subroutine.]
) <refine>

#figure(
  showybox-code(
pseudocode-list(title: smallcaps[Push($v,w$)])[
  - *Applicability*: $v$ is active, $u_f (v,w) > 0$, *and* $c_p (v,w) < 0$
  - *Action*: send $delta = min(e_f (v), u_f (v,w))$ units of flow from $v$ to $w$
],

pseudocode-list(title: smallcaps[Relabel($v$)])[
  - *Applicability*: $v$ is active *and* $forall w in V$ $u_f (v,w) > 0 => c_p (v,w) >= 0$
  - *Action*: replace $p(v)$ by $max_((v,w) in E_f) [p(w) - c(v,w) - epsilon]$
]
),
caption: [The _push_ and _relabel_ operations described in the figure are somewhat restrictive.
Some heuristics use different versions of these operations as discussed later.]
) <push-relabel>

#figure(

  showybox-code(
pseudocode-list(title: smallcaps[Discharge($v$)])[
  - *Applicability*: $v$ is active
  - *Action*: apply _push_/_relabel_ operations to $v$ until $v$ becomes inactive
]
),
caption: [The _discharge_ operation.]
) <discharge>

== Description

First we give a high-level description of the successive approximation algorithm (see @mincost). The algorithm maintains a flow $f$
and a price function $p$, such that $f$ is $epsilon$-optimal with respect to $p$. The algorithm starts with $epsilon = C$, with $p(v)=0$ for all $v in V$, and with any feasible flow. A feasible flow can be found using one invocation of any maximum flow algorithm. Any flow is $C$-optimal with respect to the zero price function. The main loop of the algorithm repeatedly reduces $epsilon$ by a constant factor $alpha$, the choice of which is discussed later. When $epsilon < 1 / n$, the algorithm terminates. The algorithm takes $ceil(log_alpha (n C))$ iterations.

Reducing $epsilon$ is the task of the subroutine _refine_. The input to _refine_ is $epsilon$, $f$ and $p$ such that $f$ is $epsilon$-optimal with respect to $p$. The output from _refine_ is $epsilon$ reduced by a factor of $alpha$, a new $f$, and a new $p$ such that $f$ is $epsilon$-optimal with respect to $p$.

The generic _refine_ subroutine (described in @refine) begins by decreasing the value of
$epsilon$ and saturating every arc with negative reduced cost.
This action converts the flow $f$ into an $epsilon$-optimal pseudoflow (indeed, into a $0$-optimal pseudoflow).
Then the subroutine converts the $epsilon$-optimal pseudoflow into an
$epsilon$-optimal flow by applying a sequence of _push_ and _relabel_
operations (see @push-relabel), each of which preserves $epsilon$-optimality.
The generic algorithm does not specify the order in which these
operations are applied.

A _push_ operation applied to a residual arc $(v,w)$ of negative reduced cost whose tail node $v$ is active. It consists of pushing $delta = min(e_f (v), u_f (v,w))$ units of flow from $v$ to $w$, thereby decreasing $e_f (v)$ and $f(w,v)$ by $delta$ and increasing $e_f (w)$ and $f(v,w)$ by $delta$.

A _relabel_ operation applies to an active node $v$ that has no exiting residual arcs with negative reduced cost. It consists of decreasing $p(v)$ to the smallest value allowed by the $epsilon$-optimality constraints, namely $max_((v,w) in E_f) [p(w) - c(v,w) - epsilon]$. (Alternatively, $p(v)$ can be decreased by $epsilon$.)

The generic implementation of the algorithm needs one additional data structure, a set $S$ containing all active nodes. Initially $S$ contains all nodes whose excess becomes positive during the initialization step of refine. Updating $S$ takes only $O(1)$ time per push or relabel operation. (Such an operation requires possibly deleting one node from S and adding one node to S.)

At a low level, the push and relabel operations are combined in the _discharge_ operation, described in @discharge. A discharge operation applies push and relabel operations to an active node until the node becomes inactive, i.e., its excess drops to zero. We assume the adjacency list representation of the graph and maintain a current arc pointer for every node _v_. The current arc of a node is set to its first arc initally and after each relabeling of the node. The $"discharge"(v)$ operation attempts to push flow along the current arc of $v$. If the current arc is not eligible for pushing, discharge advances the current arc pointer to the next arc on the edge list of $v$ unless the current arc is the last arc on the list, in which case $v$ is relabeled.

There remains the issue of the order in which to discharge active nodes. We implement the _first-in-first-out_ (FIFO) algorithm, which maintains the set of active nodes as a queue, repeatedly dischargin the front node on the queue and adding newly active nodes to the rear of the queue.

The worst-case theoretical bounds on the number of basic operations invoked during an execution of refine are as follows:

- The number of relabel operations is $O(n^2)$
- The number of push operations is $O(n^2 m)$ in any implementation of the generic method

A dynamic tree data structure can be used to do several push operations at once. Our experience suggests that in practice the relabel operations are the bottleneck, an the dynamic trees are not likely to help. We did not experiment with the dynamic tree version of the algorithm.

= Heuristic improvements

In this section we discuss heuristics used in our implementation. These
heuristics improve the typical running time of the algorithm and do not
increase the asymptotic worst-case time bound.

We have 4 heuristics:
+ Price updates
+ Price refinement
+ Arc fixing
+ Push lookahead

== Price updates

The push-relabel method modifies prices locally, one node at a time. _Price update_ heuristics modify prices in a more global way. In the maximum flow context, price updates, implemented using breadth-first search, have been shown to significantly improve practical performance of the push-relabel method. This heuristic does not help much on some problem classes, but results in asymptotic performance improvement on other classes.

The idea of price updates in the minimum cost flow context had been introduced in @goldberg1990finding. These updates, however, need to be done in such a way that the price function after an update is "better" than the price function before the update. In particular, the number of push and relabel operations should decrease even if the updates are performed infrequently. Our implementation is the first one to achieve this. The implementation uses
the techniques introduced in @goldberg1992implementing, @goldberg1995scaling.

The price update heuristic is based on the _set-relabel_ operation, which is defined as follows. Let $S$ be a set of nodes such that $S$ contains all nodes with negative excess, and $overline(S)$, the complement of $S$, contains at least one node with positive excess. Suppose that no admissible arc goes from a node in $overline(S)$ to a node in $S$. The set-relabel operation reduces the price of every node in $overline(S)$ by $epsilon$.

It can be shown that the set-relabel operation satisfies the following conditions (see @goldberg1995scaling)

+ $epsilon$-optimality is preserved
+ the admissible graph remains acyclic
+ prices are monotonically decreasing
+ prices of nodes with negative excess remain unchanged

These facts imply the $O(n^2)$ bound on the number of relabels per refine; in fact they imply that each node participates in $O(n)$ relabels and set-relabels per refine.

The set-relabel operation can be applied in the following way. Initially, the set $S$ contains a set of all nodes with negative excess. At each iteration, the set $S$ is extended to include all nodes from which a node in $S$ is reachable in the admissible graph. If all nodes with positive excess are in $S$, the computation terminates. If not, set-relabel is applied to $S$ and the next iteration begins. This computation is implemented using buckets in a way similar to that of Dial’s implementation @dial1969algorithm of Dijkstra’s shortest path algorithm, as described in @goldberg1995scaling.

Our implementation of the price update heuristic maintains an array $B$ of buckets. Each node is in at most one bucket. Define $B(v)$ to be index of the bucket containing $v$, or $infinity$ if $v$ is no bucket. Initially all buckets except bucket zero are empty, bucket zero contains all nodes with negative excess, and current bucket index $i$ is set to zero. At every step, a node is removed
from the current bucket and scanned, or if the current bucket is empty the current bucket index is incremented. The scan of a node $v$ examines all arcs $(v,w)$ , and if $w$ has not been scanned yet and $k = floor(c_p (v,w) / epsilon) + 1$ is less than $B(v)$, then $v$ is removed from its bucket (if any) and inserted into bucket $k$. Immediately after $v$ has been scanned, its label $l(v)$ is set to $i$ and $v$ is added to the set $S$ of scanned nodes. The process is continued until all nodes with positive excess have been scanned. Then the prices of all scanned nodes $v$ are reduced by $epsilon l(v)$.

Since during refine node prices change by $O(n epsilon)$ @goldberg1990finding, $O(n epsilon)$ buckets are sufficient for the price update implementation: the nodes in the buckets with high indices are never examined, so there is no need to put them in the buckets.

It is easy to see that this implementation of price updates is equivalent to the procedure described above, and that it works in time linear in the size of subgraph examined by it. If $Omega(n)$ relabels take place before each price refine, the total cost of the latter operations during an execution of the algorithm is $O(n m log(n C))$.

The ideal frequency of performing the global price updates is implementation and problem dependent. A good starting point is to perform the updates after every $n$ relabels, and then experiment. Our implementation uses a slightly different strategy of using a linear combination of the number of relabels and the number of passes over the node queue (instead of just the number of relabels) to trigger global price updates. More precisely, a global update is performed when $rho r + pi q > n$, where $rho$ and $pi$ are constants and $r$ and $q$ are the numbers of relabels and queue passes from the previous price update (or from the beginning of refine if no price updates were performed). This price update strategy makes price updates more frequent when the number of active nodes is small (toward the end of refine).

== Price refinement

// Note: "slash" means division, so $a / b$ = $a slash b$. It's just for inline formatting in typst.

As suggested in @goldberg1990finding, refine may produce a solution which is not only $epsilon$-optimal, but also $(epsilon slash alpha)$-optimal. In fact, refine may produce an optimal flow even for $epsilon > 1 / n$. Our implementation uses the _price refine heuristic_. This heuristic decreases $epsilon$ and does not change the flow $f$ while modifying $p$ in an attempt to find $p$ such that $f$ is $epsilon$-optimal with respect to $p$. The heuristic is applied every time $epsilon$ is decreased by the algorithm, and also at the beginning of the algorithm if the zero flow in the input network is feasible.

Our implementation of the price refine heuristic is similar to a version of the scaling shortest path algorithm @goldberg1995scaling. The implementation uses the
cut-relabel operation implicitly. The description below provides implementation details.

The price refine heuristic starts with a flow $f$ which is $(alpha epsilon)$-optimal with respect to a price function $p$ and attempts to modify $p$ so that $f$ is $epsilon$-optimal with respect to the modified $p$. The implementation works in passes and maintains an array of buckets $B$. At the beginning of each pass $B$ is empty. The pass starts by topologically sorting the current admissible graph. If the graph contains a cycle, the computation terminates (failing to produce the desired price function).

Otherwise the admissible graph is acyclic, and we compute approximate distances $d'$ with respect to $c_p$ in the admissible graph from the zero in-degree nodes. The approximate distances $d'$ are measured in units of $epsilon$. The distances are nonpositive integers. Initially, $d'=0$ for all $v$. We scan nodes in topological order. A node $v$ is scanned by examining its arcs $(v,w)$. If $d'(w) > d'(v) + ceil(c_p (v,w) slash epsilon)$, then we set $d'(w) = d'(v) + ceil(c_p (v,w) slash epsilon)$. After the distances are computed, each node $v$ is placed into the bucket $-d'(v)$.

//GPT
Next we go through the buckets in the decreasing order, removing nodes from the current bucket and scanning them until the bucket is empty. A node $v$ is scanned by examining its arcs $(v,w)$. If $w$ has not been scanned, we may update $d'(w)$ (and move $w$ to the bucket $-d'(w)$). If $c_p (v,w) < 0$ and $d'(w) > d'(v)$, we set $d'(w) = d'(v)$. If $c_p (v,w) >= 0$ and $d'(w) > d'(v) + ceil(c_p (v,w) slash epsilon)$, we set $d'(w) = d'(v) + ceil(c_p (v,w) slash epsilon)$.

At the end of a pass, the price of every node $v$ is updated as follows: $p(v) = p(v) + d'(v)$. If the current solution is $epsilon$-optimal, the heuristic terminates. Otherwise, a new pass begins.

Clearly a pass takes linear time. Since a price of at least one node decreases by at least $epsilon$ during a pass, the number of passes is $O(n)$. Thus, one price refine computation takes $O(n m)$ time. This bound does not exceed the bound for refine, so the asymptotic running time of the algorithm does not increase by more than a constant factor.

A price refine computation fails if an admissible cycle is created during the computation and succeeds otherwise. In the latter case $epsilon$ is decreased again or, if $epsilon$ is small enough, the algorithm terminates. In the former case, one can either apply refine to decrease $epsilon$ or contract admissible cycles and finish the shortest paths computation, then undo the contractions and apply refine. In our experience, the former alternative works better.

Our way of implementing the price refine heuristic has several advantages. One advantage is that the work done during price refinement is not lost even in the case of failure: the number of push and relabel operations in the subsequent refine usually decreases enough to pay for the price refinement. A possible reason for this is that the admissible graph is acyclic after a refine, but has cycles after a price refine fails. These cycles are saturated at the beginning of the subsequent execution of refine. The second advantage is that if an optimal flow is computed at some point of the algorithm, refine is never again applied and the computation is completed by using the scaling shortest paths algorithm. This saves time because price refine is faster than refine. In practice, several last iterations of the algorithm do not apply refine.

== Arc fixing

The arc fixing heuristic involves "deleting" some arcs from the graph, thus reducing the number of times the algorithm examines an arc. The version of this heuristic that we use is a modification of that used in @goldberg1992implementing.

The theoretical justification of this technique is as follows @goldberg1990finding, @tardos1985strongly: if the current flow is $epsilon$-optimal and the absolute value of an arc cost exceeds $2 n epsilon$, the push-relabel method will not change the flow on this arc. Thus the arc does not need to be examined until the end of the algorithm. Arc fixing can be done after every execution of refine.

In the dual context, arc fixing corresponds to edge contraction. Fujishige et al. @fujishige1991speculative propose contracting edges earlier than the theory suggests. We use this idea in the primal context and call the resulting heuristic _speculative arc fixing_.

This heuristic fixes all arcs with the absolute value of reduced cost greater than $beta$, where $beta$ is a parameter that depends on the input. Fixed arcs are examined by refine very infrequently. The arcs with the current reduced cost absolute values of $beta$ or below are unfixed. Also, fixed arcs violating complimentary slackness are unfixed and saturated. In this case, we say that a _fix-in_ occurred.

A proper choice of $beta$ is important. The smaller $beta$ is, the fewer arcs refine and price refine have to deal with, so they run faster. If $beta$ is too small, however, fix-ins happen often and refine and takes more time.

In practice, $beta$ is set below the theoretically justified value of $2 n epsilon$. This has bad side effects which come up during refine (although extremely rarely for a proper choice of $beta$). The first side effect is that after an arc is fixed, the problem may become infeasible, i.e., there may be no feasible flow consistent with the fixed arc flow value. The second side effect is that during a price update, more buckets may be needed than the theory suggests. Implementations of refine and price update need to detect such situations and unfix some of the fixed arcs.

== Push lookahead

The following scenario seems to be common in practice. Consider two nodes, $v$ and $w$, such that $e_f(v) > 0$ and $e_f(w) >= 0$. Suppose $v$ pushes flow to $w$, and the first time flow is pushed from $w$ afterward, this flow is pushed back to $v$. Observe that this can happen only if $w$ does not have any outgoing admissible arcs just before the flow is pushed into it. Intuitively, the work done during the two pushes is wasted.

Such a situation can be avoided using the _lookahead_ heuristic, introduced in @goldberg1992implementing: before pushing flow to a node $w$, check whether $w$ has an outgoing admissible arc or whether $e_f(w) < 0$. If this is so, do the pushing; if not, relabel $w$. A technical difficulty is that $w$ may be inactive and the relabel operation, as described in @themethod, may not apply. In this case, either a node with negative excess is reachable from $w$ or no such node is reachable. In the former case, the method remains correct if relabel is applied to $w$ by the same argument as that presented in [24] for active nodes. In the latter case, one can show that relabel still can be applied to $w$ except when $w$ has no outgoing residual arcs. If $w$ has no outgoing arcs, the price of $w$ can be decreased by an arbitrary amount without violating $epsilon$-optimality. For example, we can decrease the price of $w$ by $epsilon$. Alternatively, we can decrease the price by a large enough amount so that all arcs adjacent to $w$ will be fixed.

We use the lookahead heuristic in our implementation. This heuristic reduces the number of pushes significantly; in many cases, the number of pushes falls below the number of relabels. See @goldberg1992implementing for more detail.

#bibliography("works.bib")

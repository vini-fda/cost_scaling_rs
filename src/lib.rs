//! CS2 min-cost max-flow scaling algorithm.
//!
//! This is a Rust implementation of the CS2 min-cost-max-flow scaling algorithm,
//! translated from the original C implementation.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[doc(hidden)]
pub mod goto;
#[doc(hidden)]
pub mod parser;

pub use parser::ParseError;

// ---------------------------------------------------------------------------
// Index types
// ---------------------------------------------------------------------------

type NodeIndex = usize;
type ArcIndex = usize;
type BucketIndex = usize;
/// Arc cost type (signed 64-bit integer).
pub type Price = i64;
/// Node supply/demand type (signed 64-bit integer).
pub type Excess = i64;

/// Sentinel value representing a null/invalid index (replaces NULL pointers).
const NONE: usize = usize::MAX;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_64: i64 = i64::MAX;
const MAX_32: i64 = i32::MAX as i64;

const PRICE_MAX: Price = MAX_64;

// Parameters
const UPDT_FREQ: f64 = 0.4;
const UPDT_FREQ_S: f64 = 30.0;
const SCALE_DEFAULT: f64 = 12.0;
/// PRICE_OUT_START may not be less than 1
const PRICE_OUT_START: u64 = 1;
const CUT_OFF_POWER: f64 = 0.44;
const CUT_OFF_COEF: f64 = 1.5;
const CUT_OFF_POWER2: f64 = 0.75;
const CUT_OFF_COEF2: f64 = 1.0;
const CUT_OFF_GAP: f64 = 0.8;
const CUT_OFF_MIN: f64 = 12.0;
const CUT_OFF_INCREASE: f64 = 4.0;

const TIME_FOR_PRICE_IN1: i32 = 2;
const TIME_FOR_PRICE_IN2: i32 = 4;
const TIME_FOR_PRICE_IN3: i32 = 6;

const MAX_CYCLES_CANCELLED: i32 = 0;
const START_CYCLE_CANCEL: u64 = 100;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Node coloring for DFS traversal.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Color {
    /// Undiscovered node.
    White,
    /// Node is on the current DFS stack (in progress).
    Grey,
    /// All outgoing arcs have been fully explored.
    Black,
}

/// A node in the min-cost flow network.
#[derive(Clone)]
struct Node {
    /// First outgoing arc index.
    first: ArcIndex,
    /// Current outgoing arc index.
    current: ArcIndex,
    /// First suspended arc index.
    suspended: ArcIndex,
    /// Excess of the node.
    excess: Excess,
    /// Distance from a sink (node potential).
    price: Price,
    /// Next node in push-queue.
    q_next: NodeIndex,
    /// Parent node in DFS traversal (used by price_refine and compute_prices).
    dfs_parent: NodeIndex,
    /// DFS visit color (White/Grey/Black) used in price_refine and compute_prices.
    inp: Color,
}

/// An arc in the min-cost flow network.
#[derive(Clone)]
struct Arc {
    /// Residual capacity.
    res_capacity: i64,
    /// Cost of the arc.
    cost: Price,
    /// Head node index.
    head: NodeIndex,
    /// Opposite (sister) arc index.
    sister: ArcIndex,
}

/// Dial-style bucket array for priority-queue operations in price_update and
/// price_refine. Stores per-node linked-list pointers and rank in parallel
/// arrays, separate from the `Node` struct.
///
/// Note: "Dial" as in Robert B. Dial's "Algorithm 360: Shortest-path forest with topological ordering"
struct BucketArray {
    /// Head of each bucket's doubly-linked list (`NONE` if empty).
    p_first: Vec<NodeIndex>,
    /// Next node in the same bucket (`NONE` if last).
    b_next: Vec<NodeIndex>,
    /// Previous node in the same bucket.
    b_prev: Vec<NodeIndex>,
    /// Bucket index this node belongs to, or -1 if settled.
    rank: Vec<i64>,
}

impl BucketArray {
    /// Creates an empty bucket array with no allocations.
    fn empty() -> Self {
        BucketArray {
            p_first: Vec::new(),
            b_next: Vec::new(),
            b_prev: Vec::new(),
            rank: Vec::new(),
        }
    }

    /// Creates a new bucket array with `num_buckets` buckets and capacity for
    /// `num_nodes` nodes.
    fn new(num_buckets: usize, num_nodes: usize) -> Self {
        BucketArray {
            p_first: vec![NONE; num_buckets],
            b_next: vec![NONE; num_nodes],
            b_prev: vec![NONE; num_nodes],
            rank: vec![0; num_nodes],
        }
    }

    /// Returns true if bucket `b` is non-empty.
    fn nonempty(&self, b: BucketIndex) -> bool {
        self.p_first[b] != NONE
    }

    /// Reset bucket `b` to empty.
    #[allow(dead_code)]
    fn reset(&mut self, b: BucketIndex) {
        self.p_first[b] = NONE;
    }

    /// Insert node `i` into bucket `b`.
    fn insert(&mut self, i: NodeIndex, b: BucketIndex) {
        let old_first = self.p_first[b];
        self.b_next[i] = old_first;
        if old_first != NONE {
            self.b_prev[old_first] = i;
        }
        self.p_first[b] = i;
    }

    /// Pop the first node from bucket `b`.
    fn get(&mut self, b: BucketIndex) -> NodeIndex {
        let i = self.p_first[b];
        self.p_first[b] = self.b_next[i];
        i
    }

    /// Remove node `i` from bucket `b`.
    fn remove(&mut self, i: NodeIndex, b: BucketIndex) {
        if i == self.p_first[b] {
            self.p_first[b] = self.b_next[i];
        } else {
            let prev = self.b_prev[i];
            let next = self.b_next[i];
            self.b_next[prev] = next;
            if next != NONE {
                self.b_prev[next] = prev;
            }
        }
    }
}

/// The update flag.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum UpdateFlag {
    /// Update is ok: we can continue.
    Ok,
    /// Update failed, some sources are unreachable: either the
    /// problem is unfeasible or you have to return suspended arcs.
    Failed,
}

/// Fatal error condition that terminates the CS2 solver.
#[derive(Clone, Copy, Debug)]
pub enum Cs2Error {
    /// The problem is infeasible (unbalanced or unreachable nodes).
    Infeasible,
    /// Price values overflowed numerical limits.
    PriceOverflow,
}

/// CS2 min-cost max-flow solver.
///
/// Solves the minimum-cost maximum-flow problem on a directed network using
/// the cost-scaling successive approximation method.
///
/// # Building a solver
///
/// **From a DIMACS `.min` file:** use [`from_dimacs_file`](Self::from_dimacs_file).
///
/// **From a DIMACS `.min` string:**
///
/// ```
/// use cost_scaling_rs::McmfCs2;
///
/// let input = "p min 4 5\n\
///              n 1 4\n\
///              n 4 -4\n\
///              a 1 2 0 4 2\n\
///              a 1 3 0 2 2\n\
///              a 2 3 0 2 1\n\
///              a 2 4 0 3 3\n\
///              a 3 4 0 5 1\n";
/// let solver = McmfCs2::from_dimacs(input).unwrap();
/// let solution = solver.min_cost(false, false).unwrap();
/// assert!(solution.objective_cost > 0.0);
/// ```
///
/// **Programmatically:**
///
/// ```
/// use cost_scaling_rs::McmfCs2;
///
/// let mut solver = McmfCs2::new(4, 5);
///
/// // Set supply (+) and demand (-) BEFORE adding arcs.
/// solver.set_supply_demand_of_node(1, 4);   // source
/// solver.set_supply_demand_of_node(4, -4);  // sink
///
/// // Add arcs: (tail, head, lower_bound, upper_bound, cost)
/// solver.set_arc(1, 2, 0, 4, 2);
/// solver.set_arc(1, 3, 0, 2, 2);
/// solver.set_arc(2, 3, 0, 2, 1);
/// solver.set_arc(2, 4, 0, 3, 3);
/// solver.set_arc(3, 4, 0, 5, 1);
///
/// let solution = solver.min_cost(false, false).unwrap();
///
/// for (tail, head, flow) in solution.flows() {
///     if flow > 0 {
///         println!("  {tail} -> {head}: {flow}");
///     }
/// }
/// ```
pub struct McmfCs2 {
    /// Number of nodes.
    n: usize,
    /// Number of arcs.
    m: usize,

    /// Array containing original capacities.
    cap: Vec<i64>,
    /// Array of nodes.
    nodes: Vec<Node>,
    /// Sentinel node index (one past last real node).
    sentinel_node: NodeIndex,
    /// First node in push-queue.
    excq_first: NodeIndex,
    /// Last node in push-queue.
    excq_last: NodeIndex,
    /// Array of arcs.
    arcs: Vec<Arc>,
    /// Sentinel arc index (one past last real arc).
    sentinel_arc: ArcIndex,

    /// Dial-style bucket array for price_update / price_refine.
    buckets: BucketArray,
    /// Number of buckets (also used as the "infinity" rank).
    linf: usize,
    time_for_price_in: i32,

    /// Quality measure (epsilon for epsilon-optimality).
    epsilon: Price,
    /// Cost multiplier = number of nodes + 1.
    dn: Price,
    /// Lowest bound for prices.
    price_min: Price,
    /// Multiplied maximal cost.
    mmc: Price,
    /// Scale factor.
    f_scale: f64,
    /// Multiplier to produce cut_on and cut_off from n and epsilon.
    cut_off_factor: f64,
    /// The bound for returning suspended arcs.
    cut_on: f64,
    /// The bound for suspending arcs.
    cut_off: f64,
    /// Total excess.
    total_excess: Excess,

    /// Signal to start price-in ASAP — maybe there is infeasibility
    /// because of suspended arcs.
    flag_price: i32,
    /// If Failed — update failed, some sources are unreachable: either the
    /// problem is unfeasible or you have to return suspended arcs.
    flag_updt: UpdateFlag,
    /// Maximal number of cycles cancelled during price refine.
    snc_max: i32,

    /// Index of dummy node in `nodes` (used as excess queue marker).
    dummy_node: NodeIndex,

    /// Number of relabels from last price update.
    n_rel: u64,
    /// Current number of refines.
    n_ref: u64,
    /// Current number of nodes with excess.
    n_src: u64,
    n_push: u64,
    n_relabel: u64,
    n_discharge: u64,
    n_refine: u64,
    n_update: u64,
    n_scan: u64,
    n_prscan: u64,
    n_prscan1: u64,
    n_prscan2: u64,
    n_bad_pricein: u64,
    n_bad_relabel: u64,
    n_prefine: u64,

    /// Finds an optimal flow with no zero-cost cycles.
    no_zero_cycles: bool,
    /// Print the answer?
    print_ans: bool,
    /// Per-node supply/demand balance.
    node_balance: Vec<i64>,

    // -- sketch variables used during reading in arcs --
    /// Minimal node id.
    node_min: usize,
    /// Maximal node id.
    node_max: usize,
    /// Internal array for holding node degree / position of first outgoing arc.
    arc_first: Vec<i64>,
    /// Internal array: tails of the arcs.
    arc_tail: Vec<usize>,
    /// Current position while building arcs.
    pos_current: usize,
    /// Current arc index during construction.
    arc_current: ArcIndex,
    /// Maximum cost.
    max_cost: Price,
    /// Total supply.
    total_p: Excess,
    /// Total demand.
    total_n: Excess,
    /// Pointer to source node during arc construction.
    i_node: NodeIndex,
    /// Pointer to target node during arc construction.
    j_node: NodeIndex,
}

// ---------------------------------------------------------------------------
// Helper functions (replacing C macros)
// ---------------------------------------------------------------------------

/// Returns the 1-based external id for a node index, or -1 if `NONE`.
fn n_node(i: NodeIndex, node_min: usize) -> i64 {
    if i == NONE { -1 } else { (i + node_min) as i64 }
}

// ---------------------------------------------------------------------------
// Default constructors
// ---------------------------------------------------------------------------

impl Default for Node {
    fn default() -> Self {
        Node {
            first: NONE,
            current: NONE,
            suspended: NONE,
            excess: 0,
            price: 0,
            q_next: NONE,
            dfs_parent: NONE,
            inp: Color::White,
        }
    }
}

impl Default for Arc {
    fn default() -> Self {
        Arc {
            res_capacity: 0,
            cost: 0,
            head: NONE,
            sister: NONE,
        }
    }
}

impl From<parser::DimacsMin> for McmfCs2 {
    fn from(problem: parser::DimacsMin) -> Self {
        let mut solver = McmfCs2::new(problem.nodes as usize, problem.arcs_count as usize);
        // Node supply/demand must be set before arcs, because set_arc adjusts
        // excess for nonzero lower bounds (excess -= low for tail, excess += low
        // for head). Setting nodes after arcs would overwrite those adjustments.
        for node in &problem.node_descs {
            solver.set_supply_demand_of_node(node.id as usize, node.supply);
        }
        for arc in &problem.arcs {
            solver.set_arc(
                arc.from as usize,
                arc.to as usize,
                arc.min_cap,
                arc.max_cap,
                arc.cost,
            );
        }
        solver
    }
}

// ---------------------------------------------------------------------------
// McmfCs2 implementation
// ---------------------------------------------------------------------------

impl McmfCs2 {
    /// Create a new solver for a network with `num_nodes` nodes and `num_arcs` arcs.
    pub fn new(num_nodes: usize, num_arcs: usize) -> Self {
        let mut solver = McmfCs2 {
            n: num_nodes,
            m: num_arcs,

            cap: Vec::new(),
            nodes: Vec::new(),
            sentinel_node: NONE,
            excq_first: NONE,
            excq_last: NONE,
            arcs: Vec::new(),
            sentinel_arc: NONE,

            buckets: BucketArray::empty(),
            linf: 0,
            time_for_price_in: 0,

            epsilon: 0,
            dn: 0,
            price_min: 0,
            mmc: 0,
            f_scale: 0.0,
            cut_off_factor: 0.0,
            cut_on: 0.0,
            cut_off: 0.0,
            total_excess: 0,

            flag_price: 0,
            flag_updt: UpdateFlag::Ok,
            snc_max: 0,

            dummy_node: NONE,

            n_rel: 0,
            n_ref: 0,
            n_src: 0,
            n_push: 0,
            n_relabel: 0,
            n_discharge: 0,
            n_refine: 0,
            n_update: 0,
            n_scan: 0,
            n_prscan: 0,
            n_prscan1: 0,
            n_prscan2: 0,
            n_bad_pricein: 0,
            n_bad_relabel: 0,
            n_prefine: 0,

            no_zero_cycles: false,
            print_ans: true,
            node_balance: Vec::new(),

            node_min: 0,
            node_max: 0,
            arc_first: Vec::new(),
            arc_tail: Vec::new(),
            pos_current: 0,
            arc_current: NONE,
            max_cost: 0,
            total_p: 0,
            total_n: 0,
            i_node: NONE,
            j_node: NONE,
        };
        solver.allocate_arrays();
        solver
    }

    /// Parse a DIMACS `.min` format string and construct a solver.
    ///
    /// This is the easiest way to create a solver from problem data.
    /// See the [DIMACS format](http://lpsolve.sourceforge.net/5.5/DIMACS_mcf.htm) for details.
    ///
    /// # Errors
    ///
    /// Returns a [`parser::ParseError`] if the input is malformed.
    pub fn from_dimacs(input: &str) -> Result<Self, ParseError> {
        let problem = parser::parse(input)?;
        Ok(Self::from(problem))
    }

    /// Read a DIMACS `.min` file from disk and construct a solver.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or the contents are malformed.
    pub fn from_dimacs_file<P: AsRef<std::path::Path>>(
        path: P,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let input = std::fs::read_to_string(path)?;
        let solver = Self::from_dimacs(&input)?;
        Ok(solver)
    }

    // -----------------------------------------------------------------------
    // Flow / capacity helpers
    // -----------------------------------------------------------------------

    /// Push `df` units of flow from node `i` to node `j` along arc `a`.
    ///
    /// This is the "push" in the push-relabel method.
    fn increase_flow(&mut self, i: NodeIndex, j: NodeIndex, a: ArcIndex, df: i64) {
        self.nodes[i].excess -= df;
        self.nodes[j].excess += df;
        self.arcs[a].res_capacity -= df;
        let sister = self.arcs[a].sister;
        self.arcs[sister].res_capacity += df;
        self.n_push += 1;
    }

    /// Returns true when it is time for a price update.
    fn time_for_update(&self) -> bool {
        self.n_rel as f64 > self.n as f64 * UPDT_FREQ + self.n_src as f64 * UPDT_FREQ_S
    }

    // -----------------------------------------------------------------------
    // Excess queue utilities
    // -----------------------------------------------------------------------

    /// Reset the excess queue, marking all nodes as out-of-queue.
    fn reset_excess_q(&mut self) {
        while self.excq_first != NONE {
            let next = self.nodes[self.excq_first].q_next;
            self.nodes[self.excq_first].q_next = self.sentinel_node;
            self.excq_first = next;
        }
        self.excq_last = NONE;
    }

    /// Returns true if node `i` is not in the excess queue.
    fn out_of_excess_q(&self, i: NodeIndex) -> bool {
        self.nodes[i].q_next == self.sentinel_node
    }

    /// Returns true if the excess queue is empty.
    fn empty_excess_q(&self) -> bool {
        self.excq_first == NONE
    }

    /// Returns true if the excess queue is non-empty.
    fn nonempty_excess_q(&self) -> bool {
        self.excq_first != NONE
    }

    /// Insert node `i` at the back of the excess queue.
    fn insert_to_excess_q(&mut self, i: NodeIndex) {
        if self.nonempty_excess_q() {
            self.nodes[self.excq_last].q_next = i;
        } else {
            self.excq_first = i;
        }
        self.nodes[i].q_next = NONE;
        self.excq_last = i;
    }

    /// Remove the front node from the excess queue. Returns the removed node index.
    fn remove_from_excess_q(&mut self) -> NodeIndex {
        let i = self.excq_first;
        self.excq_first = self.nodes[i].q_next;
        self.nodes[i].q_next = self.sentinel_node;
        if self.excq_first == NONE {
            self.excq_last = NONE;
        }
        i
    }

    // -----------------------------------------------------------------------
    // Stack-queue utilities (excess queue used as a stack)
    // -----------------------------------------------------------------------

    /// Returns true if the stack-queue is non-empty.
    fn nonempty_stackq(&self) -> bool {
        self.nonempty_excess_q()
    }

    /// Reset the stack-queue.
    fn reset_stackq(&mut self) {
        self.reset_excess_q();
    }

    /// Push node `i` onto the stack-queue.
    fn stackq_push(&mut self, i: NodeIndex) {
        self.nodes[i].q_next = self.excq_first;
        self.excq_first = i;
    }

    /// Pop the front node from the stack-queue. Returns the popped node index.
    fn stackq_pop(&mut self) -> NodeIndex {
        self.remove_from_excess_q()
    }

    // -----------------------------------------------------------------------
    // Misc utilities
    // -----------------------------------------------------------------------

    /// Update the cut-off factor and bounds based on bad price-in / relabel counts.
    fn update_cut_off(&mut self) {
        if self.n_bad_pricein + self.n_bad_relabel == 0 {
            self.cut_off_factor = CUT_OFF_COEF2 * (self.n as f64).powf(CUT_OFF_POWER2);
            if self.cut_off_factor < CUT_OFF_MIN {
                self.cut_off_factor = CUT_OFF_MIN;
            }
            self.cut_off = self.cut_off_factor * self.epsilon as f64;
            self.cut_on = self.cut_off * CUT_OFF_GAP;
        } else {
            self.cut_off_factor *= CUT_OFF_INCREASE;
            self.cut_off = self.cut_off_factor * self.epsilon as f64;
            self.cut_on = self.cut_off * CUT_OFF_GAP;
        }
    }

    /// Exchange the contents of arcs `a` and `b`, updating sister pointers
    /// and capacities accordingly.
    fn exchange(&mut self, a: ArcIndex, b: ArcIndex) {
        if a != b {
            let sa = self.arcs[a].sister;
            let sb = self.arcs[b].sister;

            // Save arc a into temporaries.
            let d_rez = self.arcs[a].res_capacity;
            let d_cost = self.arcs[a].cost;
            let d_head = self.arcs[a].head;

            // Copy b -> a.
            self.arcs[a].res_capacity = self.arcs[b].res_capacity;
            self.arcs[a].cost = self.arcs[b].cost;
            self.arcs[a].head = self.arcs[b].head;

            // Copy saved a -> b.
            self.arcs[b].res_capacity = d_rez;
            self.arcs[b].cost = d_cost;
            self.arcs[b].head = d_head;

            if a != sb {
                self.arcs[b].sister = sa;
                self.arcs[a].sister = sb;
                self.arcs[sa].sister = b;
                self.arcs[sb].sister = a;
            }

            // Swap capacities.
            self.cap.swap(a, b);
        }
    }

    /// Allocate internal arrays and prepare for receiving arcs.
    fn allocate_arrays(&mut self) {
        self.nodes = vec![Node::default(); self.n + 2];
        self.arcs = vec![Arc::default(); 2 * self.m + 1];
        self.cap = vec![0i64; 2 * self.m];
        self.arc_tail = vec![0usize; 2 * self.m];
        self.arc_first = vec![0i64; self.n + 2];

        self.pos_current = 0;
        self.arc_current = 0;
        self.node_max = 0;
        self.node_min = self.n;
        self.max_cost = 0;
        self.total_p = 0;
        self.total_n = 0;
    }

    /// Add a directed arc from `tail_node_id` to `head_node_id` with the given bounds and cost.
    pub fn set_arc(
        &mut self,
        tail_node_id: usize,
        head_node_id: usize,
        low_bound: i64,
        mut up_bound: i64,
        cost: Price,
    ) {
        assert!(
            tail_node_id <= self.n && head_node_id <= self.n,
            "Arc with head or tail out of bounds"
        );
        if up_bound < 0 {
            up_bound = MAX_32;
            println!("Warning: Infinite capacity replaced by BIGGEST_FLOW");
        }
        assert!(
            low_bound >= 0 && low_bound <= up_bound,
            "Wrong capacity bounds"
        );

        self.arc_first[tail_node_id + 1] += 1;
        self.arc_first[head_node_id + 1] += 1;
        self.i_node = tail_node_id;
        self.j_node = head_node_id;

        let pc = self.pos_current;
        let ac = self.arc_current;

        self.arc_tail[pc] = tail_node_id;
        self.arc_tail[pc + 1] = head_node_id;
        self.arcs[ac].head = head_node_id;
        self.arcs[ac].res_capacity = up_bound - low_bound;
        self.cap[pc] = up_bound;
        self.arcs[ac].cost = cost;
        self.arcs[ac].sister = ac + 1;
        self.arcs[ac + 1].head = tail_node_id;
        self.arcs[ac + 1].res_capacity = 0;
        self.cap[pc + 1] = 0;
        self.arcs[ac + 1].cost = -cost;
        self.arcs[ac + 1].sister = ac;

        self.nodes[tail_node_id].excess -= low_bound;
        self.nodes[head_node_id].excess += low_bound;

        if head_node_id < self.node_min {
            self.node_min = head_node_id;
        }
        if tail_node_id < self.node_min {
            self.node_min = tail_node_id;
        }
        if head_node_id > self.node_max {
            self.node_max = head_node_id;
        }
        if tail_node_id > self.node_max {
            self.node_max = tail_node_id;
        }

        let abs_cost = cost.abs();
        if abs_cost > self.max_cost && up_bound > 0 {
            self.max_cost = abs_cost;
        }

        self.arc_current += 2;
        self.pos_current += 2;
    }

    /// Set the supply (positive) or demand (negative) of a node. Must be called before [`set_arc`](Self::set_arc).
    pub fn set_supply_demand_of_node(&mut self, id: usize, excess: Excess) {
        assert!(id <= self.n, "Node id out of bounds");
        self.nodes[id].excess = excess;
        if excess > 0 {
            self.total_p += excess;
        }
        if excess < 0 {
            self.total_n -= excess;
        }
    }

    /// Reorders arcs so each node's outgoing arcs are contiguous, then shifts
    /// node indices to be zero-based.
    ///
    /// Uses `arc_first` as a prefix-sum array to compute the position of each
    /// node's arc block, then permutes arcs in-place (swapping heads, costs,
    /// capacities, and sister pointers) until every arc sits in its owner's
    /// block. Frees the temporary `arc_first` and `arc_tail` arrays afterward.
    ///
    /// Must be called exactly once, after all arcs have been added via
    /// [`set_arc`](Self::set_arc) and before [`cs2_initialize`](Self::cs2_initialize).
    fn pre_processing(&mut self) {
        assert!(
            (self.total_p - self.total_n).abs() == 0,
            "Unbalanced problem"
        );

        // first arc from the first node
        self.nodes[self.node_min].first = 0;

        // prefix-sum: arc_first[i] becomes position of first outgoing arc from node i
        for i in (self.node_min + 1)..=(self.node_max + 1) {
            self.arc_first[i] += self.arc_first[i - 1];
            self.nodes[i].first = self.arc_first[i] as usize;
        }

        // reorder arcs by source node
        for i in self.node_min..self.node_max {
            let last = self.nodes[i + 1].first;
            let mut arc_num = self.arc_first[i] as usize;
            while arc_num < last {
                let mut tail_node_id = self.arc_tail[arc_num];
                while tail_node_id != i {
                    let arc_new_num = self.arc_first[tail_node_id] as usize;

                    // swap heads
                    let tmp = self.arcs[arc_new_num].head;
                    self.arcs[arc_new_num].head = self.arcs[arc_num].head;
                    self.arcs[arc_num].head = tmp;

                    // swap caps
                    self.cap.swap(arc_new_num, arc_num);

                    // swap rez_capacity
                    let tmp = self.arcs[arc_new_num].res_capacity;
                    self.arcs[arc_new_num].res_capacity = self.arcs[arc_num].res_capacity;
                    self.arcs[arc_num].res_capacity = tmp;

                    // swap cost
                    let tmp = self.arcs[arc_new_num].cost;
                    self.arcs[arc_new_num].cost = self.arcs[arc_num].cost;
                    self.arcs[arc_num].cost = tmp;

                    // swap sisters
                    if arc_new_num != self.arcs[arc_num].sister {
                        let tmp = self.arcs[arc_new_num].sister;
                        self.arcs[arc_new_num].sister = self.arcs[arc_num].sister;
                        self.arcs[arc_num].sister = tmp;

                        let s1 = self.arcs[arc_num].sister;
                        self.arcs[s1].sister = arc_num;
                        let s2 = self.arcs[arc_new_num].sister;
                        self.arcs[s2].sister = arc_new_num;
                    }

                    self.arc_tail[arc_num] = self.arc_tail[arc_new_num];
                    self.arc_tail[arc_new_num] = tail_node_id;
                    self.arc_first[tail_node_id] += 1;
                    tail_node_id = self.arc_tail[arc_num];
                }
                arc_num += 1;
            }
        }

        // overflow test (computed but not enforced, matching C++)
        for ndp in self.node_min..=self.node_max {
            let mut _cap_in: Excess = self.nodes[ndp].excess;
            let mut _cap_out: Excess = -self.nodes[ndp].excess;
            let a_start = self.nodes[ndp].first;
            let a_end = self.nodes[ndp + 1].first;
            for ac in a_start..a_end {
                if self.cap[ac] > 0 {
                    _cap_out += self.cap[ac];
                }
                if self.cap[ac] == 0 {
                    let sister = self.arcs[ac].sister;
                    _cap_in += self.cap[sister];
                }
            }
        }

        assert!(self.node_min <= 1, "Node ids must start from 0 or 1");

        // adjustments: shift node base
        self.n = self.node_max - self.node_min + 1;
        let node_min = self.node_min;
        if node_min > 0 {
            self.nodes.drain(0..node_min);
            for arc in &mut self.arcs {
                if arc.head != NONE {
                    arc.head -= node_min;
                }
            }
        }

        // free internal arrays
        self.arc_first.clear();
        self.arc_tail.clear();
    }

    /// Prepares the solver state for the cost-scaling iterations.
    ///
    /// Performs three key setup steps:
    /// 1. **Saturates negative-cost arcs** — pushes flow to capacity on every
    ///    arc with negative cost, converting the zero flow into a 0-optimal
    ///    pseudoflow.
    /// 2. **Scales costs** — multiplies all arc costs by `dn = n + 1` so that
    ///    epsilon-optimality arithmetic uses integers throughout.
    /// 3. **Allocates buckets** — creates the Dial-style bucket array used by
    ///    [`price_update`](Self::price_update) and
    ///    [`price_refine`](Self::price_refine), sized to `O(n * scale_factor)`.
    fn cs2_initialize(&mut self) {
        self.f_scale = SCALE_DEFAULT;
        self.sentinel_node = self.n;
        self.sentinel_arc = self.m;

        for i in 0..self.sentinel_node {
            self.nodes[i].price = 0;
            self.nodes[i].suspended = self.nodes[i].first;
            self.nodes[i].q_next = self.sentinel_node;
        }

        self.nodes[self.sentinel_node].first = self.sentinel_arc;
        self.nodes[self.sentinel_node].suspended = self.sentinel_arc;

        // saturate negative arcs
        for i in 0..self.sentinel_node {
            let a_stop = self.nodes[i + 1].suspended;
            let mut a = self.nodes[i].first;
            while a < a_stop {
                if self.arcs[a].cost < 0 {
                    let df = self.arcs[a].res_capacity;
                    if df > 0 {
                        let j = self.arcs[a].head;
                        self.increase_flow(i, j, a, df);
                    }
                }
                a += 1;
            }
        }

        self.dn = (self.n + 1) as Price;
        if self.no_zero_cycles {
            self.dn *= 2;
        }

        for a in 0..self.sentinel_arc {
            self.arcs[a].cost *= self.dn;
        }

        if self.no_zero_cycles {
            for a in 0..self.sentinel_arc {
                let sister = self.arcs[a].sister;
                if self.arcs[a].cost == 0 && self.arcs[sister].cost == 0 {
                    self.arcs[a].cost = 1;
                    let sister = self.arcs[a].sister;
                    self.arcs[sister].cost = -1;
                }
            }
        }

        if (self.max_cost as f64) * (self.dn as f64) > MAX_64 as f64 {
            println!("Warning: Arc lengths too large, overflow possible");
        }
        self.mmc = self.max_cost * self.dn;

        self.linf = (self.dn as f64 * self.f_scale.ceil() + 2.0) as usize;

        self.buckets = BucketArray::new(self.linf, self.n + 2);

        self.epsilon = self.mmc;
        if self.epsilon < 1 {
            self.epsilon = 1;
        }

        self.price_min = -PRICE_MAX;

        self.cut_off_factor = CUT_OFF_COEF * (self.n as f64).powf(CUT_OFF_POWER);
        if self.cut_off_factor < CUT_OFF_MIN {
            self.cut_off_factor = CUT_OFF_MIN;
        }

        self.n_ref = 0;
        self.flag_price = 0;

        // dummy_node: extra node for excess queue
        self.dummy_node = self.nodes.len();
        self.nodes.push(Node::default());

        self.excq_first = NONE;
        self.excq_last = NONE;
    }

    /// Scans node `i` during a price update, propagating distance labels
    /// to neighboring nodes via reverse residual arcs.
    ///
    /// For each neighbor `j` reachable through a reverse arc with positive
    /// residual capacity, computes a candidate rank from the reduced cost
    /// `rc = p_j + c_ji - p_i`. If `rc < 0` the arc is admissible and `j`
    /// inherits `i`'s rank; otherwise the rank increases by `ceil(rc / epsilon)`.
    /// When a neighbor's rank improves, it is moved to a closer bucket in the
    /// Dijkstra-like scan order used by [`price_update`](Self::price_update).
    ///
    /// After processing all neighbors, node `i`'s price is decreased by
    /// `rank * epsilon` and its rank is set to −1 (settled).
    fn up_node_scan(&mut self, i: NodeIndex) {
        self.n_scan += 1;
        let i_rank = self.buckets.rank[i];
        let a_start = self.nodes[i].first;
        let a_stop = self.nodes[i + 1].suspended;

        for a in a_start..a_stop {
            let ra = self.arcs[a].sister;
            if self.arcs[ra].res_capacity > 0 {
                let j = self.arcs[a].head;
                let j_rank = self.buckets.rank[j];
                if j_rank > i_rank {
                    let rc = self.nodes[j].price + self.arcs[ra].cost - self.nodes[i].price;
                    let j_new_rank = if rc < 0 {
                        i_rank
                    } else {
                        let dr = rc / self.epsilon;
                        if dr < self.linf as i64 {
                            i_rank + dr + 1
                        } else {
                            self.linf as i64
                        }
                    };
                    if j_rank > j_new_rank {
                        self.buckets.rank[j] = j_new_rank;
                        self.nodes[j].current = ra;
                        if j_rank < self.linf as i64 {
                            let b_old = j_rank as usize;
                            self.buckets.remove(j, b_old);
                        }
                        let b_new = j_new_rank as usize;
                        self.buckets.insert(j, b_new);
                    }
                }
            }
        }

        self.nodes[i].price -= i_rank * self.epsilon;
        self.buckets.rank[i] = -1;
    }

    /// Globally recomputes node prices using a Dijkstra-like bucket scan
    /// (Goldberg §2.1: *price updates*).
    ///
    /// Seeds bucket 0 with all deficit nodes (excess < 0), then scans outward
    /// through increasing buckets via [`up_node_scan`](Self::up_node_scan).
    /// Scanning stops once enough surplus has been reached to cover
    /// `total_excess`. Unsettled nodes have their prices decreased uniformly
    /// by the furthest bucket distance reached.
    ///
    /// Sets `flag_updt = Failed` if not all surplus nodes are reachable from
    /// deficit nodes, signaling potential infeasibility or a need to unsuspend
    /// arcs.
    fn price_update(&mut self) {
        self.n_update += 1;

        for i in 0..self.sentinel_node {
            if self.nodes[i].excess < 0 {
                self.buckets.insert(i, 0);
                self.buckets.rank[i] = 0;
            } else {
                self.buckets.rank[i] = self.linf as i64;
            }
        }

        let mut remain = self.total_excess;
        if (remain as f64) < 0.5 {
            return;
        }

        let mut b = 0usize;
        while b < self.linf {
            while self.buckets.nonempty(b) {
                let i = self.buckets.get(b);
                self.up_node_scan(i);
                if self.nodes[i].excess > 0 {
                    remain -= self.nodes[i].excess;
                    if remain <= 0 {
                        break;
                    }
                }
            }
            if remain <= 0 {
                break;
            }
            b += 1;
        }

        if remain as f64 > 0.5 {
            self.flag_updt = UpdateFlag::Failed;
        }

        let dp = (b as i64) * self.epsilon;

        for i in 0..self.sentinel_node {
            if self.buckets.rank[i] >= 0 {
                if self.buckets.rank[i] < self.linf as i64 {
                    let bucket_idx = self.buckets.rank[i] as usize;
                    self.buckets.remove(i, bucket_idx);
                }
                if self.nodes[i].price > self.price_min {
                    self.nodes[i].price -= dp;
                }
            }
        }
    }

    /// Relabels node `i` by scanning its outgoing residual arcs for the best
    /// admissible price.
    ///
    /// Scans the adjacency list of node `i` in two passes (from `current+1` to
    /// end, then from `first` to `current+1`) looking for the residual arc whose
    /// head offers the maximum reduced cost `p_j - c_ij`. Three outcomes are
    /// possible:
    ///
    /// - **Early exit (`Ok(true)`):** An arc with `dp > i_price` is found,
    ///   meaning the current arc is already admissible — no price change needed.
    /// - **Price update (`Ok(false)`):** The best arc has `dp <= i_price`.
    ///   Node `i`'s price is lowered to `p_max - epsilon` and its current arc
    ///   pointer is updated.
    /// - **Error:** No residual arcs exist and all arcs are suspended, indicating
    ///   infeasibility or price overflow.
    fn relabel(&mut self, i: NodeIndex) -> Result<bool, Cs2Error> {
        let mut p_max = self.price_min;
        let i_price = self.nodes[i].price;
        let mut a_max: ArcIndex = NONE;

        // scan 1/2: from current+1 to end
        let a_start = self.nodes[i].current + 1;
        let a_stop = self.nodes[i + 1].suspended;
        for a in a_start..a_stop {
            if self.arcs[a].res_capacity > 0 {
                let head = self.arcs[a].head;
                let dp = self.nodes[head].price - self.arcs[a].cost;
                if dp > p_max {
                    if i_price < dp {
                        self.nodes[i].current = a;
                        return Ok(true);
                    }
                    p_max = dp;
                    a_max = a;
                }
            }
        }

        // scan 2/2: from first to current+1
        let a_start2 = self.nodes[i].first;
        let a_stop2 = self.nodes[i].current + 1;
        for a in a_start2..a_stop2 {
            if self.arcs[a].res_capacity > 0 {
                let head = self.arcs[a].head;
                let dp = self.nodes[head].price - self.arcs[a].cost;
                if dp > p_max {
                    if i_price < dp {
                        self.nodes[i].current = a;
                        return Ok(true);
                    }
                    p_max = dp;
                    a_max = a;
                }
            }
        }

        if p_max != self.price_min {
            self.nodes[i].price = p_max - self.epsilon;
            self.nodes[i].current = a_max;
        } else if self.nodes[i].suspended == self.nodes[i].first {
            if self.nodes[i].excess == 0 {
                self.nodes[i].price = self.price_min;
            } else if self.n_ref == 1 {
                return Err(Cs2Error::Infeasible);
            } else {
                return Err(Cs2Error::PriceOverflow);
            }
        } else {
            self.flag_price = 1;
        }

        self.n_relabel += 1;
        self.n_rel += 1;
        Ok(false)
    }

    /// Applies push and relabel operations to active node `i` until it becomes
    /// inactive (Goldberg §1, Fig 4: *discharge*).
    ///
    /// Implements the **push lookahead** heuristic (Goldberg §2.4): before
    /// pushing to a node `j` with non-negative excess, checks whether `j` has
    /// an outgoing admissible arc. If `j` had zero excess and becomes active
    /// from the push, it is relabeled immediately to avoid the common scenario
    /// where flow is pushed back to `i` on the next discharge of `j`.
    ///
    /// The loop alternates between pushing along the current arc and relabeling
    /// when the current arc is inadmissible, stopping when `i`'s excess drops
    /// to zero or `flag_price` signals that suspended arcs need attention.
    #[inline(never)]
    fn discharge(&mut self, i: NodeIndex) -> Result<(), Cs2Error> {
        self.n_discharge += 1;

        let mut a = self.nodes[i].current;
        let mut j = self.arcs[a].head;

        // check admissible
        let is_admissible = self.arcs[a].res_capacity > 0
            && self.nodes[i].price + self.arcs[a].cost < self.nodes[j].price;
        if !is_admissible {
            self.relabel(i)?;
            a = self.nodes[i].current;
            j = self.arcs[a].head;
        }

        loop {
            let j_exc = self.nodes[j].excess;
            if j_exc >= 0 {
                let df = self.nodes[i].excess.min(self.arcs[a].res_capacity);
                if j_exc == 0 {
                    self.n_src += 1;
                }
                self.increase_flow(i, j, a, df);
                if self.out_of_excess_q(j) {
                    self.insert_to_excess_q(j);
                }
            } else {
                let df = self.nodes[i].excess.min(self.arcs[a].res_capacity);
                self.increase_flow(i, j, a, df);
                if self.nodes[j].excess >= 0 {
                    if self.nodes[j].excess > 0 {
                        self.n_src += 1;
                        self.relabel(j)?;
                        self.insert_to_excess_q(j);
                    }
                    self.total_excess += j_exc;
                } else {
                    self.total_excess -= df;
                }
            }

            if self.nodes[i].excess <= 0 {
                self.n_src -= 1;
            }
            if self.nodes[i].excess <= 0 || self.flag_price != 0 {
                break;
            }

            self.relabel(i)?;
            a = self.nodes[i].current;
            j = self.arcs[a].head;
        }

        self.nodes[i].current = a;
        Ok(())
    }

    /// Unsuspends arcs whose reduced cost has fallen back within the
    /// `cut_on` threshold (reverse of [`price_out`](Self::price_out)).
    ///
    /// Scans each node's suspended arc range `[suspended, first)` and moves
    /// arcs with `|rc| < cut_on` back into the active range by decrementing
    /// `first` and exchanging. If any suspended arc is found to be admissible
    /// (negative reduced cost with positive residual capacity), this is a
    /// "bad fix-in": the arc is saturated and both the forward and reverse
    /// arcs are unsuspended. On the first bad fix-in, `update_cut_off` is
    /// called and the scan restarts with a wider threshold.
    ///
    /// Returns the number of bad fix-ins found. If nonzero, the excess queue
    /// is rebuilt from scratch.
    fn price_in(&mut self) -> i32 {
        let mut bad_found = 0;
        let mut n_in_bad = 0;

        'restart: loop {
            for i in 0..self.sentinel_node {
                let initial_first = self.nodes[i].first;
                let suspended = self.nodes[i].suspended;

                for a in (suspended..initial_first).rev() {
                    let j = self.arcs[a].head;
                    let rc = self.nodes[i].price + self.arcs[a].cost - self.nodes[j].price;
                    if rc < 0 && self.arcs[a].res_capacity > 0 {
                        if bad_found == 0 {
                            bad_found = 1;
                            self.update_cut_off();
                            continue 'restart;
                        }
                        let df = self.arcs[a].res_capacity;
                        self.increase_flow(i, j, a, df);

                        let ra = self.arcs[a].sister;
                        let j = self.arcs[a].head;

                        self.nodes[i].first -= 1;
                        let b = self.nodes[i].first;
                        self.exchange(a, b);

                        if ra < self.nodes[j].first {
                            self.nodes[j].first -= 1;
                            let rb = self.nodes[j].first;
                            self.exchange(ra, rb);
                        }

                        n_in_bad += 1;
                    } else if (rc < self.cut_on as i64) && (rc > -(self.cut_on as i64)) {
                        self.nodes[i].first -= 1;
                        let b = self.nodes[i].first;
                        self.exchange(a, b);
                    }
                }
            }
            break;
        }

        if n_in_bad != 0 {
            self.n_bad_pricein += 1;

            self.total_excess = 0;
            self.n_src = 0;
            self.reset_excess_q();

            for i in 0..self.sentinel_node {
                self.nodes[i].current = self.nodes[i].first;
                let i_exc = self.nodes[i].excess;
                if i_exc > 0 {
                    self.total_excess += i_exc;
                    self.n_src += 1;
                    self.insert_to_excess_q(i);
                }
            }

            self.insert_to_excess_q(self.dummy_node);
        }

        if self.time_for_price_in == TIME_FOR_PRICE_IN2 {
            self.time_for_price_in = TIME_FOR_PRICE_IN3;
        }
        if self.time_for_price_in == TIME_FOR_PRICE_IN1 {
            self.time_for_price_in = TIME_FOR_PRICE_IN2;
        }

        n_in_bad
    }

    /// Converts an epsilon-optimal pseudoflow into an (epsilon/alpha)-optimal
    /// flow using the FIFO push-relabel method (Goldberg §1, Fig 2: *refine*).
    ///
    /// Enqueues all nodes with positive excess into the FIFO queue, then
    /// repeatedly discharges the front node. Periodically triggers global
    /// [`price_update`](Self::price_update) (based on relabel count and active
    /// node count) and [`price_in`](Self::price_in) (to recover suspended arcs
    /// that may have become relevant). If the price update fails because some
    /// surplus nodes are unreachable, widens the arc-fixing threshold and
    /// retries.
    #[inline(never)]
    fn refine(&mut self) -> Result<(), Cs2Error> {
        self.n_refine += 1;
        self.n_ref += 1;
        self.n_rel = 0;
        let mut pr_in_int: i32 = 0;

        self.total_excess = 0;
        self.n_src = 0;
        self.reset_excess_q();

        self.time_for_price_in = TIME_FOR_PRICE_IN1;

        for i in 0..self.sentinel_node {
            self.nodes[i].current = self.nodes[i].first;
            let i_exc = self.nodes[i].excess;
            if i_exc > 0 {
                self.total_excess += i_exc;
                self.n_src += 1;
                self.insert_to_excess_q(i);
            }
        }

        if self.total_excess <= 0 {
            return Ok(());
        }

        loop {
            if self.empty_excess_q() {
                if self.n_ref > PRICE_OUT_START {
                    pr_in_int = 0;
                    self.price_in();
                }
                if self.empty_excess_q() {
                    break;
                }
            }

            let i = self.remove_from_excess_q();

            if self.nodes[i].excess > 0 {
                self.discharge(i)?;

                if self.time_for_update() || self.flag_price != 0 {
                    if self.nodes[i].excess > 0 {
                        self.insert_to_excess_q(i);
                    }

                    if self.flag_price != 0 && self.n_ref > PRICE_OUT_START {
                        pr_in_int = 0;
                        self.price_in();
                        self.flag_price = 0;
                    }

                    self.price_update();

                    while self.flag_updt != UpdateFlag::Ok {
                        if self.n_ref == 1 {
                            return Err(Cs2Error::Infeasible);
                        } else {
                            self.flag_updt = UpdateFlag::Ok;
                            self.update_cut_off();
                            self.n_bad_relabel += 1;
                            pr_in_int = 0;
                            self.price_in();
                            self.price_update();
                        }
                    }
                    self.n_rel = 0;

                    if self.n_ref > PRICE_OUT_START {
                        pr_in_int += 1;
                        if pr_in_int > self.time_for_price_in {
                            pr_in_int = 0;
                            self.price_in();
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Attempts to find prices making the current flow epsilon-optimal without
    /// changing the flow, using the scaling shortest-paths technique
    /// (Goldberg §2.2: *price refinement*).
    ///
    /// Each pass performs a DFS on the admissible graph to topologically sort
    /// it. If a negative-cost cycle is found, it is saturated (cancelling flow
    /// around the cycle) and epsilon-optimality fails. Otherwise, longest-path
    /// distances `d'` are computed in the acyclic admissible graph (in units of
    /// epsilon) using a reverse-topological bucket scan, and prices are adjusted
    /// by `d' * epsilon`.
    ///
    /// Returns `true` if epsilon-optimal prices were found, `false` if an
    /// admissible cycle was detected (requiring a subsequent [`refine`](Self::refine)).
    fn price_refine(&mut self) -> bool {
        self.n_prefine += 1;
        let mut eps_optimal = true;
        let mut snc: i32 = 0;

        self.snc_max = if self.n_ref >= START_CYCLE_CANCEL {
            MAX_CYCLES_CANCELLED
        } else {
            0
        };

        // main loop
        loop {
            let mut nnc: i32 = 0;
            for i in 0..self.sentinel_node {
                self.buckets.rank[i] = 0;
                self.nodes[i].inp = Color::White;
                self.nodes[i].current = self.nodes[i].first;
            }
            self.reset_stackq();

            for root in 0..self.sentinel_node {
                if self.nodes[root].inp == Color::Black {
                    continue;
                }
                self.nodes[root].dfs_parent = NONE;
                let mut i = root;

                // depth first search
                'dfs: loop {
                    self.nodes[i].inp = Color::Grey;
                    let mut a = self.nodes[i].current;
                    let a_stop = self.nodes[i + 1].suspended;
                    let mut stepped = false;

                    while a < a_stop {
                        if self.arcs[a].res_capacity > 0 {
                            let j = self.arcs[a].head;
                            let rc = self.nodes[i].price + self.arcs[a].cost - self.nodes[j].price;
                            if rc < 0 {
                                if self.nodes[j].inp == Color::White {
                                    // step forward
                                    self.nodes[i].current = a;
                                    self.nodes[j].dfs_parent = i;
                                    i = j;
                                    stepped = true;
                                    break;
                                }
                                if self.nodes[j].inp == Color::Grey {
                                    // cycle detected
                                    eps_optimal = false;
                                    nnc += 1;
                                    self.nodes[i].current = a;

                                    // find min capacity on cycle
                                    let mut is = i;
                                    let mut ir = i;
                                    let mut df: i64 = MAX_32;
                                    loop {
                                        let ar = self.nodes[ir].current;
                                        if self.arcs[ar].res_capacity <= df {
                                            df = self.arcs[ar].res_capacity;
                                            is = ir;
                                        }
                                        if ir == j {
                                            break;
                                        }
                                        ir = self.nodes[ir].dfs_parent;
                                    }

                                    // push flow around cycle
                                    ir = i;
                                    loop {
                                        let ar = self.nodes[ir].current;
                                        let head = self.arcs[ar].head;
                                        self.increase_flow(ir, head, ar, df);
                                        if ir == j {
                                            break;
                                        }
                                        ir = self.nodes[ir].dfs_parent;
                                    }

                                    if is != i {
                                        ir = i;
                                        while ir != is {
                                            self.nodes[ir].inp = Color::White;
                                            ir = self.nodes[ir].dfs_parent;
                                        }
                                        i = is;
                                        stepped = true;
                                        break;
                                    }
                                    // is == i: continue scanning
                                }
                            }
                        }
                        a += 1;
                    }

                    if stepped {
                        continue 'dfs;
                    }

                    // step back
                    self.nodes[i].inp = Color::Black;
                    self.n_prscan1 += 1;
                    let j = self.nodes[i].dfs_parent;
                    self.stackq_push(i);
                    if j == NONE {
                        break 'dfs;
                    }
                    i = j;
                    self.nodes[i].current += 1;
                }
            }

            // computing longest paths
            snc += nnc;
            if snc < self.snc_max {
                eps_optimal = true;
            }
            if !eps_optimal {
                break;
            }
            let mut bmax: usize = 0;

            while self.nonempty_stackq() {
                self.n_prscan2 += 1;
                let i = self.stackq_pop();
                let i_rank = self.buckets.rank[i];
                let a_start = self.nodes[i].first;
                let a_stop = self.nodes[i + 1].suspended;
                for a in a_start..a_stop {
                    if self.arcs[a].res_capacity > 0 {
                        let j = self.arcs[a].head;
                        let rc = self.nodes[i].price + self.arcs[a].cost - self.nodes[j].price;
                        if rc < 0 {
                            let dr = (-rc as f64 - 0.5) / self.epsilon as f64;
                            let j_rank = dr as i64 + i_rank;
                            if j_rank < self.linf as i64 && j_rank > self.buckets.rank[j] {
                                self.buckets.rank[j] = j_rank;
                            }
                        }
                    }
                }
                if i_rank > 0 {
                    if i_rank as usize > bmax {
                        bmax = i_rank as usize;
                    }
                    let b = i_rank as usize;
                    self.buckets.insert(i, b);
                }
            }

            if bmax == 0 {
                break;
            }

            let mut b = bmax;
            while b >= 1 {
                let i_rank = b as i64;
                let dp = i_rank * self.epsilon;

                while self.buckets.nonempty(b) {
                    let i = self.buckets.get(b);
                    self.n_prscan += 1;

                    let a_start = self.nodes[i].first;
                    let a_stop = self.nodes[i + 1].suspended;
                    for a in a_start..a_stop {
                        if self.arcs[a].res_capacity > 0 {
                            let j = self.arcs[a].head;
                            let j_rank = self.buckets.rank[j];
                            if j_rank < i_rank {
                                let rc =
                                    self.nodes[i].price + self.arcs[a].cost - self.nodes[j].price;
                                let j_new_rank = if rc < 0 {
                                    i_rank
                                } else {
                                    let dr = rc / self.epsilon;
                                    if dr < self.linf as i64 {
                                        i_rank - (dr + 1)
                                    } else {
                                        0
                                    }
                                };
                                if j_rank < j_new_rank {
                                    if eps_optimal {
                                        self.buckets.rank[j] = j_new_rank;
                                        if j_rank > 0 {
                                            let b_old = j_rank as usize;
                                            self.buckets.remove(j, b_old);
                                        }
                                        let b_new = j_new_rank as usize;
                                        self.buckets.insert(j, b_new);
                                    } else {
                                        let df = self.arcs[a].res_capacity;
                                        let j = self.arcs[a].head;
                                        self.increase_flow(i, j, a, df);
                                    }
                                }
                            }
                        }
                    }

                    self.nodes[i].price -= dp;
                }
                b -= 1;
            }

            if !eps_optimal {
                break;
            }
        }

        // finish: saturate non-epsilon-optimal arcs if needed
        if !eps_optimal {
            for i in 0..self.sentinel_node {
                let a_start = self.nodes[i].first;
                let a_stop = self.nodes[i + 1].suspended;
                for a in a_start..a_stop {
                    let j = self.arcs[a].head;
                    let rc = self.nodes[i].price + self.arcs[a].cost - self.nodes[j].price;
                    if rc < -self.epsilon {
                        let df = self.arcs[a].res_capacity;
                        if df > 0 {
                            self.increase_flow(i, j, a, df);
                        }
                    }
                }
            }
        }

        eps_optimal
    }

    /// Computes optimal dual prices (node potentials) for the final solution.
    ///
    /// Structurally similar to [`price_refine`](Self::price_refine) but operates
    /// on *all* arcs (including suspended) and uses exact reduced costs (not
    /// scaled by epsilon). Performs a DFS to topologically sort the residual
    /// graph, computes longest-path distances via a reverse-topological bucket
    /// scan, and adjusts prices accordingly. Aborts early if a negative-cost
    /// residual cycle is detected (should not happen for a correct solution).
    fn compute_prices(&mut self) {
        self.n_prefine += 1;
        // Whether the graph is cycle free
        // (expected for a correct, finished solution).
        let mut cycle_free = true;

        loop {
            for i in 0..self.sentinel_node {
                self.buckets.rank[i] = 0;
                self.nodes[i].inp = Color::White;
                self.nodes[i].current = self.nodes[i].first;
            }
            self.reset_stackq();

            for root in 0..self.sentinel_node {
                if self.nodes[root].inp == Color::Black {
                    continue;
                }
                self.nodes[root].dfs_parent = NONE;
                let mut i = root;

                'dfs: loop {
                    self.nodes[i].inp = Color::Grey;
                    let mut a = self.nodes[i].suspended;
                    let a_stop = self.nodes[i + 1].suspended;
                    let mut stepped = false;

                    while a < a_stop {
                        if self.arcs[a].res_capacity > 0 {
                            let j = self.arcs[a].head;
                            let rc = self.nodes[i].price + self.arcs[a].cost - self.nodes[j].price;
                            if rc < 0 {
                                if self.nodes[j].inp == Color::White {
                                    self.nodes[i].current = a;
                                    self.nodes[j].dfs_parent = i;
                                    i = j;
                                    stepped = true;
                                    break;
                                }
                                if self.nodes[j].inp == Color::Grey {
                                    cycle_free = false;
                                }
                            }
                        }
                        a += 1;
                    }

                    if stepped {
                        continue 'dfs;
                    }

                    self.nodes[i].inp = Color::Black;
                    self.n_prscan1 += 1;
                    let j = self.nodes[i].dfs_parent;
                    self.stackq_push(i);
                    if j == NONE {
                        break 'dfs;
                    }
                    i = j;
                    self.nodes[i].current += 1;
                }
            }

            if !cycle_free {
                break;
            }
            let mut bmax: usize = 0;

            while self.nonempty_stackq() {
                self.n_prscan2 += 1;
                let i = self.stackq_pop();
                let i_rank = self.buckets.rank[i];
                let a_start = self.nodes[i].suspended;
                let a_stop = self.nodes[i + 1].suspended;
                for a in a_start..a_stop {
                    if self.arcs[a].res_capacity > 0 {
                        let j = self.arcs[a].head;
                        let rc = self.nodes[i].price + self.arcs[a].cost - self.nodes[j].price;
                        if rc < 0 {
                            let dr = -rc;
                            let j_rank = dr + i_rank;
                            if j_rank < self.linf as i64 && j_rank > self.buckets.rank[j] {
                                self.buckets.rank[j] = j_rank;
                            }
                        }
                    }
                }
                if i_rank > 0 {
                    if i_rank as usize > bmax {
                        bmax = i_rank as usize;
                    }
                    let b = i_rank as usize;
                    self.buckets.insert(i, b);
                }
            }

            if bmax == 0 {
                break;
            }

            let mut b = bmax;
            while b >= 1 {
                let i_rank = b as i64;
                let dp = i_rank;

                while self.buckets.nonempty(b) {
                    let i = self.buckets.get(b);
                    self.n_prscan += 1;

                    let a_start = self.nodes[i].suspended;
                    let a_stop = self.nodes[i + 1].suspended;
                    for a in a_start..a_stop {
                        if self.arcs[a].res_capacity > 0 {
                            let j = self.arcs[a].head;
                            let j_rank = self.buckets.rank[j];
                            if j_rank < i_rank {
                                let rc =
                                    self.nodes[i].price + self.arcs[a].cost - self.nodes[j].price;
                                let j_new_rank = if rc < 0 {
                                    i_rank
                                } else {
                                    let dr = rc;
                                    if dr < self.linf as i64 {
                                        i_rank - (dr + 1)
                                    } else {
                                        0
                                    }
                                };
                                if j_rank < j_new_rank && cycle_free {
                                    self.buckets.rank[j] = j_new_rank;
                                    if j_rank > 0 {
                                        let b_old = j_rank as usize;
                                        self.buckets.remove(j, b_old);
                                    }
                                    let b_new = j_new_rank as usize;
                                    self.buckets.insert(j, b_new);
                                }
                            }
                        }
                    }

                    self.nodes[i].price -= dp;
                }
                b -= 1;
            }

            if !cycle_free {
                break;
            }
        }
    }

    /// Suspends arcs whose reduced cost exceeds the `cut_off` threshold
    /// (Goldberg §2.3: *speculative arc fixing*).
    ///
    /// An arc is suspended if its reduced cost is large enough that the
    /// push-relabel method will not change its flow before epsilon decreases
    /// further. Suspended arcs are moved before `first` in the adjacency list
    /// via [`exchange`](Self::exchange), so they are skipped by relabel and
    /// discharge. They can later be recovered by [`price_in`](Self::price_in).
    fn price_out(&mut self) {
        let n_cut_off = -self.cut_off;

        for i in 0..self.sentinel_node {
            let a_stop = self.nodes[i + 1].suspended;
            let mut a = self.nodes[i].first;
            while a < a_stop {
                let j = self.arcs[a].head;
                let rc = (self.nodes[i].price + self.arcs[a].cost - self.nodes[j].price) as f64;
                let sister = self.arcs[a].sister;
                if (rc > self.cut_off && self.arcs[sister].res_capacity <= 0)
                    || (rc < n_cut_off && self.arcs[a].res_capacity <= 0)
                {
                    let b = self.nodes[i].first;
                    self.nodes[i].first += 1;
                    self.exchange(a, b);
                }
                a += 1;
            }
        }
    }

    /// Reduce epsilon by the scale factor for the next scaling iteration.
    /// Returns `true` if epsilon has reached 1 (scaling is complete),
    /// and `false` if epsilon was successfully reduced.
    fn update_epsilon(&mut self) -> bool {
        // decrease epsilon after epsilon-optimal flow is constructed
        if self.epsilon <= 1 {
            return true;
        }
        self.epsilon = (self.epsilon as f64 / self.f_scale).ceil() as Price;
        self.cut_off = self.cut_off_factor * self.epsilon as f64;
        self.cut_on = self.cut_off * CUT_OFF_GAP;
        false
    }

    /// Checks the feasibility of the proposed problem.
    fn is_feasible(&mut self) -> bool {
        let mut ans = true;
        for i in 0..self.sentinel_node {
            let a_start = self.nodes[i].suspended;
            let a_stop = self.nodes[i + 1].suspended;
            for a in a_start..a_stop {
                if self.cap[a] > 0 {
                    let fa = self.cap[a] - self.arcs[a].res_capacity;
                    if fa < 0 {
                        ans = false;
                        break;
                    }
                    self.node_balance[i] -= fa;
                    let head = self.arcs[a].head;
                    self.node_balance[head] += fa;
                }
            }
        }
        for i in 0..self.sentinel_node {
            if self.node_balance[i] != 0 {
                ans = false;
                break;
            }
        }
        ans
    }

    /// Checks complimentary slackness.
    ///
    /// If true, then the problem is possibly feasible,
    /// otherwise the problem is unfeasible.
    fn check_cs(&self) -> bool {
        for i in 0..self.sentinel_node {
            let a_start = self.nodes[i].suspended;
            let a_stop = self.nodes[i + 1].suspended;
            for a in a_start..a_stop {
                if self.arcs[a].res_capacity > 0 {
                    let j = self.arcs[a].head;
                    let rc = self.nodes[i].price + self.arcs[a].cost - self.nodes[j].price;
                    if rc < 0 {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Prints the solution.
    ///
    /// comp_duals: whether to compute the prices.
    fn print_solution(&self, comp_duals: bool) {
        if !self.print_ans {
            return;
        }
        println!("c");
        println!("s 0"); // cost printed separately

        for i in 0..self.n {
            let ni = n_node(i, self.node_min);
            let a_start = self.nodes[i].suspended;
            let a_stop = self.nodes[i + 1].suspended;
            for a in a_start..a_stop {
                if self.cap[a] > 0 {
                    println!(
                        "f {:7} {:7} {:10}",
                        ni,
                        n_node(self.arcs[a].head, self.node_min),
                        self.cap[a] - self.arcs[a].res_capacity
                    );
                }
            }
        }

        if comp_duals {
            let mut min_price = MAX_32;
            for i in 0..self.sentinel_node {
                min_price = min_price.min(self.nodes[i].price);
            }
            for i in 0..self.sentinel_node {
                println!(
                    "p {:7} {:7}",
                    n_node(i, self.node_min),
                    self.nodes[i].price - min_price
                );
            }
        }
        println!("c");
    }

    /// Prints the graph structure to stdout (for debugging).
    fn print_graph(&self) {
        println!("\nGraph: {}", self.n);
        for i in 0..self.n {
            let ni = n_node(i, self.node_min);
            println!("\nNode {ni}");
            let a_start = self.nodes[i].suspended;
            let a_stop = self.nodes[i + 1].suspended;
            for a in a_start..a_stop {
                println!(
                    " {{{}}} {} -> {}  cap: {}  cost: {}",
                    a,
                    ni,
                    n_node(self.arcs[a].head, self.node_min),
                    self.cap[a],
                    self.arcs[a].cost
                );
            }
        }
    }

    /// Post-processing: unscales costs and prices back to original units,
    /// computes the objective cost, and optionally computes dual prices.
    ///
    /// During [`cs2_initialize`](Self::cs2_initialize), all arc costs were
    /// multiplied by `dn` for integer epsilon arithmetic. This method divides
    /// them back, computes `sum(cost * flow)` over forward arcs, and divides
    /// node prices by `dn`.
    fn finishup(&mut self, objective_cost: &mut f64, comp_duals: bool) {
        // remove zero-cost cycle markers
        if self.no_zero_cycles {
            for a in 0..self.sentinel_arc {
                if self.arcs[a].cost == 1 {
                    let sister = self.arcs[a].sister;
                    assert!(self.arcs[sister].cost == -1);
                    self.arcs[a].cost = 0;
                    self.arcs[sister].cost = 0;
                }
            }
        }

        let mut obj_internal: f64 = 0.0;
        for a in 0..self.sentinel_arc {
            let cs = self.arcs[a].cost / self.dn;
            if self.cap[a] > 0 {
                let flow = self.cap[a] - self.arcs[a].res_capacity;
                if flow != 0 {
                    obj_internal += cs as f64 * flow as f64;
                }
            }
            self.arcs[a].cost = cs;
        }

        for i in 0..self.sentinel_node {
            self.nodes[i].price /= self.dn;
        }

        if comp_duals {
            self.compute_prices();
        }

        *objective_cost = obj_internal;
    }

    /// Main loop of the successive approximation algorithm
    /// (Goldberg §1, Fig 1: *Min-Cost*).
    ///
    /// Starting from `epsilon = max_cost * dn`, repeatedly:
    /// 1. Calls [`refine`](Self::refine) to convert the current pseudoflow
    ///    into an epsilon-optimal flow.
    /// 2. Calls [`price_out`](Self::price_out) to suspend arcs with large
    ///    reduced costs.
    /// 3. Reduces epsilon by the scale factor.
    /// 4. Attempts [`price_refine`](Self::price_refine) to skip full refine
    ///    iterations when prices alone can establish optimality at the new
    ///    epsilon. Falls back to refine if price_refine detects a cycle.
    ///
    /// Terminates when `epsilon < 1`, at which point the flow is optimal.
    #[inline(never)]
    fn cs2(&mut self, objective_cost: &mut f64, comp_duals: bool) -> Result<(), Cs2Error> {
        let mut scaling_done = false;

        self.update_epsilon();

        loop {
            self.refine()?;

            if self.n_ref >= PRICE_OUT_START {
                self.price_out();
            }

            if self.update_epsilon() {
                break;
            }

            loop {
                // need to refine further
                if !self.price_refine() {
                    break;
                }

                if self.n_ref >= PRICE_OUT_START {
                    if self.price_in() != 0 {
                        break;
                    }
                    scaling_done = self.update_epsilon();
                    if scaling_done {
                        break;
                    }
                }
            }

            if scaling_done {
                break;
            }
        }

        self.finishup(objective_cost, comp_duals);
        Ok(())
    }

    /// Executes the cost-scaling minimum-cost maximum-flow algorithm, printing the solution.
    ///
    /// Args
    /// - check_solution: Check feasibility/optimality. Note that this adds high overhead.
    /// - comp_duals: Enable to compute prices
    pub fn run_cs2(&mut self, check_solution: bool, comp_duals: bool) -> Result<(), Cs2Error> {
        // ordering
        self.pre_processing();

        // check solution setup
        if check_solution {
            self.node_balance = vec![0i64; self.n + 1];
            for i in 0..self.n {
                self.node_balance[i] = self.nodes[i].excess;
            }
        }

        // double the arc count (forward + backward)
        self.m *= 2;
        self.cs2_initialize();
        self.print_graph();

        println!("\nc CS 4.3");
        println!("c nodes: {}  arcs: {}", self.n, self.m / 2);
        println!(
            "c scale-factor: {}  cut-off-factor: {}\nc",
            self.f_scale, self.cut_off_factor
        );

        let mut objective_cost: f64 = 0.0;
        self.cs2(&mut objective_cost, comp_duals)?;

        let t = 0.0f64;
        println!("c time:         {t:15.2}    cost:       {objective_cost:15.0}");
        println!(
            "c refines:      {:10}     discharges: {:10}",
            self.n_refine, self.n_discharge
        );
        println!(
            "c pushes:       {:10}     relabels:   {:10}",
            self.n_push, self.n_relabel
        );
        println!(
            "c updates:      {:10}     u-scans:    {:10}",
            self.n_update, self.n_scan
        );
        println!(
            "c p-refines:    {:10}     r-scans:    {:10}",
            self.n_prefine, self.n_prscan
        );
        println!(
            "c dfs-scans:    {:10}     bad-in:     {:4}  + {:2}",
            self.n_prscan1, self.n_bad_pricein, self.n_bad_relabel
        );

        if check_solution {
            println!("c checking feasibility...");
            if self.is_feasible() {
                println!("c ...OK");
            } else {
                println!("c ERROR: solution infeasible");
            }
            println!("c computing prices and checking CS...");
            self.compute_prices();
            if self.check_cs() {
                println!("c ...OK");
            } else {
                println!("ERROR: CS violation");
            }
        }

        if self.print_ans {
            self.print_solution(comp_duals);
        }
        Ok(())
    }

    /// Executes the cost-scaling minimum-cost maximum-flow algorithm, returning the solution
    /// as a [McmfSolution] object.
    ///
    /// Args
    /// - check_solution: Check feasibility/optimality. Note that this adds high overhead.
    /// - comp_duals: Enable to compute prices
    pub fn min_cost(
        mut self,
        check_solution: bool,
        comp_duals: bool,
    ) -> Result<McmfSolution, Cs2Error> {
        // ordering
        self.pre_processing();

        // check solution setup
        if check_solution {
            self.node_balance = vec![0i64; self.n + 1];
            for i in 0..self.n {
                self.node_balance[i] = self.nodes[i].excess;
            }
        }

        // double the arc count (forward + backward)
        self.m *= 2;
        self.cs2_initialize();

        let mut objective_cost = 0.0;
        self.cs2(&mut objective_cost, comp_duals)?;

        if check_solution {
            if !self.is_feasible() {
                return Err(Cs2Error::Infeasible);
            }
            self.compute_prices();
            if !self.check_cs() {
                return Err(Cs2Error::Infeasible);
            }
        }

        Ok(McmfSolution {
            objective_cost,
            solver: self,
        })
    }
}

/// Result of a successful min-cost flow computation.
pub struct McmfSolution {
    /// Optimal objective cost.
    pub objective_cost: f64,
    /// The solver's state after completion
    solver: McmfCs2,
}

/// Informational statistics about the min-cost flow computation.
#[derive(Debug)]
pub struct McmfStats {
    /// Number of push operations.
    pub n_push: u64,
    /// Number of relabel operations.
    pub n_relabel: u64,
    /// Number of discharge operations.
    pub n_discharge: u64,
    /// Number of price refinement phases.
    pub n_refine: u64,
    /// Number of price update operations.
    pub n_update: u64,
    /// Number of node scans.
    pub n_scan: u64,
    /// Number of price scans.
    pub n_prscan: u64,
    /// Number of type-1 price scans.
    pub n_prscan1: u64,
    /// Number of bad price-in operations.
    pub n_bad_pricein: u64,
    /// Number of bad relabel operations.
    pub n_bad_relabel: u64,
    /// Number of price refinement restarts.
    pub n_prefine: u64,
}

impl McmfSolution {
    /// Iterate over original (forward) arcs yielding (tail, head, flow)
    pub fn flows(&self) -> impl Iterator<Item = (usize, usize, i64)> {
        let s = &self.solver;
        (0..s.n).flat_map(move |i| {
            let a_start = s.nodes[i].suspended;
            let a_stop = s.nodes[i + 1].suspended;
            (a_start..a_stop).filter_map(move |a| {
                if s.cap[a] > 0 {
                    let flow = s.cap[a] - s.arcs[a].res_capacity;
                    let tail = n_node(i, s.node_min) as usize;
                    let head = n_node(s.arcs[a].head, s.node_min) as usize;
                    Some((tail, head, flow))
                } else {
                    None
                }
            })
        })
    }

    /// Iterate over node prices yielding (node_id, price).
    /// Only meaningful if comp_duals was enabled.
    pub fn prices(&self) -> impl Iterator<Item = (usize, Price)> {
        let s = &self.solver;
        (0..s.sentinel_node).map(move |i| (n_node(i, s.node_min) as usize, s.nodes[i].price))
    }

    /// Returns statistics of the solution.
    pub fn stats(&self) -> McmfStats {
        let s = &self.solver;
        McmfStats {
            n_push: s.n_push,
            n_relabel: s.n_relabel,
            n_discharge: s.n_discharge,
            n_refine: s.n_refine,
            n_update: s.n_update,
            n_scan: s.n_scan,
            n_prscan: s.n_prscan,
            n_prscan1: s.n_prscan1,
            n_bad_pricein: s.n_bad_pricein,
            n_bad_relabel: s.n_bad_relabel,
            n_prefine: s.n_prefine,
        }
    }
}

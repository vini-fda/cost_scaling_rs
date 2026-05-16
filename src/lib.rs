//! CS2 min-cost max-flow scaling algorithm.
//!
//! This is a Rust implementation of the CS2 min-cost-max-flow scaling algorithm,
//! translated from the original C implementation.

#![warn(missing_docs)]
// Forbid the panic-emitting macros in the library crate. Build-time and
// solver-time errors are surfaced through Result<_, Cs2Error> instead.
// `#[cfg(test)]` modules and doc-tests are exempted so they can still
// `.expect()` / `.unwrap()` for terseness; integration tests in `tests/`
// and the `cost-scaling-rs` binary are separate crates and unaffected.
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::unreachable))]
#![cfg_attr(not(test), deny(clippy::todo))]
#![cfg_attr(not(test), deny(clippy::unimplemented))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]

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
///
/// Field layout mirrors the C `node` struct so that `a->head->price` is a
/// single load with offset, without any base+index*sizeof arithmetic.
#[derive(Clone)]
struct Node {
    /// First outgoing arc pointer.
    first: *mut Arc,
    /// Current outgoing arc pointer.
    current: *mut Arc,
    /// First suspended arc pointer.
    suspended: *mut Arc,
    /// Excess of the node.
    excess: Excess,
    /// Distance from a sink (node potential).
    price: Price,
    /// Next node in push-queue (or [`McmfCs2::sentinel_node_ptr`] if out of queue).
    q_next: *mut Node,
    /// Next node in bucket-list.
    b_next: *mut Node,
    /// Previous node in bucket-list.
    b_prev: *mut Node,
    /// Bucket number.
    rank: i64,
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
    /// Head node pointer.
    ///
    /// Stored as a raw pointer (matching the C implementation's `arc::head:
    /// node*`) so `(*a).head` is a direct address instead of an index that
    /// requires `base + idx*sizeof(Node)` arithmetic on every access.
    head: *mut Node,
    /// Opposite (sister) arc pointer.
    sister: *mut Arc,
}

/// A bucket used for node ordering during price updates.
#[derive(Clone)]
struct Bucket {
    /// First node in the bucket (or `dnode` if the bucket is empty).
    p_first: *mut Node,
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

/// Fatal error condition that terminates the CS2 solver, or rejects an
/// invalid build-time input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cs2Error {
    /// The problem is infeasible (unbalanced or unreachable nodes).
    Infeasible,
    /// Price values overflowed numerical limits.
    PriceOverflow,
    /// A supplied node id is outside the `1..=n` range fixed by
    /// [`McmfCs2::new`].
    NodeIdOutOfBounds {
        /// The offending node id.
        id: usize,
        /// The maximum valid node id (= `n`, the constructor's `num_nodes`).
        max: usize,
    },
    /// An arc references a node id outside the `1..=n` range.
    ArcOutOfBounds {
        /// Tail node id supplied to [`McmfCs2::set_arc`].
        tail: usize,
        /// Head node id supplied to [`McmfCs2::set_arc`].
        head: usize,
        /// The maximum valid node id (= `n`).
        max: usize,
    },
    /// An arc's lower/upper capacity bounds are inconsistent (e.g. negative
    /// lower bound or `low > up`).
    InvalidCapacityBounds {
        /// Lower bound that was rejected.
        low: i64,
        /// Upper bound that was rejected.
        up: i64,
    },
    /// Total supply does not equal total demand (sum of positive node
    /// excesses minus sum of negative excesses is non-zero).
    Unbalanced {
        /// Total supply (sum of positive node excesses).
        supply: Excess,
        /// Total demand (negated sum of negative node excesses).
        demand: Excess,
    },
    /// Node ids must start at 0 or 1; pre-processing found a smaller
    /// minimum that the solver cannot internally remap.
    NodeIdsMustStartAtZeroOrOne {
        /// The minimum node id observed.
        min: usize,
    },
}

impl std::fmt::Display for Cs2Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Cs2Error::Infeasible => write!(f, "problem is infeasible"),
            Cs2Error::PriceOverflow => write!(f, "price values overflowed"),
            Cs2Error::NodeIdOutOfBounds { id, max } => {
                write!(f, "node id {id} out of bounds (max {max})")
            }
            Cs2Error::ArcOutOfBounds { tail, head, max } => write!(
                f,
                "arc {tail}->{head} has at least one endpoint out of bounds (max {max})"
            ),
            Cs2Error::InvalidCapacityBounds { low, up } => write!(
                f,
                "invalid capacity bounds: low={low}, up={up} (require 0 <= low <= up)"
            ),
            Cs2Error::Unbalanced { supply, demand } => {
                write!(f, "unbalanced problem: supply {supply} != demand {demand}")
            }
            Cs2Error::NodeIdsMustStartAtZeroOrOne { min } => write!(
                f,
                "node ids must start at 0 or 1; smallest observed id is {min}"
            ),
        }
    }
}

impl std::error::Error for Cs2Error {}

/// Error returned by [`McmfCs2::from_dimacs`] / [`McmfCs2::from_dimacs_file`]:
/// either the input could not be parsed, or the parsed problem failed the
/// build-time checks in [`McmfCs2::set_arc`] / [`McmfCs2::set_supply_demand_of_node`].
#[derive(Debug)]
pub enum DimacsLoadError {
    /// The DIMACS parser rejected the input.
    Parse(ParseError),
    /// The parsed problem did not satisfy the solver's build-time invariants
    /// (e.g. node ids out of range, invalid capacity bounds).
    Build(Cs2Error),
}

impl std::fmt::Display for DimacsLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DimacsLoadError::Parse(e) => write!(f, "parse error: {e}"),
            DimacsLoadError::Build(e) => write!(f, "build error: {e}"),
        }
    }
}

impl std::error::Error for DimacsLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DimacsLoadError::Parse(e) => Some(e),
            DimacsLoadError::Build(e) => Some(e),
        }
    }
}

impl From<ParseError> for DimacsLoadError {
    fn from(e: ParseError) -> Self {
        DimacsLoadError::Parse(e)
    }
}

impl From<Cs2Error> for DimacsLoadError {
    fn from(e: Cs2Error) -> Self {
        DimacsLoadError::Build(e)
    }
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
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let input = "p min 4 5\n\
///              n 1 4\n\
///              n 4 -4\n\
///              a 1 2 0 4 2\n\
///              a 1 3 0 2 2\n\
///              a 2 3 0 2 1\n\
///              a 2 4 0 3 3\n\
///              a 3 4 0 5 1\n";
/// let solver = McmfCs2::from_dimacs(input)?;
/// let solution = solver.min_cost(false, false)?;
/// assert!(solution.objective_cost > 0.0);
/// # Ok(()) }
/// ```
///
/// **Programmatically:**
///
/// ```
/// use cost_scaling_rs::McmfCs2;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let mut solver = McmfCs2::new(4, 5);
///
/// // Set supply (+) and demand (-) BEFORE adding arcs.
/// solver.set_supply_demand_of_node(1, 4)?;   // source
/// solver.set_supply_demand_of_node(4, -4)?;  // sink
///
/// // Add arcs: (tail, head, lower_bound, upper_bound, cost)
/// solver.set_arc(1, 2, 0, 4, 2)?;
/// solver.set_arc(1, 3, 0, 2, 2)?;
/// solver.set_arc(2, 3, 0, 2, 1)?;
/// solver.set_arc(2, 4, 0, 3, 3)?;
/// solver.set_arc(3, 4, 0, 5, 1)?;
///
/// let solution = solver.min_cost(false, false)?;
///
/// for (tail, head, flow) in solution.flows() {
///     if flow > 0 {
///         println!("  {tail} -> {head}: {flow}");
///     }
/// }
/// # Ok(()) }
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
    /// Cached base pointer for `nodes`, valid after [`Self::allocate_arrays`].
    nodes_base: *mut Node,
    /// Sentinel node pointer (one past last real node).
    sentinel_node: *mut Node,
    /// First node in push-queue, or null when empty.
    excq_first: *mut Node,
    /// Last node in push-queue, or null when empty.
    excq_last: *mut Node,
    /// Array of arcs.
    arcs: Vec<Arc>,
    /// Cached base pointer for `arcs`, valid after [`Self::allocate_arrays`].
    arcs_base: *mut Arc,
    /// Sentinel arc pointer (one past last real arc).
    sentinel_arc: *mut Arc,

    /// Array of buckets.
    buckets: Vec<Bucket>,
    /// Cached base pointer for `buckets`, valid after [`Self::cs2_initialize`].
    buckets_base: *mut Bucket,
    /// Last bucket index.
    l_bucket: BucketIndex,
    /// Number of l_bucket + 1.
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

    /// Pointer to dummy node for the excess queue.
    dummy_node: *mut Node,
    /// Pointer to the dnode used as a bucket sentinel.
    dnode: *mut Node,

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
            first: std::ptr::null_mut(),
            current: std::ptr::null_mut(),
            suspended: std::ptr::null_mut(),
            excess: 0,
            price: 0,
            q_next: std::ptr::null_mut(),
            b_next: std::ptr::null_mut(),
            b_prev: std::ptr::null_mut(),
            rank: 0,
            inp: Color::White,
        }
    }
}

impl Default for Arc {
    fn default() -> Self {
        Arc {
            res_capacity: 0,
            cost: 0,
            head: std::ptr::null_mut(),
            sister: std::ptr::null_mut(),
        }
    }
}

impl Default for Bucket {
    fn default() -> Self {
        Bucket {
            p_first: std::ptr::null_mut(),
        }
    }
}

impl TryFrom<parser::DimacsMin> for McmfCs2 {
    type Error = Cs2Error;

    fn try_from(problem: parser::DimacsMin) -> Result<Self, Self::Error> {
        let mut solver = McmfCs2::new(problem.nodes as usize, problem.arcs_count as usize);
        // Node supply/demand must be set before arcs, because set_arc adjusts
        // excess for nonzero lower bounds (excess -= low for tail, excess += low
        // for head). Setting nodes after arcs would overwrite those adjustments.
        for node in &problem.node_descs {
            solver.set_supply_demand_of_node(node.id as usize, node.supply)?;
        }
        for arc in &problem.arcs {
            solver.set_arc(
                arc.from as usize,
                arc.to as usize,
                arc.min_cap,
                arc.max_cap,
                arc.cost,
            )?;
        }
        Ok(solver)
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
            nodes_base: std::ptr::null_mut(),
            sentinel_node: std::ptr::null_mut(),
            excq_first: std::ptr::null_mut(),
            excq_last: std::ptr::null_mut(),
            arcs: Vec::new(),
            arcs_base: std::ptr::null_mut(),
            sentinel_arc: std::ptr::null_mut(),

            buckets: Vec::new(),
            buckets_base: std::ptr::null_mut(),
            l_bucket: 0,
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

            dummy_node: std::ptr::null_mut(),
            dnode: std::ptr::null_mut(),

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
    /// Returns a [`DimacsLoadError`] if the input is malformed (parse error)
    /// or fails the solver's build-time invariants ([`Cs2Error`]).
    pub fn from_dimacs(input: &str) -> Result<Self, DimacsLoadError> {
        let problem = parser::parse(input)?;
        Ok(Self::try_from(problem)?)
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
    ///
    /// # Safety
    /// Caller must pass valid node/arc pointers (i.e., into the live arenas).
    #[inline(always)]
    unsafe fn increase_flow(&mut self, i: *mut Node, j: *mut Node, a: *mut Arc, df: i64) {
        unsafe {
            (*i).excess -= df;
            (*j).excess += df;
            (*a).res_capacity -= df;
            (*(*a).sister).res_capacity += df;
            self.n_push += 1;
        }
    }

    /// Returns true when it is time for a price update.
    fn time_for_update(&self) -> bool {
        self.n_rel as f64 > self.n as f64 * UPDT_FREQ + self.n_src as f64 * UPDT_FREQ_S
    }

    // -----------------------------------------------------------------------
    // Excess queue utilities
    // -----------------------------------------------------------------------

    /// Reset the excess queue, marking all nodes as out-of-queue.
    ///
    /// # Safety
    /// Requires base pointers to be set (post-[`Self::cs2_initialize`]).
    #[inline(always)]
    unsafe fn reset_excess_q(&mut self) {
        unsafe {
            while !self.excq_first.is_null() {
                let next = (*self.excq_first).q_next;
                (*self.excq_first).q_next = self.sentinel_node;
                self.excq_first = next;
            }
            self.excq_last = std::ptr::null_mut();
        }
    }

    /// Returns true if node `i` is not in the excess queue.
    ///
    /// # Safety
    /// Caller must pass a valid node pointer.
    #[inline(always)]
    unsafe fn out_of_excess_q(&self, i: *mut Node) -> bool {
        unsafe { (*i).q_next == self.sentinel_node }
    }

    /// Returns true if the excess queue is empty.
    #[inline(always)]
    fn empty_excess_q(&self) -> bool {
        self.excq_first.is_null()
    }

    /// Returns true if the excess queue is non-empty.
    #[inline(always)]
    fn nonempty_excess_q(&self) -> bool {
        !self.excq_first.is_null()
    }

    /// Insert node `i` at the back of the excess queue.
    ///
    /// # Safety
    /// Caller must pass a valid node pointer.
    #[inline(always)]
    unsafe fn insert_to_excess_q(&mut self, i: *mut Node) {
        unsafe {
            if self.nonempty_excess_q() {
                (*self.excq_last).q_next = i;
            } else {
                self.excq_first = i;
            }
            (*i).q_next = std::ptr::null_mut();
            self.excq_last = i;
        }
    }

    /// Remove the front node from the excess queue. Returns the removed node pointer.
    ///
    /// # Safety
    /// Caller must ensure the queue is non-empty.
    #[inline(always)]
    unsafe fn remove_from_excess_q(&mut self) -> *mut Node {
        unsafe {
            let i = self.excq_first;
            self.excq_first = (*i).q_next;
            (*i).q_next = self.sentinel_node;
            if self.excq_first.is_null() {
                self.excq_last = std::ptr::null_mut();
            }
            i
        }
    }

    // -----------------------------------------------------------------------
    // Stack-queue utilities (excess queue used as a stack)
    // -----------------------------------------------------------------------

    /// Returns true if the stack-queue is non-empty.
    #[inline(always)]
    fn nonempty_stackq(&self) -> bool {
        self.nonempty_excess_q()
    }

    /// Reset the stack-queue.
    ///
    /// # Safety
    /// Requires base pointers to be set.
    #[inline(always)]
    unsafe fn reset_stackq(&mut self) {
        unsafe { self.reset_excess_q() };
    }

    /// Push node `i` onto the stack-queue.
    ///
    /// # Safety
    /// Caller must pass a valid node pointer.
    #[inline(always)]
    unsafe fn stackq_push(&mut self, i: *mut Node) {
        unsafe {
            (*i).q_next = self.excq_first;
            self.excq_first = i;
        }
    }

    /// Pop the front node from the stack-queue. Returns the popped node pointer.
    ///
    /// # Safety
    /// Caller must ensure the stack is non-empty.
    #[inline(always)]
    unsafe fn stackq_pop(&mut self) -> *mut Node {
        unsafe { self.remove_from_excess_q() }
    }

    // -----------------------------------------------------------------------
    // Bucket utilities
    // -----------------------------------------------------------------------

    /// Reset bucket `b` to empty (sentinel).
    ///
    /// # Safety
    /// Requires base pointers to be set.
    #[inline(always)]
    unsafe fn reset_bucket(&mut self, b: BucketIndex) {
        unsafe { (*self.buckets_base.add(b)).p_first = self.dnode };
    }

    /// Returns true if bucket `b` is non-empty.
    ///
    /// # Safety
    /// Requires base pointers to be set.
    #[inline(always)]
    unsafe fn nonempty_bucket(&self, b: BucketIndex) -> bool {
        unsafe { (*self.buckets_base.add(b)).p_first != self.dnode }
    }

    /// Insert node `i` into bucket `b`.
    ///
    /// # Safety
    /// Caller must pass a valid node pointer and bucket index.
    #[inline(always)]
    unsafe fn insert_to_bucket(&mut self, i: *mut Node, b: BucketIndex) {
        unsafe {
            let bucket = self.buckets_base.add(b);
            let old_first = (*bucket).p_first;
            (*i).b_next = old_first;
            if old_first != self.dnode {
                (*old_first).b_prev = i;
            }
            (*bucket).p_first = i;
        }
    }

    /// Get (pop) the first node from bucket `b`. Returns the node pointer.
    ///
    /// # Safety
    /// Caller must ensure bucket `b` is non-empty.
    #[inline(always)]
    unsafe fn get_from_bucket(&mut self, b: BucketIndex) -> *mut Node {
        unsafe {
            let bucket = self.buckets_base.add(b);
            let i = (*bucket).p_first;
            (*bucket).p_first = (*i).b_next;
            i
        }
    }

    /// Remove node `i` from bucket `b`.
    ///
    /// # Safety
    /// Caller must pass a valid node pointer and bucket index.
    #[inline(always)]
    unsafe fn remove_from_bucket(&mut self, i: *mut Node, b: BucketIndex) {
        unsafe {
            let bucket = self.buckets_base.add(b);
            if i == (*bucket).p_first {
                (*bucket).p_first = (*i).b_next;
            } else {
                let prev = (*i).b_prev;
                let next = (*i).b_next;
                (*prev).b_next = next;
                if next != self.dnode {
                    (*next).b_prev = prev;
                }
            }
        }
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
    #[inline(always)]
    fn exchange(&mut self, a: ArcIndex, b: ArcIndex) {
        if a != b {
            // SAFETY: arcs_base is valid post-cs2_initialize; a, b in range
            // because price_in/price_out only pass indices < sentinel_arc.
            unsafe {
                let ab = self.arcs_base;
                let a_ptr = ab.add(a);
                let b_ptr = ab.add(b);
                let sa = (*a_ptr).sister;
                let sb = (*b_ptr).sister;

                // Swap (res_capacity, cost, head) between *a and *b.
                let d_rez = (*a_ptr).res_capacity;
                let d_cost = (*a_ptr).cost;
                let d_head = (*a_ptr).head;
                (*a_ptr).res_capacity = (*b_ptr).res_capacity;
                (*a_ptr).cost = (*b_ptr).cost;
                (*a_ptr).head = (*b_ptr).head;
                (*b_ptr).res_capacity = d_rez;
                (*b_ptr).cost = d_cost;
                (*b_ptr).head = d_head;

                if a_ptr != sb {
                    (*b_ptr).sister = sa;
                    (*a_ptr).sister = sb;
                    (*sa).sister = b_ptr;
                    (*sb).sister = a_ptr;
                }
            }
            // Swap capacities (separate Vec).
            self.cap.swap(a, b);
        }
    }

    /// Allocate internal arrays and prepare for receiving arcs.
    fn allocate_arrays(&mut self) {
        // Reserve capacity for n+4 so cs2_initialize's two pushes after
        // pre_processing's drain cannot cause a reallocation that would
        // invalidate cached base pointers.
        let mut nodes = Vec::with_capacity(self.n + 4);
        nodes.resize(self.n + 2, Node::default());
        self.nodes = nodes;
        self.arcs = vec![Arc::default(); 2 * self.m + 1];
        self.cap = vec![0i64; 2 * self.m];
        self.arc_tail = vec![0usize; 2 * self.m];
        self.arc_first = vec![0i64; self.n + 2];

        // Cache base pointers now. These are stable for set_arc/pre_processing
        // because: nodes/arcs Vecs are never grown past their reserved
        // capacity, and Vec::drain (used in pre_processing) does not
        // reallocate. The two pushes in cs2_initialize fit within the
        // pre-reserved capacity.
        self.nodes_base = self.nodes.as_mut_ptr();
        self.arcs_base = self.arcs.as_mut_ptr();

        self.pos_current = 0;
        self.arc_current = 0;
        self.node_max = 0;
        self.node_min = self.n;
        self.max_cost = 0;
        self.total_p = 0;
        self.total_n = 0;
    }

    /// Add a directed arc from `tail_node_id` to `head_node_id` with the given bounds and cost.
    ///
    /// # Errors
    ///
    /// Returns [`Cs2Error::ArcOutOfBounds`] if either endpoint is outside
    /// `1..=n`, or [`Cs2Error::InvalidCapacityBounds`] if `low_bound < 0`
    /// or `low_bound > up_bound` (after the negative-`up_bound` sentinel
    /// is rewritten to `MAX_32`).
    pub fn set_arc(
        &mut self,
        tail_node_id: usize,
        head_node_id: usize,
        low_bound: i64,
        mut up_bound: i64,
        cost: Price,
    ) -> Result<(), Cs2Error> {
        if tail_node_id > self.n || head_node_id > self.n {
            return Err(Cs2Error::ArcOutOfBounds {
                tail: tail_node_id,
                head: head_node_id,
                max: self.n,
            });
        }
        if up_bound < 0 {
            up_bound = MAX_32;
            println!("Warning: Infinite capacity replaced by BIGGEST_FLOW");
        }
        if low_bound < 0 || low_bound > up_bound {
            return Err(Cs2Error::InvalidCapacityBounds {
                low: low_bound,
                up: up_bound,
            });
        }

        self.arc_first[tail_node_id + 1] += 1;
        self.arc_first[head_node_id + 1] += 1;
        self.i_node = tail_node_id;
        self.j_node = head_node_id;

        let pc = self.pos_current;
        let ac = self.arc_current;

        self.arc_tail[pc] = tail_node_id;
        self.arc_tail[pc + 1] = head_node_id;
        // SAFETY: nodes_base/arcs_base set in allocate_arrays; tail/head IDs
        // were validated against self.n above; ac < 2*m.
        unsafe {
            let head_p = self.nodes_base.add(head_node_id);
            let tail_p = self.nodes_base.add(tail_node_id);
            let fwd = self.arcs_base.add(ac);
            let rev = self.arcs_base.add(ac + 1);
            self.arcs[ac].head = head_p;
            self.arcs[ac].sister = rev;
            self.arcs[ac + 1].head = tail_p;
            self.arcs[ac + 1].sister = fwd;
        }
        self.arcs[ac].res_capacity = up_bound - low_bound;
        self.cap[pc] = up_bound;
        self.arcs[ac].cost = cost;
        self.arcs[ac + 1].res_capacity = 0;
        self.cap[pc + 1] = 0;
        self.arcs[ac + 1].cost = -cost;

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
        Ok(())
    }

    /// Set the supply (positive) or demand (negative) of a node. Must be called before [`set_arc`](Self::set_arc).
    ///
    /// # Errors
    ///
    /// Returns [`Cs2Error::NodeIdOutOfBounds`] if `id` exceeds the network's
    /// node count (the `num_nodes` passed to [`Self::new`]).
    pub fn set_supply_demand_of_node(&mut self, id: usize, excess: Excess) -> Result<(), Cs2Error> {
        if id > self.n {
            return Err(Cs2Error::NodeIdOutOfBounds { id, max: self.n });
        }
        self.nodes[id].excess = excess;
        if excess > 0 {
            self.total_p += excess;
        }
        if excess < 0 {
            self.total_n -= excess;
        }
        Ok(())
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
    ///
    /// # Errors
    ///
    /// - [`Cs2Error::Unbalanced`] if total supply != total demand.
    /// - [`Cs2Error::NodeIdsMustStartAtZeroOrOne`] if the smallest node id
    ///   seen by [`set_arc`](Self::set_arc) exceeds 1 (the internal
    ///   zero-based remap shifts by `node_min`, which only works for
    ///   `node_min <= 1`).
    fn pre_processing(&mut self) -> Result<(), Cs2Error> {
        if (self.total_p - self.total_n).abs() != 0 {
            return Err(Cs2Error::Unbalanced {
                supply: self.total_p,
                demand: self.total_n,
            });
        }

        // first arc from the first node.
        // SAFETY: arcs_base / nodes_base set in allocate_arrays.
        unsafe {
            self.nodes[self.node_min].first = self.arcs_base;

            // prefix-sum: arc_first[i] becomes position of first outgoing arc from node i
            for i in (self.node_min + 1)..=(self.node_max + 1) {
                self.arc_first[i] += self.arc_first[i - 1];
                self.nodes[i].first = self.arcs_base.add(self.arc_first[i] as usize);
            }
        }

        // reorder arcs by source node
        for i in self.node_min..self.node_max {
            // SAFETY: nodes_base/arcs_base set in allocate_arrays.
            let last = unsafe { self.nodes[i + 1].first.offset_from(self.arcs_base) as usize };
            let mut arc_num = self.arc_first[i] as usize;
            while arc_num < last {
                let mut tail_node_id = self.arc_tail[arc_num];
                while tail_node_id != i {
                    let arc_new_num = self.arc_first[tail_node_id] as usize;

                    // SAFETY: arcs_base set in allocate_arrays; arc_num and
                    // arc_new_num are valid arc indexes within sentinel_arc.
                    // `arc_new_num != arc_num` is an algorithm invariant in
                    // this branch: `arc_new_num = arc_first[tail_node_id]`
                    // stays within tail_node_id's contiguous block, and
                    // arc_num is within node i's (disjoint) block since
                    // `tail_node_id != i`. So the `&mut`s passed to
                    // `mem::swap` never alias.
                    unsafe {
                        let ab = self.arcs_base;
                        let p_new = ab.add(arc_new_num);
                        let p_old = ab.add(arc_num);

                        std::mem::swap(&mut (*p_new).head, &mut (*p_old).head);
                        std::mem::swap(&mut (*p_new).res_capacity, &mut (*p_old).res_capacity);
                        std::mem::swap(&mut (*p_new).cost, &mut (*p_old).cost);

                        // Sister fixup: if the two arcs are each other's
                        // sisters, the swap above already preserved the
                        // relationship; otherwise we need to swap sister
                        // fields and redirect the *other* arcs' sister
                        // pointers to the new positions.
                        if p_new != (*p_old).sister {
                            std::mem::swap(&mut (*p_new).sister, &mut (*p_old).sister);

                            let s1 = (*p_old).sister;
                            (*s1).sister = p_old;
                            let s2 = (*p_new).sister;
                            (*s2).sister = p_new;
                        }
                    }

                    // swap caps (separate Vec)
                    self.cap.swap(arc_new_num, arc_num);

                    self.arc_tail[arc_num] = self.arc_tail[arc_new_num];
                    self.arc_tail[arc_new_num] = tail_node_id;
                    self.arc_first[tail_node_id] += 1;
                    tail_node_id = self.arc_tail[arc_num];
                }
                arc_num += 1;
            }
        }

        // overflow test (computed but not enforced, matching C++)
        // SAFETY: arcs_base set in allocate_arrays; .first pointers are
        // valid arcs base offsets.
        for ndp in self.node_min..=self.node_max {
            let mut _cap_in: Excess = self.nodes[ndp].excess;
            let mut _cap_out: Excess = -self.nodes[ndp].excess;
            let a_start = unsafe { self.nodes[ndp].first.offset_from(self.arcs_base) as usize };
            let a_end = unsafe { self.nodes[ndp + 1].first.offset_from(self.arcs_base) as usize };
            for ac in a_start..a_end {
                if self.cap[ac] > 0 {
                    _cap_out += self.cap[ac];
                }
                if self.cap[ac] == 0 {
                    let sister_idx =
                        unsafe { self.arcs[ac].sister.offset_from(self.arcs_base) as usize };
                    _cap_in += self.cap[sister_idx];
                }
            }
        }

        if self.node_min > 1 {
            return Err(Cs2Error::NodeIdsMustStartAtZeroOrOne { min: self.node_min });
        }

        // adjustments: shift node base.
        // Vec::drain(0..node_min) shifts the remaining elements forward
        // in-place. The buffer base pointer (self.nodes_base) is unchanged,
        // but every head pointer stored in arcs is now off by `node_min`
        // node-slots — point them back to the correct (shifted) node.
        self.n = self.node_max - self.node_min + 1;
        let node_min = self.node_min;
        if node_min > 0 {
            self.nodes.drain(0..node_min);
            // SAFETY: every arc's head is either null (default, unused) or
            // points to a node within the original buffer; subtracting
            // node_min keeps it pointing to the same logical element
            // post-drain.
            unsafe {
                for arc in &mut self.arcs {
                    if !arc.head.is_null() {
                        arc.head = arc.head.sub(node_min);
                    }
                }
            }
        }

        // free internal arrays
        self.arc_first.clear();
        self.arc_tail.clear();
        Ok(())
    }

    /// Prepares the solver state for the cost-scaling iterations.
    ///
    /// Performs three key setup steps:
    /// 1. **Saturates negative-cost arcs** — pushes flow to capacity on every
    ///    arc with negative cost, converting the zero flow into a 0-optimal
    ///    pseudoflow.
    /// 2. **Scales costs** — multiplies all arc costs by `dn = n + 1` so that
    ///    epsilon-optimality arithmetic uses integers throughout.
    /// 3. **Allocates buckets** — creates the bucket vec used by
    ///    [`price_update`](Self::price_update) and
    ///    [`price_refine`](Self::price_refine), sized to `O(n * scale_factor)`.
    fn cs2_initialize(&mut self) {
        self.f_scale = SCALE_DEFAULT;
        // Base pointers were already set in allocate_arrays. pre_processing
        // may have drained `nodes` (in-place, no reallocation), so the base
        // address is unchanged.
        debug_assert!(!self.nodes_base.is_null());
        debug_assert!(!self.arcs_base.is_null());

        // SAFETY: all bases valid post-allocate_arrays/pre_processing.
        unsafe {
            self.sentinel_node = self.nodes_base.add(self.n);
            self.sentinel_arc = self.arcs_base.add(self.m);

            for i in 0..self.n {
                let n_ptr = self.nodes_base.add(i);
                (*n_ptr).price = 0;
                (*n_ptr).suspended = (*n_ptr).first;
                (*n_ptr).q_next = self.sentinel_node;
            }

            (*self.sentinel_node).first = self.sentinel_arc;
            (*self.sentinel_node).suspended = self.sentinel_arc;

            // saturate negative arcs
            for i in 0..self.n {
                let i_ptr = self.nodes_base.add(i);
                let a_stop = (*self.nodes_base.add(i + 1)).suspended;
                let mut a = (*i_ptr).first;
                while a < a_stop {
                    if (*a).cost < 0 {
                        let df = (*a).res_capacity;
                        if df > 0 {
                            let j_ptr = (*a).head;
                            self.increase_flow(i_ptr, j_ptr, a, df);
                        }
                    }
                    a = a.add(1);
                }
            }

            self.dn = (self.n + 1) as Price;
            if self.no_zero_cycles {
                self.dn *= 2;
            }

            // Scale all arc costs by dn.
            let mut a = self.arcs_base;
            while a < self.sentinel_arc {
                (*a).cost *= self.dn;
                a = a.add(1);
            }

            if self.no_zero_cycles {
                let mut a = self.arcs_base;
                while a < self.sentinel_arc {
                    let sister = (*a).sister;
                    if (*a).cost == 0 && (*sister).cost == 0 {
                        (*a).cost = 1;
                        (*sister).cost = -1;
                    }
                    a = a.add(1);
                }
            }

            if (self.max_cost as f64) * (self.dn as f64) > MAX_64 as f64 {
                println!("Warning: Arc lengths too large, overflow possible");
            }
            self.mmc = self.max_cost * self.dn;

            self.linf = (self.dn as f64 * self.f_scale.ceil() + 2.0) as usize;

            self.buckets = vec![Bucket::default(); self.linf];
            self.buckets_base = self.buckets.as_mut_ptr();
            self.l_bucket = self.linf;

            // dnode: extra node used as bucket sentinel.
            // self.nodes.capacity() was reserved to n+4 in allocate_arrays so
            // this push does not reallocate (would invalidate nodes_base).
            let dnode_idx = self.nodes.len();
            self.nodes.push(Node::default());
            debug_assert!(self.nodes.as_mut_ptr() == self.nodes_base);
            self.dnode = self.nodes_base.add(dnode_idx);

            for b in 0..self.l_bucket {
                self.reset_bucket(b);
            }

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
            let dummy_idx = self.nodes.len();
            self.nodes.push(Node::default());
            self.dummy_node = self.nodes_base.add(dummy_idx);

            self.excq_first = std::ptr::null_mut();
            self.excq_last = std::ptr::null_mut();
        }
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
    fn up_node_scan(&mut self, i_ptr: *mut Node) {
        // SAFETY: only called via price_update -> refine -> cs2, after
        // cs2_initialize set base pointers; i_ptr is a live node.
        unsafe {
            self.n_scan += 1;
            let i_rank = (*i_ptr).rank;
            let i_price = (*i_ptr).price;
            let mut a = (*i_ptr).first;
            let a_stop = (*i_ptr.add(1)).suspended;
            let linf_i = self.linf as i64;
            let eps = self.epsilon;

            while a < a_stop {
                let ra = (*a).sister;
                if (*ra).res_capacity > 0 {
                    let j_ptr = (*a).head;
                    let j_rank = (*j_ptr).rank;
                    if j_rank > i_rank {
                        let rc = (*j_ptr).price + (*ra).cost - i_price;
                        let j_new_rank = if rc < 0 {
                            i_rank
                        } else {
                            let dr = rc / eps;
                            if dr < linf_i { i_rank + dr + 1 } else { linf_i }
                        };
                        if j_rank > j_new_rank {
                            (*j_ptr).rank = j_new_rank;
                            (*j_ptr).current = ra;
                            if j_rank < linf_i {
                                self.remove_from_bucket(j_ptr, j_rank as usize);
                            }
                            self.insert_to_bucket(j_ptr, j_new_rank as usize);
                        }
                    }
                }
                a = a.add(1);
            }

            (*i_ptr).price -= i_rank * eps;
            (*i_ptr).rank = -1;
        }
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
        // SAFETY: only called after cs2_initialize.
        unsafe {
            self.n_update += 1;
            let linf_i = self.linf as i64;
            let sentinel_node = self.sentinel_node;

            let mut p = self.nodes_base;
            while p < sentinel_node {
                if (*p).excess < 0 {
                    self.insert_to_bucket(p, 0);
                    (*p).rank = 0;
                } else {
                    (*p).rank = linf_i;
                }
                p = p.add(1);
            }

            let mut remain = self.total_excess;
            if (remain as f64) < 0.5 {
                return;
            }

            let mut b = 0usize;
            while b < self.l_bucket {
                while self.nonempty_bucket(b) {
                    let i_ptr = self.get_from_bucket(b);
                    self.up_node_scan(i_ptr);
                    let i_exc = (*i_ptr).excess;
                    if i_exc > 0 {
                        remain -= i_exc;
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
            let price_min = self.price_min;

            let mut p = self.nodes_base;
            while p < sentinel_node {
                let rank = (*p).rank;
                if rank >= 0 {
                    if rank < linf_i {
                        self.remove_from_bucket(p, rank as usize);
                    }
                    if (*p).price > price_min {
                        (*p).price -= dp;
                    }
                }
                p = p.add(1);
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
    #[inline]
    fn relabel(&mut self, i_ptr: *mut Node) -> Result<bool, Cs2Error> {
        // SAFETY: only called from discharge/price_in, which run after
        // cs2_initialize; i_ptr is a valid live node.
        unsafe {
            let mut p_max = self.price_min;
            let i_price = (*i_ptr).price;
            let mut a_max: *mut Arc = std::ptr::null_mut();

            let current = (*i_ptr).current;
            let a_stop = (*i_ptr.add(1)).suspended;

            // scan 1/2: from current+1 to end
            let mut a = current.add(1);
            while a < a_stop {
                if (*a).res_capacity > 0 {
                    let head = (*a).head;
                    let dp = (*head).price - (*a).cost;
                    if dp > p_max {
                        if i_price < dp {
                            (*i_ptr).current = a;
                            return Ok(true);
                        }
                        p_max = dp;
                        a_max = a;
                    }
                }
                a = a.add(1);
            }

            // scan 2/2: from first to current+1
            let a_start2 = (*i_ptr).first;
            let a_stop2 = current.add(1);
            let mut a = a_start2;
            while a < a_stop2 {
                if (*a).res_capacity > 0 {
                    let head = (*a).head;
                    let dp = (*head).price - (*a).cost;
                    if dp > p_max {
                        if i_price < dp {
                            (*i_ptr).current = a;
                            return Ok(true);
                        }
                        p_max = dp;
                        a_max = a;
                    }
                }
                a = a.add(1);
            }

            if p_max != self.price_min {
                (*i_ptr).price = p_max - self.epsilon;
                (*i_ptr).current = a_max;
            } else if (*i_ptr).suspended == (*i_ptr).first {
                if (*i_ptr).excess == 0 {
                    (*i_ptr).price = self.price_min;
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
    fn discharge(&mut self, i_ptr: *mut Node) -> Result<(), Cs2Error> {
        // SAFETY: invoked from refine() after cs2_initialize; i_ptr is valid.
        unsafe {
            self.n_discharge += 1;

            let mut a = (*i_ptr).current;
            let mut j_ptr = (*a).head;

            // check admissible
            let is_admissible =
                (*a).res_capacity > 0 && (*i_ptr).price + (*a).cost < (*j_ptr).price;
            if !is_admissible {
                self.relabel(i_ptr)?;
                a = (*i_ptr).current;
                j_ptr = (*a).head;
            }

            loop {
                let j_exc = (*j_ptr).excess;
                if j_exc >= 0 {
                    let df = (*i_ptr).excess.min((*a).res_capacity);
                    if j_exc == 0 {
                        self.n_src += 1;
                    }
                    self.increase_flow(i_ptr, j_ptr, a, df);
                    if self.out_of_excess_q(j_ptr) {
                        self.insert_to_excess_q(j_ptr);
                    }
                } else {
                    let df = (*i_ptr).excess.min((*a).res_capacity);
                    self.increase_flow(i_ptr, j_ptr, a, df);
                    let j_exc_after = (*j_ptr).excess;
                    if j_exc_after >= 0 {
                        if j_exc_after > 0 {
                            self.n_src += 1;
                            self.relabel(j_ptr)?;
                            self.insert_to_excess_q(j_ptr);
                        }
                        self.total_excess += j_exc;
                    } else {
                        self.total_excess -= df;
                    }
                }

                let i_exc_after = (*i_ptr).excess;
                if i_exc_after <= 0 {
                    self.n_src -= 1;
                }
                if i_exc_after <= 0 || self.flag_price != 0 {
                    break;
                }

                self.relabel(i_ptr)?;
                a = (*i_ptr).current;
                j_ptr = (*a).head;
            }

            (*i_ptr).current = a;
            Ok(())
        }
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
        // SAFETY: called only after cs2_initialize set base pointers.
        unsafe {
            let arcs_base = self.arcs_base;
            let mut bad_found = 0;
            let mut n_in_bad = 0;
            let cut_on_i = self.cut_on as i64;
            let sentinel_node = self.sentinel_node;

            'restart: loop {
                let mut i_ptr = self.nodes_base;
                while i_ptr < sentinel_node {
                    let initial_first = (*i_ptr).first;
                    let suspended = (*i_ptr).suspended;
                    let i_price = (*i_ptr).price;

                    let mut a = initial_first;
                    while a > suspended {
                        a = a.sub(1);
                        let j_ptr = (*a).head;
                        let rc = i_price + (*a).cost - (*j_ptr).price;
                        if rc < 0 && (*a).res_capacity > 0 {
                            if bad_found == 0 {
                                bad_found = 1;
                                self.update_cut_off();
                                continue 'restart;
                            }
                            let df = (*a).res_capacity;
                            self.increase_flow(i_ptr, j_ptr, a, df);

                            let ra = (*a).sister;
                            let j2_ptr = (*a).head;

                            (*i_ptr).first = (*i_ptr).first.sub(1);
                            let b_idx = (*i_ptr).first.offset_from(arcs_base) as usize;
                            let a_idx = a.offset_from(arcs_base) as usize;
                            self.exchange(a_idx, b_idx);

                            if ra < (*j2_ptr).first {
                                (*j2_ptr).first = (*j2_ptr).first.sub(1);
                                let rb_idx = (*j2_ptr).first.offset_from(arcs_base) as usize;
                                let ra_idx = ra.offset_from(arcs_base) as usize;
                                self.exchange(ra_idx, rb_idx);
                            }

                            n_in_bad += 1;
                        } else if rc < cut_on_i && rc > -cut_on_i {
                            (*i_ptr).first = (*i_ptr).first.sub(1);
                            let b_idx = (*i_ptr).first.offset_from(arcs_base) as usize;
                            let a_idx = a.offset_from(arcs_base) as usize;
                            self.exchange(a_idx, b_idx);
                        }
                    }
                    i_ptr = i_ptr.add(1);
                }
                break;
            }

            if n_in_bad != 0 {
                self.n_bad_pricein += 1;

                self.total_excess = 0;
                self.n_src = 0;
                self.reset_excess_q();

                let mut i_ptr = self.nodes_base;
                while i_ptr < sentinel_node {
                    (*i_ptr).current = (*i_ptr).first;
                    let i_exc = (*i_ptr).excess;
                    if i_exc > 0 {
                        self.total_excess += i_exc;
                        self.n_src += 1;
                        self.insert_to_excess_q(i_ptr);
                    }
                    i_ptr = i_ptr.add(1);
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
    fn refine(&mut self) -> Result<(), Cs2Error> {
        // SAFETY: called from cs2 after cs2_initialize set base pointers.
        unsafe {
            self.n_refine += 1;
            self.n_ref += 1;
            self.n_rel = 0;
            let mut pr_in_int: i32 = 0;
            let sentinel_node = self.sentinel_node;

            self.total_excess = 0;
            self.n_src = 0;
            self.reset_excess_q();

            self.time_for_price_in = TIME_FOR_PRICE_IN1;

            let mut p = self.nodes_base;
            while p < sentinel_node {
                (*p).current = (*p).first;
                let i_exc = (*p).excess;
                if i_exc > 0 {
                    self.total_excess += i_exc;
                    self.n_src += 1;
                    self.insert_to_excess_q(p);
                }
                p = p.add(1);
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

                let i_ptr = self.remove_from_excess_q();

                if (*i_ptr).excess > 0 {
                    self.discharge(i_ptr)?;

                    if self.time_for_update() || self.flag_price != 0 {
                        if (*i_ptr).excess > 0 {
                            self.insert_to_excess_q(i_ptr);
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
        // SAFETY: called from cs2 after cs2_initialize.
        unsafe {
            self.n_prefine += 1;
            let mut eps_optimal = true;
            let mut snc: i32 = 0;
            let linf_i = self.linf as i64;
            let eps = self.epsilon;
            let sentinel_node = self.sentinel_node;

            self.snc_max = if self.n_ref >= START_CYCLE_CANCEL {
                MAX_CYCLES_CANCELLED
            } else {
                0
            };

            // main loop
            loop {
                let mut nnc: i32 = 0;
                let mut p = self.nodes_base;
                while p < sentinel_node {
                    (*p).rank = 0;
                    (*p).inp = Color::White;
                    (*p).current = (*p).first;
                    p = p.add(1);
                }
                self.reset_stackq();

                let mut root = self.nodes_base;
                while root < sentinel_node {
                    if (*root).inp == Color::Black {
                        root = root.add(1);
                        continue;
                    }
                    (*root).b_next = std::ptr::null_mut();
                    let mut i_ptr = root;

                    // depth first search
                    'dfs: loop {
                        (*i_ptr).inp = Color::Grey;
                        let mut a = (*i_ptr).current;
                        let a_stop = (*i_ptr.add(1)).suspended;
                        let i_price = (*i_ptr).price;
                        let mut stepped = false;

                        while a < a_stop {
                            if (*a).res_capacity > 0 {
                                let j_ptr = (*a).head;
                                let rc = i_price + (*a).cost - (*j_ptr).price;
                                if rc < 0 {
                                    let j_inp = (*j_ptr).inp;
                                    if j_inp == Color::White {
                                        // step forward
                                        (*i_ptr).current = a;
                                        (*j_ptr).b_next = i_ptr;
                                        i_ptr = j_ptr;
                                        stepped = true;
                                        break;
                                    }
                                    if j_inp == Color::Grey {
                                        // cycle detected
                                        eps_optimal = false;
                                        nnc += 1;
                                        (*i_ptr).current = a;

                                        // find min capacity on cycle
                                        let mut is = i_ptr;
                                        let mut ir = i_ptr;
                                        let mut df: i64 = MAX_32;
                                        loop {
                                            let ar = (*ir).current;
                                            let ar_cap = (*ar).res_capacity;
                                            if ar_cap <= df {
                                                df = ar_cap;
                                                is = ir;
                                            }
                                            if ir == j_ptr {
                                                break;
                                            }
                                            ir = (*ir).b_next;
                                        }

                                        // push flow around cycle
                                        ir = i_ptr;
                                        loop {
                                            let ar = (*ir).current;
                                            let head = (*ar).head;
                                            self.increase_flow(ir, head, ar, df);
                                            if ir == j_ptr {
                                                break;
                                            }
                                            ir = (*ir).b_next;
                                        }

                                        if is != i_ptr {
                                            ir = i_ptr;
                                            while ir != is {
                                                (*ir).inp = Color::White;
                                                ir = (*ir).b_next;
                                            }
                                            i_ptr = is;
                                            stepped = true;
                                            break;
                                        }
                                        // is == i: continue scanning
                                    }
                                }
                            }
                            a = a.add(1);
                        }

                        if stepped {
                            continue 'dfs;
                        }

                        // step back
                        (*i_ptr).inp = Color::Black;
                        self.n_prscan1 += 1;
                        let j = (*i_ptr).b_next;
                        self.stackq_push(i_ptr);
                        if j.is_null() {
                            break 'dfs;
                        }
                        i_ptr = j;
                        (*i_ptr).current = (*i_ptr).current.add(1);
                    }
                    root = root.add(1);
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
                    let i_ptr = self.stackq_pop();
                    let i_rank = (*i_ptr).rank;
                    let i_price = (*i_ptr).price;
                    let mut a = (*i_ptr).first;
                    let a_stop = (*i_ptr.add(1)).suspended;
                    while a < a_stop {
                        if (*a).res_capacity > 0 {
                            let j_ptr = (*a).head;
                            let rc = i_price + (*a).cost - (*j_ptr).price;
                            if rc < 0 {
                                let dr = (-rc as f64 - 0.5) / eps as f64;
                                let j_rank = dr as i64 + i_rank;
                                if j_rank < linf_i && j_rank > (*j_ptr).rank {
                                    (*j_ptr).rank = j_rank;
                                }
                            }
                        }
                        a = a.add(1);
                    }
                    if i_rank > 0 {
                        if i_rank as usize > bmax {
                            bmax = i_rank as usize;
                        }
                        self.insert_to_bucket(i_ptr, i_rank as usize);
                    }
                }

                if bmax == 0 {
                    break;
                }

                let mut b = bmax;
                while b >= 1 {
                    let i_rank = b as i64;
                    let dp = i_rank * eps;

                    while self.nonempty_bucket(b) {
                        let i_ptr = self.get_from_bucket(b);
                        self.n_prscan += 1;

                        let i_price = (*i_ptr).price;
                        let mut a = (*i_ptr).first;
                        let a_stop = (*i_ptr.add(1)).suspended;
                        while a < a_stop {
                            if (*a).res_capacity > 0 {
                                let j_ptr = (*a).head;
                                let j_rank = (*j_ptr).rank;
                                if j_rank < i_rank {
                                    let rc = i_price + (*a).cost - (*j_ptr).price;
                                    let j_new_rank = if rc < 0 {
                                        i_rank
                                    } else {
                                        let dr = rc / eps;
                                        if dr < linf_i { i_rank - (dr + 1) } else { 0 }
                                    };
                                    if j_rank < j_new_rank {
                                        if eps_optimal {
                                            (*j_ptr).rank = j_new_rank;
                                            if j_rank > 0 {
                                                self.remove_from_bucket(j_ptr, j_rank as usize);
                                            }
                                            self.insert_to_bucket(j_ptr, j_new_rank as usize);
                                        } else {
                                            let df = (*a).res_capacity;
                                            self.increase_flow(i_ptr, j_ptr, a, df);
                                        }
                                    }
                                }
                            }
                            a = a.add(1);
                        }

                        (*i_ptr).price -= dp;
                    }
                    b -= 1;
                }

                if !eps_optimal {
                    break;
                }
            }

            // finish: saturate non-epsilon-optimal arcs if needed
            if !eps_optimal {
                let mut p = self.nodes_base;
                while p < sentinel_node {
                    let i_price = (*p).price;
                    let mut a = (*p).first;
                    let a_stop = (*p.add(1)).suspended;
                    while a < a_stop {
                        let j_ptr = (*a).head;
                        let rc = i_price + (*a).cost - (*j_ptr).price;
                        if rc < -eps {
                            let df = (*a).res_capacity;
                            if df > 0 {
                                self.increase_flow(p, j_ptr, a, df);
                            }
                        }
                        a = a.add(1);
                    }
                    p = p.add(1);
                }
            }

            eps_optimal
        }
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
        // SAFETY: called after cs2_initialize.
        unsafe {
            self.n_prefine += 1;
            // Whether the graph is cycle free
            // (expected for a correct, finished solution).
            let mut cycle_free = true;
            let linf_i = self.linf as i64;

            let sentinel_node = self.sentinel_node;

            loop {
                let mut p = self.nodes_base;
                while p < sentinel_node {
                    (*p).rank = 0;
                    (*p).inp = Color::White;
                    (*p).current = (*p).first;
                    p = p.add(1);
                }
                self.reset_stackq();

                let mut root = self.nodes_base;
                while root < sentinel_node {
                    if (*root).inp == Color::Black {
                        root = root.add(1);
                        continue;
                    }
                    (*root).b_next = std::ptr::null_mut();
                    let mut i_ptr = root;

                    'dfs: loop {
                        (*i_ptr).inp = Color::Grey;
                        let mut a = (*i_ptr).suspended;
                        let a_stop = (*i_ptr.add(1)).suspended;
                        let i_price = (*i_ptr).price;
                        let mut stepped = false;

                        while a < a_stop {
                            if (*a).res_capacity > 0 {
                                let j_ptr = (*a).head;
                                let rc = i_price + (*a).cost - (*j_ptr).price;
                                if rc < 0 {
                                    let j_inp = (*j_ptr).inp;
                                    if j_inp == Color::White {
                                        (*i_ptr).current = a;
                                        (*j_ptr).b_next = i_ptr;
                                        i_ptr = j_ptr;
                                        stepped = true;
                                        break;
                                    }
                                    if j_inp == Color::Grey {
                                        cycle_free = false;
                                    }
                                }
                            }
                            a = a.add(1);
                        }

                        if stepped {
                            continue 'dfs;
                        }

                        (*i_ptr).inp = Color::Black;
                        self.n_prscan1 += 1;
                        let j = (*i_ptr).b_next;
                        self.stackq_push(i_ptr);
                        if j.is_null() {
                            break 'dfs;
                        }
                        i_ptr = j;
                        (*i_ptr).current = (*i_ptr).current.add(1);
                    }
                    root = root.add(1);
                }

                if !cycle_free {
                    break;
                }
                let mut bmax: usize = 0;

                while self.nonempty_stackq() {
                    self.n_prscan2 += 1;
                    let i_ptr = self.stackq_pop();
                    let i_rank = (*i_ptr).rank;
                    let i_price = (*i_ptr).price;
                    let mut a = (*i_ptr).suspended;
                    let a_stop = (*i_ptr.add(1)).suspended;
                    while a < a_stop {
                        if (*a).res_capacity > 0 {
                            let j_ptr = (*a).head;
                            let rc = i_price + (*a).cost - (*j_ptr).price;
                            if rc < 0 {
                                let dr = -rc;
                                let j_rank = dr + i_rank;
                                if j_rank < linf_i && j_rank > (*j_ptr).rank {
                                    (*j_ptr).rank = j_rank;
                                }
                            }
                        }
                        a = a.add(1);
                    }
                    if i_rank > 0 {
                        if i_rank as usize > bmax {
                            bmax = i_rank as usize;
                        }
                        self.insert_to_bucket(i_ptr, i_rank as usize);
                    }
                }

                if bmax == 0 {
                    break;
                }

                let mut b = bmax;
                while b >= 1 {
                    let i_rank = b as i64;
                    let dp = i_rank;

                    while self.nonempty_bucket(b) {
                        let i_ptr = self.get_from_bucket(b);
                        self.n_prscan += 1;

                        let i_price = (*i_ptr).price;
                        let mut a = (*i_ptr).suspended;
                        let a_stop = (*i_ptr.add(1)).suspended;
                        while a < a_stop {
                            if (*a).res_capacity > 0 {
                                let j_ptr = (*a).head;
                                let j_rank = (*j_ptr).rank;
                                if j_rank < i_rank {
                                    let rc = i_price + (*a).cost - (*j_ptr).price;
                                    let j_new_rank = if rc < 0 {
                                        i_rank
                                    } else {
                                        let dr = rc;
                                        if dr < linf_i { i_rank - (dr + 1) } else { 0 }
                                    };
                                    if j_rank < j_new_rank && cycle_free {
                                        (*j_ptr).rank = j_new_rank;
                                        if j_rank > 0 {
                                            self.remove_from_bucket(j_ptr, j_rank as usize);
                                        }
                                        self.insert_to_bucket(j_ptr, j_new_rank as usize);
                                    }
                                }
                            }
                            a = a.add(1);
                        }

                        (*i_ptr).price -= dp;
                    }
                    b -= 1;
                }

                if !cycle_free {
                    break;
                }
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
        // SAFETY: called from cs2 after cs2_initialize.
        unsafe {
            let arcs_base = self.arcs_base;
            let n_cut_off = -self.cut_off;
            let cut_off = self.cut_off;
            let sentinel_node = self.sentinel_node;

            let mut i_ptr = self.nodes_base;
            while i_ptr < sentinel_node {
                let a_stop = (*i_ptr.add(1)).suspended;
                let i_price = (*i_ptr).price;
                let mut a = (*i_ptr).first;
                while a < a_stop {
                    let j_ptr = (*a).head;
                    let rc = (i_price + (*a).cost - (*j_ptr).price) as f64;
                    let sister = (*a).sister;
                    if (rc > cut_off && (*sister).res_capacity <= 0)
                        || (rc < n_cut_off && (*a).res_capacity <= 0)
                    {
                        let b = (*i_ptr).first;
                        (*i_ptr).first = b.add(1);
                        let a_idx = a.offset_from(arcs_base) as usize;
                        let b_idx = b.offset_from(arcs_base) as usize;
                        self.exchange(a_idx, b_idx);
                    }
                    a = a.add(1);
                }
                i_ptr = i_ptr.add(1);
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
        let arcs_base = self.arcs_base;
        let nodes_base = self.nodes_base;
        for i in 0..self.n {
            // SAFETY: post-cs2_initialize, all .suspended pointers are valid.
            let a_start = unsafe { self.nodes[i].suspended.offset_from(arcs_base) as usize };
            let a_stop = unsafe { self.nodes[i + 1].suspended.offset_from(arcs_base) as usize };
            for a in a_start..a_stop {
                if self.cap[a] > 0 {
                    let fa = self.cap[a] - self.arcs[a].res_capacity;
                    if fa < 0 {
                        ans = false;
                        break;
                    }
                    self.node_balance[i] -= fa;
                    let head_idx = unsafe { self.arcs[a].head.offset_from(nodes_base) as usize };
                    self.node_balance[head_idx] += fa;
                }
            }
        }
        for i in 0..self.n {
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
        let arcs_base = self.arcs_base;
        let nodes_base = self.nodes_base;
        for i in 0..self.n {
            let a_start = unsafe { self.nodes[i].suspended.offset_from(arcs_base) as usize };
            let a_stop = unsafe { self.nodes[i + 1].suspended.offset_from(arcs_base) as usize };
            for a in a_start..a_stop {
                if self.arcs[a].res_capacity > 0 {
                    let j = unsafe { self.arcs[a].head.offset_from(nodes_base) as usize };
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

        let arcs_base = self.arcs_base;
        let nodes_base = self.nodes_base;
        for i in 0..self.n {
            let ni = n_node(i, self.node_min);
            let a_start = unsafe { self.nodes[i].suspended.offset_from(arcs_base) as usize };
            let a_stop = unsafe { self.nodes[i + 1].suspended.offset_from(arcs_base) as usize };
            for a in a_start..a_stop {
                if self.cap[a] > 0 {
                    let head_idx = unsafe { self.arcs[a].head.offset_from(nodes_base) as usize };
                    println!(
                        "f {:7} {:7} {:10}",
                        ni,
                        n_node(head_idx, self.node_min),
                        self.cap[a] - self.arcs[a].res_capacity
                    );
                }
            }
        }

        if comp_duals {
            let mut min_price = MAX_32;
            for i in 0..self.n {
                min_price = min_price.min(self.nodes[i].price);
            }
            for i in 0..self.n {
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
        let arcs_base = self.arcs_base;
        let nodes_base = self.nodes_base;
        for i in 0..self.n {
            let ni = n_node(i, self.node_min);
            println!("\nNode {ni}");
            let a_start = unsafe { self.nodes[i].suspended.offset_from(arcs_base) as usize };
            let a_stop = unsafe { self.nodes[i + 1].suspended.offset_from(arcs_base) as usize };
            for a in a_start..a_stop {
                let head_idx = unsafe { self.arcs[a].head.offset_from(nodes_base) as usize };
                println!(
                    " {{{}}} {} -> {}  cap: {}  cost: {}",
                    a,
                    ni,
                    n_node(head_idx, self.node_min),
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
            // SAFETY: arcs_base valid post-cs2_initialize.
            unsafe {
                for a in 0..self.m {
                    let arc = self.arcs_base.add(a);
                    if (*arc).cost == 1 {
                        let sister = (*arc).sister;
                        // Invariant from cs2_initialize: when an arc's cost
                        // is rewritten to 1, its sister's is set to -1.
                        debug_assert_eq!((*sister).cost, -1);
                        (*arc).cost = 0;
                        (*sister).cost = 0;
                    }
                }
            }
        }

        let mut obj_internal: f64 = 0.0;
        for a in 0..self.m {
            let cs = self.arcs[a].cost / self.dn;
            if self.cap[a] > 0 {
                let flow = self.cap[a] - self.arcs[a].res_capacity;
                if flow != 0 {
                    obj_internal += cs as f64 * flow as f64;
                }
            }
            self.arcs[a].cost = cs;
        }

        for i in 0..self.n {
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
        self.pre_processing()?;

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
        self.pre_processing()?;

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
        let arcs_base = s.arcs_base;
        let nodes_base = s.nodes_base;
        (0..s.n).flat_map(move |i| {
            // SAFETY: pointers stored in node fields are valid arcs/nodes
            // offsets after cs2_initialize.
            let a_start = unsafe { s.nodes[i].suspended.offset_from(arcs_base) as usize };
            let a_stop = unsafe { s.nodes[i + 1].suspended.offset_from(arcs_base) as usize };
            (a_start..a_stop).filter_map(move |a| {
                if s.cap[a] > 0 {
                    let flow = s.cap[a] - s.arcs[a].res_capacity;
                    let tail = n_node(i, s.node_min) as usize;
                    let head_idx = unsafe { s.arcs[a].head.offset_from(nodes_base) as usize };
                    let head = n_node(head_idx, s.node_min) as usize;
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
        (0..s.n).map(move |i| (n_node(i, s.node_min) as usize, s.nodes[i].price))
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

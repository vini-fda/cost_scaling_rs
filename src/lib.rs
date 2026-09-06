//! CS2 min-cost max-flow scaling algorithm.
//!
//! This is a Rust implementation of the CS2 min-cost-max-flow scaling algorithm,
//! translated from the original C implementation.

#![warn(missing_docs)]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::unreachable))]
#![cfg_attr(not(test), deny(clippy::todo))]
#![cfg_attr(not(test), deny(clippy::unimplemented))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]

#[doc(hidden)]
pub mod parser;
#[doc(hidden)]
pub mod problem_generators;

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
/// `PRICE_OUT_START` may not be less than 1
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
    /// Next node in push-queue (or `McmfCs2::sentinel_node` if out of queue).
    q_next: *mut Node,
    /// Next node in bucket-list.
    b_next: *mut Node,
    /// Previous node in bucket-list.
    b_prev: *mut Node,
    /// Bucket number.
    rank: i64,
    /// DFS visit color (White/Grey/Black) used in `price_refine` and `compute_prices`.
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
    /// A supply, demand, or intermediate excess cannot be represented in `i64`.
    ExcessOverflow,
    /// Graph dimensions are negative or exceed the addressable array sizes.
    InvalidProblemSize,
    /// A parsed node identifier cannot be represented as an unsigned index.
    InvalidNodeId {
        /// The identifier from the input.
        id: i64,
    },
    /// The number of inserted arcs differs from the declared number.
    ArcCountMismatch {
        /// Number of arcs declared at construction.
        expected: usize,
        /// Number of arcs supplied (including a rejected excess insertion).
        actual: usize,
    },
    /// The solver has already started solving, or supplies were set after arcs.
    InvalidBuildState,
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
    /// Legacy error retained for compatibility. The solver now preserves the
    /// declared `1..=n` node domain and no longer returns this variant.
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
            Cs2Error::ExcessOverflow => write!(f, "supply, demand, or excess overflowed"),
            Cs2Error::InvalidProblemSize => write!(f, "invalid or unaddressable graph dimensions"),
            Cs2Error::InvalidNodeId { id } => write!(f, "invalid node identifier {id}"),
            Cs2Error::ArcCountMismatch { expected, actual } => {
                write!(f, "expected {expected} arcs, received {actual}")
            }
            Cs2Error::InvalidBuildState => {
                write!(f, "solver is no longer in the required build phase")
            }
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
/// let solution = solver.min_cost()?;
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
/// let solution = solver.min_cost()?;
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
    /// True once preprocessing starts; builder arrays cannot be reused afterward.
    started: bool,

    /// Check feasibility/optimality during `min_cost`. Note that this adds high overhead. False by default.
    check_solution: bool,
    /// Enable to compute prices during `min_cost`. False by default.
    comp_duals: bool,

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
    /// Number of `l_bucket` + 1.
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
    /// Multiplier to produce `cut_on` and `cut_off` from n and epsilon.
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

/// Keep all arena lengths and pointer offsets addressable before allocating.
fn valid_dimensions(n: usize, m: usize) -> bool {
    n <= (isize::MAX as usize / size_of::<Node>()).saturating_sub(4)
        && m <= (isize::MAX as usize / size_of::<Arc>()).saturating_sub(1) / 2
}

/// Checked reduced-cost arithmetic; unsupported intermediates are errors, not wrapping costs.
#[inline]
fn reduced_cost(tail: Price, cost: Price, head: Price) -> Result<Price, Cs2Error> {
    tail.checked_add(cost)
        .and_then(|value| value.checked_sub(head))
        .ok_or(Cs2Error::PriceOverflow)
}

/// Match C's post-increment comparison, including resetting the counter on a trigger.
fn price_in_due(counter: &mut i32, interval: i32) -> bool {
    let due = *counter > interval;
    *counter = if due { 0 } else { *counter + 1 };
    due
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
        let n = usize::try_from(problem.nodes).map_err(|_| Cs2Error::InvalidProblemSize)?;
        let m = usize::try_from(problem.arcs_count).map_err(|_| Cs2Error::InvalidProblemSize)?;
        if !valid_dimensions(n, m) {
            return Err(Cs2Error::InvalidProblemSize);
        }
        if problem.arcs.len() != m {
            return Err(Cs2Error::ArcCountMismatch {
                expected: m,
                actual: problem.arcs.len(),
            });
        }
        let mut solver = McmfCs2::new(n, m);
        // Node supply/demand must be set before arcs, because set_arc adjusts
        // excess for nonzero lower bounds (excess -= low for tail, excess += low
        // for head). Setting nodes after arcs would overwrite those adjustments.
        for node in &problem.node_descs {
            let id =
                usize::try_from(node.id).map_err(|_| Cs2Error::InvalidNodeId { id: node.id })?;
            solver.set_supply_demand_of_node(id, node.supply)?;
        }
        for arc in &problem.arcs {
            solver.set_arc(
                usize::try_from(arc.from).map_err(|_| Cs2Error::InvalidNodeId { id: arc.from })?,
                usize::try_from(arc.to).map_err(|_| Cs2Error::InvalidNodeId { id: arc.to })?,
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
    /// Create a solver with node IDs `1..=num_nodes` and exactly `num_arcs` arcs.
    ///
    /// # Panics
    /// Panics if the dimensions exceed addressable array sizes.
    #[must_use]
    pub fn new(num_nodes: usize, num_arcs: usize) -> Self {
        assert!(
            valid_dimensions(num_nodes, num_arcs),
            "unaddressable graph dimensions"
        );
        let mut solver = McmfCs2 {
            n: num_nodes,
            m: num_arcs,
            started: false,

            check_solution: false,
            comp_duals: false,

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

    /// Sets `check_solution` to `value`.
    #[must_use]
    pub fn check_solution(mut self, value: bool) -> Self {
        self.check_solution = value;
        self
    }
    /// Sets `comp_duals` to `value`.
    #[must_use]
    pub fn comp_duals(mut self, value: bool) -> Self {
        self.comp_duals = value;
        self
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
    unsafe fn increase_flow(
        &mut self,
        i: *mut Node,
        j: *mut Node,
        a: *mut Arc,
        df: i64,
    ) -> Result<(), Cs2Error> {
        // SAFETY: upheld by this function's safety contract: `i`, `j`, `a`,
        // and `(*a).sister` all point into the live node/arc arenas.
        unsafe {
            // A self-loop changes residual capacities, but not node excess.
            if i != j {
                let tail = (*i)
                    .excess
                    .checked_sub(df)
                    .ok_or(Cs2Error::ExcessOverflow)?;
                let head = (*j)
                    .excess
                    .checked_add(df)
                    .ok_or(Cs2Error::ExcessOverflow)?;
                (*i).excess = tail;
                (*j).excess = head;
            }
            (*a).res_capacity -= df;
            (*(*a).sister).res_capacity += df;
            self.n_push += 1;
            Ok(())
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
        // SAFETY: upheld by this function's safety contract: `excq_first`
        // forms a valid linked list of node pointers terminated by null.
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
        // SAFETY: upheld by this function's safety contract.
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
        // SAFETY: upheld by this function's safety contract; `excq_last` is
        // valid whenever `nonempty_excess_q()` returns true.
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
        // SAFETY: upheld by this function's safety contract: a non-empty
        // queue guarantees `excq_first` is a valid node pointer.
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
        // SAFETY: upheld by this function's safety contract.
        unsafe { self.reset_excess_q() };
    }

    /// Push node `i` onto the stack-queue.
    ///
    /// # Safety
    /// Caller must pass a valid node pointer.
    #[inline(always)]
    unsafe fn stackq_push(&mut self, i: *mut Node) {
        // SAFETY: upheld by this function's safety contract.
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
        // SAFETY: upheld by this function's safety contract.
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
        // SAFETY: upheld by this function's safety contract: `buckets_base`
        // points to a live buckets arena of length > `b`.
        unsafe { (*self.buckets_base.add(b)).p_first = self.dnode };
    }

    /// Returns true if bucket `b` is non-empty.
    ///
    /// # Safety
    /// Requires base pointers to be set.
    #[inline(always)]
    unsafe fn nonempty_bucket(&self, b: BucketIndex) -> bool {
        // SAFETY: upheld by this function's safety contract.
        unsafe { (*self.buckets_base.add(b)).p_first != self.dnode }
    }

    /// Insert node `i` into bucket `b`.
    ///
    /// # Safety
    /// Caller must pass a valid node pointer and bucket index.
    #[inline(always)]
    unsafe fn insert_to_bucket(&mut self, i: *mut Node, b: BucketIndex) {
        // SAFETY: upheld by this function's safety contract; `old_first` is
        // either a valid node pointer or the `dnode` sentinel.
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
        // SAFETY: upheld by this function's safety contract: a non-empty
        // bucket guarantees `(*bucket).p_first` is a valid node pointer.
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
        // SAFETY: upheld by this function's safety contract; `b_prev` /
        // `b_next` form a valid doubly linked list inside the bucket.
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
        // The declared node domain includes isolated nodes.
        self.node_max = self.n;
        self.node_min = 1;
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
    /// is rewritten to `MAX_32`). Also rejects excess arcs, solving already
    /// started, and unrepresentable reverse costs or lower-bound adjustments.
    pub fn set_arc(
        &mut self,
        tail_node_id: usize,
        head_node_id: usize,
        low_bound: i64,
        mut up_bound: i64,
        cost: Price,
    ) -> Result<(), Cs2Error> {
        if self.started {
            return Err(Cs2Error::InvalidBuildState);
        }
        if self.arc_current / 2 == self.m {
            return Err(Cs2Error::ArcCountMismatch {
                expected: self.m,
                actual: self.m + 1,
            });
        }
        if tail_node_id == 0 || head_node_id == 0 || tail_node_id > self.n || head_node_id > self.n
        {
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

        // Validate all fallible arithmetic before modifying the builder.
        let reverse_cost = cost.checked_neg().ok_or(Cs2Error::PriceOverflow)?;
        let abs_cost = cost.checked_abs().ok_or(Cs2Error::PriceOverflow)?;
        let mut tail_excess = self.nodes[tail_node_id].excess;
        let mut head_excess = self.nodes[head_node_id].excess;
        if tail_node_id != head_node_id {
            tail_excess = tail_excess
                .checked_sub(low_bound)
                .ok_or(Cs2Error::ExcessOverflow)?;
            head_excess = head_excess
                .checked_add(low_bound)
                .ok_or(Cs2Error::ExcessOverflow)?;
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
        self.arcs[ac + 1].cost = reverse_cost;

        self.nodes[tail_node_id].excess = tail_excess;
        self.nodes[head_node_id].excess = head_excess;
        // Zero-capacity arcs are scaled too.
        self.max_cost = self.max_cost.max(abs_cost);

        self.arc_current += 2;
        self.pos_current += 2;
        Ok(())
    }

    /// Replace the supply (positive) or demand (negative) of a node.
    /// Must be called before the first [`set_arc`](Self::set_arc).
    ///
    /// # Errors
    ///
    /// Returns [`Cs2Error::NodeIdOutOfBounds`] if `id` is outside `1..=n`,
    /// [`Cs2Error::InvalidBuildState`] after arcs or solving have started, or
    /// [`Cs2Error::ExcessOverflow`] if the new aggregate totals exceed `i64`.
    pub fn set_supply_demand_of_node(&mut self, id: usize, excess: Excess) -> Result<(), Cs2Error> {
        if self.started || self.arc_current != 0 {
            return Err(Cs2Error::InvalidBuildState);
        }
        if id == 0 || id > self.n {
            return Err(Cs2Error::NodeIdOutOfBounds { id, max: self.n });
        }
        let old = self.nodes[id].excess;
        let total_p = i128::from(self.total_p) - i128::from(old.max(0)) + i128::from(excess.max(0));
        let total_n = i128::from(self.total_n) + i128::from(old.min(0)) - i128::from(excess.min(0));
        let total_p = i64::try_from(total_p).map_err(|_| Cs2Error::ExcessOverflow)?;
        let total_n = i64::try_from(total_n).map_err(|_| Cs2Error::ExcessOverflow)?;
        self.nodes[id].excess = excess;
        self.total_p = total_p;
        self.total_n = total_n;
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
    /// - [`Cs2Error::ArcCountMismatch`] if the builder is incomplete.
    /// - [`Cs2Error::InvalidBuildState`] if solving has already started.
    fn pre_processing(&mut self) -> Result<(), Cs2Error> {
        if self.started {
            return Err(Cs2Error::InvalidBuildState);
        }
        if self.arc_current / 2 != self.m {
            return Err(Cs2Error::ArcCountMismatch {
                expected: self.m,
                actual: self.arc_current / 2,
            });
        }
        if self.total_p != self.total_n {
            return Err(Cs2Error::Unbalanced {
                supply: self.total_p,
                demand: self.total_n,
            });
        }
        self.started = true;

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

                    // SAFETY: arcs_base is set in allocate_arrays, and
                    // arc_num / arc_new_num are valid indexes into the
                    // allocated arc storage tracked by arcs_base here.
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

        // adjustments: shift node base.
        // Vec::drain(0..node_min) shifts the remaining elements forward
        // in-place. The buffer base pointer (self.nodes_base) is unchanged,
        // but every head pointer stored in arcs is now off by `node_min`
        // node-slots — point them back to the correct (shifted) node.
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
    fn cs2_initialize(&mut self) -> Result<(), Cs2Error> {
        self.f_scale = SCALE_DEFAULT;
        self.dn = i64::try_from(self.n + 1).map_err(|_| Cs2Error::PriceOverflow)?;
        if self.no_zero_cycles {
            self.dn = self.dn.checked_mul(2).ok_or(Cs2Error::PriceOverflow)?;
        }
        // Validate scaling before changing costs or saturating any arcs. max_cost
        // includes every arc, and reverse costs have equal absolute magnitude.
        self.mmc = self
            .max_cost
            .checked_mul(self.dn)
            .ok_or(Cs2Error::PriceOverflow)?;
        self.linf = usize::try_from(self.dn)
            .ok()
            .and_then(|dn| dn.checked_mul(SCALE_DEFAULT as usize))
            .and_then(|buckets| buckets.checked_add(2))
            .filter(|&buckets| buckets <= isize::MAX as usize / size_of::<Bucket>())
            .ok_or(Cs2Error::InvalidProblemSize)?;
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
                            self.increase_flow(i_ptr, j_ptr, a, df)?;
                        }
                    }
                    a = a.add(1);
                }
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
        Ok(())
    }

    /// Scans node `i` during a price update, propagating distance labels
    /// to neighboring nodes via reverse residual arcs.
    ///
    /// For each neighbor `j` reachable through a reverse arc with positive
    /// residual capacity, computes a candidate rank from the reduced cost
    /// `rc = p_j + c_ji - p_i`. If `rc < 0` the arc is admissible and `j`
    /// inherits `i`'s rank; otherwise the rank increases by `floor(rc / epsilon) + 1`.
    /// When a neighbor's rank improves, it is moved to a closer bucket in the
    /// Dijkstra-like scan order used by [`price_update`](Self::price_update).
    ///
    /// After processing all neighbors, node `i`'s price is decreased by
    /// `rank * epsilon` and its rank is set to −1 (settled).
    fn up_node_scan(&mut self, i_ptr: *mut Node) -> Result<(), Cs2Error> {
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
                        let rc = reduced_cost((*j_ptr).price, (*ra).cost, i_price)?;
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

            let dp = i_rank.checked_mul(eps).ok_or(Cs2Error::PriceOverflow)?;
            (*i_ptr).price = (*i_ptr)
                .price
                .checked_sub(dp)
                .ok_or(Cs2Error::PriceOverflow)?;
            (*i_ptr).rank = -1;
        }
        Ok(())
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
    fn price_update(&mut self) -> Result<(), Cs2Error> {
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
            if remain == 0 {
                return Ok(());
            }

            let mut b = 0usize;
            while b < self.l_bucket {
                while self.nonempty_bucket(b) {
                    let i_ptr = self.get_from_bucket(b);
                    self.up_node_scan(i_ptr)?;
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

            if remain > 0 {
                self.flag_updt = UpdateFlag::Failed;
            }

            let dp = (b as i64)
                .checked_mul(self.epsilon)
                .ok_or(Cs2Error::PriceOverflow)?;
            let price_min = self.price_min;

            let mut p = self.nodes_base;
            while p < sentinel_node {
                let rank = (*p).rank;
                if rank >= 0 {
                    if rank < linf_i {
                        self.remove_from_bucket(p, rank as usize);
                    }
                    if (*p).price > price_min {
                        (*p).price = (*p).price.checked_sub(dp).ok_or(Cs2Error::PriceOverflow)?;
                    }
                }
                p = p.add(1);
            }
        }
        Ok(())
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
                // Relabeling cannot change a self-loop's reduced cost. Negative
                // self-loops were already saturated during initialization.
                if (*a).res_capacity > 0 && (*a).head != i_ptr {
                    let head = (*a).head;
                    let dp = (*head)
                        .price
                        .checked_sub((*a).cost)
                        .ok_or(Cs2Error::PriceOverflow)?;
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
            // For an empty adjacency list current == a_stop. Do not inspect
            // the next node's arc (or the trailing sentinel) as an outgoing arc.
            let a_stop2 = current.add(1).min(a_stop);
            let mut a = a_start2;
            while a < a_stop2 {
                if (*a).res_capacity > 0 && (*a).head != i_ptr {
                    let head = (*a).head;
                    let dp = (*head)
                        .price
                        .checked_sub((*a).cost)
                        .ok_or(Cs2Error::PriceOverflow)?;
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
                (*i_ptr).price = p_max
                    .checked_sub(self.epsilon)
                    .ok_or(Cs2Error::PriceOverflow)?;
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
    /// When a push turns a deficit node into a positive-excess node, relabels
    /// that destination before enqueuing it. Other destinations are enqueued
    /// without an outgoing-arc lookahead.
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
            let is_admissible = a < (*i_ptr.add(1)).suspended
                && (*a).res_capacity > 0
                && reduced_cost((*i_ptr).price, (*a).cost, (*j_ptr).price)? < 0;
            if !is_admissible {
                self.relabel(i_ptr)?;
                if self.flag_price != 0 {
                    return Ok(());
                }
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
                    self.increase_flow(i_ptr, j_ptr, a, df)?;
                    if self.out_of_excess_q(j_ptr) {
                        self.insert_to_excess_q(j_ptr);
                    }
                } else {
                    let df = (*i_ptr).excess.min((*a).res_capacity);
                    self.increase_flow(i_ptr, j_ptr, a, df)?;
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
                if self.flag_price != 0 {
                    break;
                }
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
    fn price_in(&mut self) -> Result<i32, Cs2Error> {
        // SAFETY: called only after cs2_initialize set base pointers.
        unsafe {
            let arcs_base = self.arcs_base;
            let mut bad_found = 0;
            let mut n_in_bad = 0;
            let sentinel_node = self.sentinel_node;

            'restart: loop {
                let cut_on = self.cut_on;
                let mut i_ptr = self.nodes_base;
                while i_ptr < sentinel_node {
                    let initial_first = (*i_ptr).first;
                    let suspended = (*i_ptr).suspended;
                    let i_price = (*i_ptr).price;

                    let mut a = initial_first;
                    while a > suspended {
                        a = a.sub(1);
                        let j_ptr = (*a).head;
                        let rc = reduced_cost(i_price, (*a).cost, (*j_ptr).price)?;
                        if rc < 0 && (*a).res_capacity > 0 {
                            if bad_found == 0 {
                                bad_found = 1;
                                self.update_cut_off();
                                continue 'restart;
                            }
                            let df = (*a).res_capacity;
                            self.increase_flow(i_ptr, j_ptr, a, df)?;

                            let reverse_arc = (*a).sister;

                            (*i_ptr).first = (*i_ptr).first.sub(1);
                            let b_idx = (*i_ptr).first.offset_from(arcs_base) as usize;
                            let a_idx = a.offset_from(arcs_base) as usize;
                            self.exchange(a_idx, b_idx);

                            if reverse_arc < (*j_ptr).first {
                                (*j_ptr).first = (*j_ptr).first.sub(1);
                                let reverse_first_idx =
                                    (*j_ptr).first.offset_from(arcs_base) as usize;
                                let reverse_arc_idx = reverse_arc.offset_from(arcs_base) as usize;
                                self.exchange(reverse_arc_idx, reverse_first_idx);
                            }

                            n_in_bad += 1;
                        } else if (rc as f64) < cut_on && (rc as f64) > -cut_on {
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
                        self.total_excess = self
                            .total_excess
                            .checked_add(i_exc)
                            .ok_or(Cs2Error::ExcessOverflow)?;
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

            Ok(n_in_bad)
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
                    self.total_excess = self
                        .total_excess
                        .checked_add(i_exc)
                        .ok_or(Cs2Error::ExcessOverflow)?;
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
                        self.price_in()?;
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
                            self.price_in()?;
                            self.flag_price = 0;
                        }

                        self.price_update()?;

                        while self.flag_updt != UpdateFlag::Ok {
                            if self.n_ref == 1 {
                                return Err(Cs2Error::Infeasible);
                            }
                            self.flag_updt = UpdateFlag::Ok;
                            self.update_cut_off();
                            self.n_bad_relabel += 1;
                            pr_in_int = 0;
                            self.price_in()?;
                            self.price_update()?;
                        }
                        self.n_rel = 0;

                        if self.n_ref > PRICE_OUT_START
                            && price_in_due(&mut pr_in_int, self.time_for_price_in)
                        {
                            self.price_in()?;
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
    fn price_refine(&mut self) -> Result<bool, Cs2Error> {
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
                                let rc = reduced_cost(i_price, (*a).cost, (*j_ptr).price)?;
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
                                        let mut df: i64 = i64::MAX;
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
                                            self.increase_flow(ir, head, ar, df)?;
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
                            let rc = reduced_cost(i_price, (*a).cost, (*j_ptr).price)?;
                            if rc < 0 {
                                // Exact floor((|rc| - 1) / eps), including rc == i64::MIN.
                                let dr = ((rc.unsigned_abs() - 1) / eps as u64) as i64;
                                let j_rank = dr.saturating_add(i_rank);
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
                    let dp = i_rank.checked_mul(eps).ok_or(Cs2Error::PriceOverflow)?;

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
                                    let rc = reduced_cost(i_price, (*a).cost, (*j_ptr).price)?;
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
                                            self.increase_flow(i_ptr, j_ptr, a, df)?;
                                        }
                                    }
                                }
                            }
                            a = a.add(1);
                        }

                        (*i_ptr).price = (*i_ptr)
                            .price
                            .checked_sub(dp)
                            .ok_or(Cs2Error::PriceOverflow)?;
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
                        let rc = reduced_cost(i_price, (*a).cost, (*j_ptr).price)?;
                        if rc < -eps {
                            let df = (*a).res_capacity;
                            if df > 0 {
                                self.increase_flow(p, j_ptr, a, df)?;
                            }
                        }
                        a = a.add(1);
                    }
                    p = p.add(1);
                }
            }

            Ok(eps_optimal)
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
    fn compute_prices(&mut self) -> Result<(), Cs2Error> {
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
                                let rc = reduced_cost(i_price, (*a).cost, (*j_ptr).price)?;
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
                            let rc = reduced_cost(i_price, (*a).cost, (*j_ptr).price)?;
                            if rc < 0 {
                                let j_rank = rc.unsigned_abs().saturating_add(i_rank as u64);
                                if j_rank < linf_i as u64 && j_rank > (*j_ptr).rank as u64 {
                                    (*j_ptr).rank = j_rank as i64;
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
                                    let rc = reduced_cost(i_price, (*a).cost, (*j_ptr).price)?;
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

                        (*i_ptr).price = (*i_ptr)
                            .price
                            .checked_sub(dp)
                            .ok_or(Cs2Error::PriceOverflow)?;
                    }
                    b -= 1;
                }

                if !cycle_free {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Suspends arcs whose reduced cost exceeds the `cut_off` threshold
    /// (Goldberg §2.3: *speculative arc fixing*).
    ///
    /// An arc is suspended if its reduced cost is large enough that the
    /// push-relabel method will not change its flow before epsilon decreases
    /// further. Suspended arcs are moved before `first` in the adjacency list
    /// via [`exchange`](Self::exchange), so they are skipped by relabel and
    /// discharge. They can later be recovered by [`price_in`](Self::price_in).
    fn price_out(&mut self) -> Result<(), Cs2Error> {
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
                    let rc = reduced_cost(i_price, (*a).cost, (*j_ptr).price)? as f64;
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
        Ok(())
    }

    /// Reduce epsilon by the scale factor for the next scaling iteration.
    /// Returns `true` if epsilon has reached 1 (scaling is complete),
    /// and `false` if epsilon was successfully reduced.
    fn update_epsilon(&mut self) -> bool {
        // decrease epsilon after epsilon-optimal flow is constructed
        if self.epsilon <= 1 {
            return true;
        }
        // f_scale is the fixed integer SCALE_DEFAULT. Avoid losing integer
        // precision above 2^53 when taking the ceiling.
        let scale = SCALE_DEFAULT as i64;
        self.epsilon = self.epsilon / scale + i64::from(self.epsilon % scale != 0);
        self.cut_off = self.cut_off_factor * self.epsilon as f64;
        self.cut_on = self.cut_off * CUT_OFF_GAP;
        false
    }

    /// Check pre-initialization transformed balances against flow above each lower bound.
    ///
    /// - `initial_balances`: Per-node supply/demand balance.
    fn is_feasible(&self, initial_balances: &[Excess]) -> bool {
        debug_assert_eq!(initial_balances.len(), self.n);
        // Wider scratch sums avoid overflow due only to the order of checking
        // incident arcs.
        // We do not mutate the saved balances so that checking is repeatable.
        // Current node excesses are not a substitute: solving has changed them.
        let mut balance: Vec<i128> = initial_balances.iter().copied().map(i128::from).collect();
        let arcs_base = self.arcs_base;
        let nodes_base = self.nodes_base;
        for i in 0..self.n {
            // SAFETY: cs2_initialize stored `node.suspended` as a pointer
            // into the same allocation as `arcs_base`.
            let a_start = unsafe { self.nodes[i].suspended.offset_from(arcs_base) as usize };
            // SAFETY: same allocation invariant as `a_start`; the trailing
            // sentinel at `self.nodes[self.n]` keeps `i + 1` in-bounds.
            let a_stop = unsafe { self.nodes[i + 1].suspended.offset_from(arcs_base) as usize };
            for a in a_start..a_stop {
                if self.cap[a] > 0 {
                    let full_flow = self.cap[a] - self.arcs[a].res_capacity;
                    // SAFETY: complete construction and arc exchanges preserve
                    // sister pointers into the live arc arena.
                    let above_lower = unsafe { (*self.arcs[a].sister).res_capacity };
                    if full_flow < 0
                        || full_flow > self.cap[a]
                        || above_lower < 0
                        || above_lower > full_flow
                    {
                        return false;
                    }
                    balance[i] -= i128::from(above_lower);
                    // SAFETY: cs2_initialize stored `arc.head` as a pointer
                    // into the same allocation as `nodes_base`.
                    let head_idx = unsafe { self.arcs[a].head.offset_from(nodes_base) as usize };
                    balance[head_idx] += i128::from(above_lower);
                }
            }
        }
        balance.iter().all(|&excess| excess == 0)
    }

    /// Checks complimentary slackness.
    ///
    /// If true, then the problem is possibly feasible,
    /// otherwise the problem is unfeasible.
    fn check_cs(&self) -> bool {
        let arcs_base = self.arcs_base;
        let nodes_base = self.nodes_base;
        for i in 0..self.n {
            // SAFETY: cs2_initialize stored `node.suspended` as a pointer
            // into the same allocation as `arcs_base`.
            let a_start = unsafe { self.nodes[i].suspended.offset_from(arcs_base) as usize };
            // SAFETY: same as `a_start`; the trailing sentinel at
            // `self.nodes[self.n]` keeps `i + 1` in-bounds.
            let a_stop = unsafe { self.nodes[i + 1].suspended.offset_from(arcs_base) as usize };
            for a in a_start..a_stop {
                if self.arcs[a].res_capacity > 0 {
                    // SAFETY: cs2_initialize stored `arc.head` as a pointer
                    // into the same allocation as `nodes_base`.
                    let j = unsafe { self.arcs[a].head.offset_from(nodes_base) as usize };
                    let rc = i128::from(self.nodes[i].price) + i128::from(self.arcs[a].cost)
                        - i128::from(self.nodes[j].price);
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
    /// `comp_duals`: whether to compute the prices.
    fn print_solution(&self, objective_cost: f64, comp_duals: bool) {
        if !self.print_ans {
            return;
        }
        println!("c");
        println!("s {objective_cost:.0}");

        let arcs_base = self.arcs_base;
        let nodes_base = self.nodes_base;
        for i in 0..self.n {
            let ni = n_node(i, self.node_min);
            // SAFETY: cs2_initialize stored `node.suspended` as a pointer
            // into the same allocation as `arcs_base`.
            let a_start = unsafe { self.nodes[i].suspended.offset_from(arcs_base) as usize };
            // SAFETY: same as `a_start`; the trailing sentinel at
            // `self.nodes[self.n]` keeps `i + 1` in-bounds.
            let a_stop = unsafe { self.nodes[i + 1].suspended.offset_from(arcs_base) as usize };
            for a in a_start..a_stop {
                if self.cap[a] > 0 {
                    // SAFETY: cs2_initialize stored `arc.head` as a pointer
                    // into the same allocation as `nodes_base`.
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
            let mut min_price = i64::MAX;
            for i in 0..self.n {
                min_price = min_price.min(self.nodes[i].price);
            }
            for i in 0..self.n {
                println!(
                    "p {:7} {:7}",
                    n_node(i, self.node_min),
                    i128::from(self.nodes[i].price) - i128::from(min_price)
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
            // SAFETY: cs2_initialize stored `node.suspended` as a pointer
            // into the same allocation as `arcs_base`.
            let a_start = unsafe { self.nodes[i].suspended.offset_from(arcs_base) as usize };
            // SAFETY: same as `a_start`; the trailing sentinel at
            // `self.nodes[self.n]` keeps `i + 1` in-bounds.
            let a_stop = unsafe { self.nodes[i + 1].suspended.offset_from(arcs_base) as usize };
            for a in a_start..a_stop {
                // SAFETY: cs2_initialize stored `arc.head` as a pointer
                // into the same allocation as `nodes_base`.
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
    fn finishup(&mut self, objective_cost: &mut f64, comp_duals: bool) -> Result<(), Cs2Error> {
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
            self.compute_prices()?;
        }

        *objective_cost = obj_internal;
        Ok(())
    }

    /// Main loop of the successive approximation algorithm
    /// (Goldberg §1, Fig 1: *Min-Cost*).
    ///
    /// The solver option is [`Self::comp_duals`]. `false` by default.
    ///
    /// Starting from `epsilon = max_cost * dn`, repeatedly:
    /// 1. Calls [`refine`](Self::refine) to convert the current pseudoflow
    ///    into an epsilon-optimal flow.
    /// 2. Calls [`price_out`](Self::price_out) to suspend arcs with large
    ///    reduced costs.
    /// 3. Reduces epsilon by the scale factor.
    /// 4. Attempts [`price_refine`](Self::price_refine) to skip full refine
    ///    iterations when prices alone can establish optimality at the new
    ///    epsilon. Falls back to refine if `price_refine` detects a cycle.
    ///
    /// Terminates when `epsilon < 1`, at which point the flow is optimal.
    #[inline(never)]
    fn cs2(&mut self, objective_cost: &mut f64) -> Result<(), Cs2Error> {
        let comp_duals = self.comp_duals;
        let mut scaling_done = false;

        self.update_epsilon();

        loop {
            self.refine()?;

            if self.n_ref >= PRICE_OUT_START {
                self.price_out()?;
            }

            if self.update_epsilon() {
                break;
            }

            loop {
                // need to refine further
                if !self.price_refine()? {
                    break;
                }

                if self.n_ref >= PRICE_OUT_START {
                    if self.price_in()? != 0 {
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

        self.finishup(objective_cost, comp_duals)?;
        Ok(())
    }

    /// Executes the cost-scaling minimum-cost maximum-flow algorithm, printing the solution.
    ///
    /// The solver options are set by [`Self::check_solution`] and [`Self::comp_duals`]. Both are `false` by default.
    ///
    /// # Errors
    /// Returns [`Cs2Error::Infeasible`] when the problem has no feasible
    /// circulation, or any other [`Cs2Error`] variant produced by the
    /// preprocessing / cost-scaling phases.
    /// This method may be called only once after successful preprocessing;
    /// subsequent calls return [`Cs2Error::InvalidBuildState`].
    pub fn run_cs2(&mut self) -> Result<(), Cs2Error> {
        // ordering
        self.pre_processing()?;

        let check_solution = self.check_solution;
        let comp_duals = self.comp_duals;

        // Save transformed supplies before initialization or pushes change excess.
        // The snapshot is local to this solve, and is not allocated when checking is off.
        let initial_balances = check_solution.then(|| {
            self.nodes[..self.n]
                .iter()
                .map(|node| node.excess)
                .collect::<Vec<_>>()
        });

        // double the arc count (forward + backward)
        self.m *= 2;
        self.cs2_initialize()?;
        self.print_graph();

        println!("\nc CS 4.3");
        println!("c nodes: {}  arcs: {}", self.n, self.m / 2);
        println!(
            "c scale-factor: {}  cut-off-factor: {}\nc",
            self.f_scale, self.cut_off_factor
        );

        let mut objective_cost: f64 = 0.0;
        self.cs2(&mut objective_cost)?;

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

        if let Some(initial_balances) = initial_balances {
            println!("c checking feasibility...");
            if self.is_feasible(&initial_balances) {
                println!("c ...OK");
            } else {
                println!("c ERROR: solution infeasible");
                return Err(Cs2Error::Infeasible);
            }
            println!("c computing prices and checking CS...");
            self.compute_prices()?;
            if self.check_cs() {
                println!("c ...OK");
            } else {
                println!("ERROR: CS violation");
                return Err(Cs2Error::Infeasible);
            }
        }

        if self.print_ans {
            self.print_solution(objective_cost, comp_duals);
        }
        Ok(())
    }

    /// Executes the cost-scaling minimum-cost maximum-flow algorithm, returning the solution
    /// as a [`McmfSolution`] object.
    ///
    /// The solver options are set by [`Self::check_solution`] and [`Self::comp_duals`]. Both are `false` by default.
    ///
    /// # Errors
    /// Returns [`Cs2Error::Infeasible`] when the problem has no feasible
    /// circulation, or any other [`Cs2Error`] variant produced by the
    /// preprocessing / cost-scaling phases.
    /// In particular, unsupported intermediate price/excess arithmetic returns
    /// [`Cs2Error::PriceOverflow`] / [`Cs2Error::ExcessOverflow`] in all profiles.
    pub fn min_cost(mut self) -> Result<McmfSolution, Cs2Error> {
        // ordering
        self.pre_processing()?;

        let check_solution = self.check_solution;

        // Save transformed supplies before initialization or pushes change excess.
        // The snapshot is local to this solve, and is not allocated when checking is off.
        let initial_balances = check_solution.then(|| {
            self.nodes[..self.n]
                .iter()
                .map(|node| node.excess)
                .collect::<Vec<_>>()
        });

        // double the arc count (forward + backward)
        self.m *= 2;
        self.cs2_initialize()?;

        let mut objective_cost = 0.0;
        self.cs2(&mut objective_cost)?;

        if let Some(initial_balances) = initial_balances {
            if !self.is_feasible(&initial_balances) {
                return Err(Cs2Error::Infeasible);
            }
            self.compute_prices()?;
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
    /// Optimal objective cost, represented approximately as `f64`.
    /// Integer objectives above `2^53` need not be exactly representable.
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
            // SAFETY: cs2_initialize stored `node.suspended` as a pointer
            // into the same allocation as `arcs_base`.
            let a_start = unsafe { s.nodes[i].suspended.offset_from(arcs_base) as usize };
            // SAFETY: same as `a_start`; the trailing sentinel at
            // `s.nodes[s.n]` keeps `i + 1` in-bounds.
            let a_stop = unsafe { s.nodes[i + 1].suspended.offset_from(arcs_base) as usize };
            (a_start..a_stop).filter_map(move |a| {
                if s.cap[a] > 0 {
                    let flow = s.cap[a] - s.arcs[a].res_capacity;
                    let tail = n_node(i, s.node_min) as usize;
                    // SAFETY: cs2_initialize stored `arc.head` as a pointer
                    // into the same allocation as `nodes_base`.
                    let head_idx = unsafe { s.arcs[a].head.offset_from(nodes_base) as usize };
                    let head = n_node(head_idx, s.node_min) as usize;
                    Some((tail, head, flow))
                } else {
                    None
                }
            })
        })
    }

    /// Iterate over node prices yielding (`node_id`, price).
    /// Only meaningful if `comp_duals` was enabled.
    pub fn prices(&self) -> impl Iterator<Item = (usize, Price)> {
        let s = &self.solver;
        (0..s.n).map(move |i| (n_node(i, s.node_min) as usize, s.nodes[i].price))
    }

    /// Returns statistics of the solution.
    #[must_use]
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

#[cfg(test)]
mod tests {
    use super::{CUT_OFF_GAP, CUT_OFF_MIN, Cs2Error, McmfCs2, price_in_due, reduced_cost};

    fn initialize(solver: &mut McmfCs2) {
        solver.pre_processing().expect("preprocessing");
        solver.m *= 2;
        solver.cs2_initialize().expect("initialization");
    }

    #[test]
    fn price_in_preserves_fractional_and_integer_threshold_boundaries() {
        for cut_on in [9.6, 10.0] {
            for rc in [-10, -9, 0, 9, 10] {
                let mut solver = McmfCs2::new(2, 1);
                solver.set_arc(1, 2, 0, 1, 0).expect("arc");
                initialize(&mut solver);
                // Saturate the forward arc so a negative rc does not invoke
                // mandatory bad-fix-in handling instead of the threshold test.
                solver.arcs[0].res_capacity = 0;
                solver.arcs[1].res_capacity = 1;
                solver.nodes[0].first = solver.nodes[1].suspended;
                solver.nodes[0].price = rc;
                solver.cut_on = cut_on;
                assert_eq!(solver.price_in(), Ok(0));
                let active = solver.nodes[0].first == solver.nodes[0].suspended;
                assert_eq!(
                    active,
                    (rc as f64).abs() < cut_on,
                    "rc={rc}, cut_on={cut_on}"
                );
            }
        }
    }

    #[test]
    fn price_in_schedule_matches_post_increment() {
        for interval in [2, 4, 6] {
            let mut counter = 0;
            for _ in 0..2 {
                for _ in 0..=interval {
                    assert!(!price_in_due(&mut counter, interval));
                }
                assert!(price_in_due(&mut counter, interval));
                assert_eq!(counter, 0);
            }
        }
    }

    #[test]
    fn price_refine_rank_is_exact_at_two_to_the_53() {
        let mut solver = McmfCs2::new(2, 1);
        solver.set_arc(1, 2, 0, 1, 0).expect("arc");
        initialize(&mut solver);
        solver.epsilon = 1_i64 << 53;
        solver.nodes[0].price = -solver.epsilon;
        assert_eq!(solver.price_refine(), Ok(true));
        // |rc| == epsilon needs rank floor((epsilon - 1) / epsilon) == 0.
        assert_eq!(solver.nodes[1].price, 0);
    }

    #[test]
    fn epsilon_ceiling_is_exact_above_f64_integer_precision() {
        let mut solver = McmfCs2::new(0, 0);
        for epsilon in ((1_i64 << 53) - 20)..=((1_i64 << 53) + 20) {
            solver.epsilon = epsilon;
            assert!(!solver.update_epsilon());
            assert_eq!(i128::from(solver.epsilon), (i128::from(epsilon) + 11) / 12);
        }
    }

    #[test]
    fn cycle_cancellation_uses_the_full_i64_capacity_range() {
        let capacity = i64::from(i32::MAX) + 100;
        let mut solver = McmfCs2::new(2, 2);
        solver.set_arc(1, 2, 0, capacity, 0).expect("arc");
        solver.set_arc(2, 1, 0, capacity + 10, 0).expect("arc");
        initialize(&mut solver);
        // Build a residual negative cycle after initialization, to exercise
        // price_refine's bottleneck computation rather than initial saturation.
        for (a, &cap) in solver.arcs.iter_mut().zip(&solver.cap) {
            a.cost = if cap > 0 { -1 } else { 1 };
        }
        solver.epsilon = 1;
        assert_eq!(solver.price_refine(), Ok(false));
        for (a, &cap) in solver.arcs.iter().zip(&solver.cap) {
            if cap > 0 {
                assert_eq!(a.res_capacity, cap - capacity);
            }
        }
    }

    #[test]
    fn reduced_cost_and_later_price_updates_report_overflow() {
        assert_eq!(reduced_cost(i64::MAX, 1, 0), Err(Cs2Error::PriceOverflow));
        assert_eq!(reduced_cost(i64::MIN, 0, 1), Err(Cs2Error::PriceOverflow));
        assert_eq!(reduced_cost(-10, 3, -8), Ok(1));
        let mut solver = McmfCs2::new(1, 0);
        initialize(&mut solver);
        solver.nodes[0].rank = 2;
        solver.epsilon = i64::MAX;
        assert_eq!(
            solver.up_node_scan(solver.nodes_base),
            Err(Cs2Error::PriceOverflow)
        );
        solver.nodes[0].rank = 1;
        solver.nodes[0].price = i64::MIN;
        solver.epsilon = 1;
        assert_eq!(
            solver.up_node_scan(solver.nodes_base),
            Err(Cs2Error::PriceOverflow)
        );
    }

    #[test]
    fn feasibility_check_is_repeatable_with_lower_bounds() {
        let input = "p min 2 1\nn 1 10\nn 2 -10\na 1 2 3 10 2\n";
        let initial_balances = [7, -7]; // Supply adjusted by the lower bound of 3.
        let solution = McmfCs2::from_dimacs(input)
            .expect("input")
            .check_solution(true)
            .min_cost()
            .expect("solution");
        assert!(solution.solver.is_feasible(&initial_balances));
        assert!(solution.solver.is_feasible(&initial_balances));
        assert_eq!(initial_balances, [7, -7]);
    }

    #[test]
    fn feasibility_check_needs_pre_solve_balances_not_final_excess() {
        let input = "p min 2 1\nn 1 1\nn 2 -1\na 1 2 0 1 7\n";
        let solution = McmfCs2::from_dimacs(input)
            .expect("input")
            .check_solution(true)
            .min_cost()
            .expect("solution");
        let solver = &solution.solver;
        let final_excess: Vec<_> = solver.nodes[..solver.n]
            .iter()
            .map(|node| node.excess)
            .collect();
        assert_eq!(final_excess, [0, 0]);
        assert!(solver.is_feasible(&[1, -1]));
        assert!(!solver.is_feasible(&final_excess));
    }

    #[test]
    fn price_in_restart_uses_widened_cut_on() {
        let mut solver = McmfCs2::new(3, 2);
        solver.set_arc(1, 2, 0, 1, 1).expect("first arc");
        solver.set_arc(1, 3, 0, 1, 5).expect("second arc");
        solver.pre_processing().expect("preprocessing");
        solver.m *= 2;
        solver.cs2_initialize().expect("initialization");

        // Costs are now 4 and 20. Suspend both arcs from node 1, and
        // make the first arc admissible so price_in must restart.
        solver.nodes[0].first = solver.nodes[1].suspended;
        solver.nodes[1].price = 5;
        solver.epsilon = 1;
        solver.cut_off_factor = CUT_OFF_MIN;
        solver.cut_off = CUT_OFF_MIN;
        solver.cut_on = CUT_OFF_MIN * CUT_OFF_GAP;
        solver.n_bad_pricein = 1;

        assert_eq!(solver.price_in().expect("price-in"), 1);
        // The cost-20 arc is outside the old threshold (9.6), but inside
        // the widened threshold (38.4), so both arcs must now be active.
        assert_eq!(solver.nodes[0].first, solver.nodes[0].suspended);
    }
}

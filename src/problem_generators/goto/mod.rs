//! GOTO (Grid On Torus) generator
//!
//! Produces a capacitated transportation problem in DIMACS `.min` format,
//! laid out on a grid-on-torus. Originally by Andrew V. Goldberg (1991),
//! Stanford University.
//!
//! The main functions for usage are:
//! - [`generate`]
//! - [`generate_to_string`]
//! - [`generate_to_file`]
//!
//! # Inputs
//!
//! | # | Description                    | Restriction            |
//! |---|--------------------------------|------------------------|
//! | 1 | number of nodes, N             | N >= 15                |
//! | 2 | number of arcs, M              | 6*N <= M <= N^(5/3)    |
//! | 3 | max capacity, MAXCAP           | MAXCAP >= 8            |
//! | 4 | max arc cost, MAXCOST          | MAXCOST >= 8           |
//! | 5 | seed (for random number gen)   |                        |

use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter};
use std::path::Path;

mod diagram;
pub use diagram::write_typst;

const B: i64 = 13_415_821;
const MOD: i64 = 100_000_000;
const MOD1: i64 = 10_000;
const MAX_Y_COST: i64 = 8;

/// Direction of an arc on the torus grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    X,
    Y,
}

/// Errors that can occur during GOTO generation.
#[derive(Debug)]
pub enum GotoError {
    /// An input parameter violated its constraints.
    InvalidParams(String),
    /// A formatting/write error occurred during output.
    Fmt(fmt::Error),
    /// An I/O error occurred when writing to a file or stdout.
    Io(io::Error),
}

impl fmt::Display for GotoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GotoError::InvalidParams(msg) => write!(f, "invalid parameters: {msg}"),
            GotoError::Fmt(e) => write!(f, "format error: {e}"),
            GotoError::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for GotoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GotoError::Fmt(e) => Some(e),
            GotoError::Io(e) => Some(e),
            GotoError::InvalidParams(_) => None,
        }
    }
}

impl From<fmt::Error> for GotoError {
    fn from(e: fmt::Error) -> Self {
        GotoError::Fmt(e)
    }
}

impl From<io::Error> for GotoError {
    fn from(e: io::Error) -> Self {
        GotoError::Io(e)
    }
}

/// Input parameters for the GOTO generator.
#[derive(Debug, Clone)]
pub struct GotoParams {
    /// Number of nodes (>= 15).
    pub n: i64,
    /// Number of arcs (6*N <= M <= N^(5/3)).
    pub m: i64,
    /// Maximum arc capacity (>= 8).
    pub max_cap: i64,
    /// Maximum arc cost (>= 8).
    pub max_cost: i64,
    /// Seed for the random number generator.
    pub seed: i64,
}

/// Validates the input parameters.
fn validate(params: &GotoParams) -> Result<(), GotoError> {
    if params.n < 15 {
        return Err(GotoError::InvalidParams("(# nodes) < 15".into()));
    }
    if params.m < 6 * params.n {
        return Err(GotoError::InvalidParams("(# arcs) < 6*(# nodes)".into()));
    }
    if params.m > (params.n as f64).powf(5.0 / 3.0) as i64 {
        return Err(GotoError::InvalidParams(
            "(# arcs) > (# nodes)^(5/3)".into(),
        ));
    }
    if params.max_cap < 8 {
        return Err(GotoError::InvalidParams("MAXCAP < 8".into()));
    }
    if params.max_cost < 8 {
        return Err(GotoError::InvalidParams("MAXCOST < 8".into()));
    }
    Ok(())
}

/// Internal state of the GOTO generator.
struct GotoGenerator {
    x: i64,
    y: i64,
    xdeg: i64,
    ydeg: i64,
    seed: i64,
    max_cap: i64,
    small_cap: i64,
    ret_cost: i64,
    max_cost: i64,
    max_deg: i64,
    n: i64,
    m: i64,
    extra_n: i64,
    extra_m: i64,
    cost: Vec<i64>,
    capacity: Vec<i64>,
    alpha: f64,
    s: i64,
    t: i64,
    supply: i64,
}

impl GotoGenerator {
    /// Creates a new generator from validated parameters.
    fn new(params: &GotoParams) -> Self {
        Self {
            x: 0,
            y: 0,
            xdeg: 0,
            ydeg: 0,
            seed: params.seed,
            max_cap: params.max_cap,
            small_cap: 0,
            ret_cost: 0,
            max_cost: params.max_cost,
            max_deg: 0,
            n: params.n,
            m: params.m,
            extra_n: 0,
            extra_m: 0,
            cost: Vec::new(),
            capacity: Vec::new(),
            alpha: 0.0,
            s: 0,
            t: 0,
            supply: 0,
        }
    }

    /// Sedgewick-style random number generator multiplication helper.
    fn mult(p: i64, q: i64) -> i64 {
        let p1 = p / MOD1;
        let p0 = p % MOD1;
        let q1 = q / MOD1;
        let q0 = q % MOD1;
        (((p0 * q1 + p1 * q0) % MOD1) * MOD1 + p0 * q0) % MOD
    }

    /// Returns a random integer in `[a, b)`.
    fn random_int(&mut self, a: i64, b: i64) -> i64 {
        let r = b - a;
        self.seed = (Self::mult(self.seed, B) + 1) % MOD;
        a + ((self.seed as f64 * r as f64) / MOD as f64) as i64
    }

    /// Returns a random bit (0 or 1).
    fn random_bit(&mut self) -> bool {
        self.random_int(0, 1) != 0
    }

    /// Returns a random capacity conditioned by distance from source.
    fn random_capacity(&mut self, dist: i64) -> i64 {
        let ans = self.random_int(1, self.max_cap) as f64;
        let scaled = ans / self.alpha.powf((dist - 1) as f64);
        scaled.ceil() as i64
    }

    /// Returns a random cost conditioned by direction.
    fn random_cost(&mut self, dir: Direction) -> i64 {
        match dir {
            Direction::X => self.random_int(0, self.max_cost),
            Direction::Y => self.random_int(0, MAX_Y_COST),
        }
    }

    /// Converts grid coordinates to 1-based node id.
    fn grid_to_id(&self, x: i64, y: i64) -> i64 {
        y * self.x + x + 1
    }

    /// Returns 0-based node location index.
    fn node_loc(&self, x: i64, y: i64) -> i64 {
        y * self.x + x
    }

    /// Returns arc location index in the cost/capacity arrays.
    fn arc_loc(&self, x1: i64, y1: i64, x2: i64, y2: i64) -> usize {
        let (xdeg, ydeg) = (self.xdeg, self.ydeg);
        if y1 == y2 {
            let dist = if x1 < x2 { x2 - x1 } else { self.x - (x1 - x2) };
            ((xdeg + ydeg) * self.node_loc(x1, y1) + dist - 1) as usize
        } else {
            let dist = if y1 < y2 { y2 - y1 } else { self.y - (y1 - y2) };
            ((xdeg + ydeg) * self.node_loc(x1, y1) + dist + xdeg - 1) as usize
        }
    }

    /// Computes the number of extra arcs beyond the grid skeleton.
    fn extra_arcs(&self) -> i64 {
        let torus = self.x * self.y * (self.xdeg + self.ydeg);
        let cut = self.y * ((self.xdeg * (self.xdeg + 1)) / 2);
        let st_adjust = 2 * self.xdeg;
        let ret_path = self.x * self.y - 1;

        let base = self.m - torus - cut + st_adjust - ret_path;
        if self.extra_n > 0 {
            base - (self.extra_n + 1)
        } else {
            base
        }
    }

    /// Initializes grid dimensions, degrees, and derived constants.
    fn initialize(&mut self) {
        // Compute Y: largest Y such that Y^3 <= N
        self.y = 1;
        loop {
            self.y += 1;
            if self.y * self.y * self.y > self.n {
                break;
            }
        }
        self.y -= 1;

        // Compute X = Y^2, then stretch Y and X to fill N
        self.x = self.y * self.y;
        loop {
            self.y += 1;
            if self.x * self.y > self.n {
                break;
            }
        }
        self.y -= 1;
        loop {
            self.x += 1;
            if self.x * self.y > self.n {
                break;
            }
        }
        self.x -= 1;

        self.extra_n = self.n - self.x * self.y;

        // Initialize degrees
        self.ydeg = 1;
        loop {
            self.ydeg += 1;
            self.xdeg = self.ydeg * self.ydeg;
            if self.extra_arcs() < 0 {
                break;
            }
        }
        self.ydeg -= 1;
        self.xdeg = self.ydeg * self.ydeg;

        loop {
            self.ydeg += 1;
            if self.extra_arcs() < 0 {
                break;
            }
        }
        self.ydeg -= 1;
        if self.ydeg >= self.y {
            self.ydeg = self.y - 1;
        }

        loop {
            self.xdeg += 1;
            if self.extra_arcs() < 0 {
                break;
            }
        }
        self.xdeg -= 1;
        if self.xdeg >= self.x {
            self.xdeg = self.x - 1;
        }

        self.extra_m = self.extra_arcs();

        self.max_deg = self.xdeg.max(self.ydeg);
        self.s = 1;
        self.t = self.x * self.y;

        self.small_cap = (self.max_cap as f64).sqrt().ceil() as i64;

        self.ret_cost = self.max_cost / self.y;
        if self.ret_cost == 0 {
            self.ret_cost = 1;
        }

        self.supply = self.small_cap;

        self.alpha = (self.max_cap as f64).powf(1.0 / (self.max_deg + 2) as f64);

        let size = (self.x * self.y * (self.xdeg + self.ydeg)) as usize;
        self.capacity = vec![0; size];
        self.cost = vec![0; size];
    }

    /// Builds the transportation problem, writing DIMACS `.min` format to `out`.
    fn build(&mut self, out: &mut impl fmt::Write) -> fmt::Result {
        let (gx, gy) = (self.x, self.y);
        let (xdeg, ydeg) = (self.xdeg, self.ydeg);

        // Generate costs and capacities
        for y in 0..gy {
            for x in 0..gx {
                // x-direction edges
                for i in 1..=xdeg {
                    let z = (x + i) % gx;
                    let u = self.random_capacity(i);
                    let c = self.random_cost(Direction::X);
                    let loc = self.arc_loc(x, y, z, y);
                    self.cost[loc] = c;
                    self.capacity[loc] = u;
                    if (x >= z) && (self.grid_to_id(x, y) != self.t) {
                        self.supply += self.capacity[loc];
                    }
                    if self.grid_to_id(z, y) == self.t {
                        self.supply += self.capacity[loc];
                    }
                }
                // y-direction edges
                for i in 1..=ydeg {
                    let z = (y + i) % gy;
                    let u = self.random_int(1, self.max_cap);
                    let c = self.random_cost(Direction::Y);
                    let loc = self.arc_loc(x, y, x, z);
                    self.cost[loc] = c;
                    self.capacity[loc] = u;
                }
            }
        }

        // Print node descriptors
        writeln!(out, "n {:>8} {:>10}", self.s, self.supply)?;
        writeln!(out, "n {:>8} {:>10}", self.t, -self.supply)?;

        // Print arcs
        for y in 0..gy {
            for x in 0..gx {
                // x direction
                for i in 1..=xdeg {
                    let z = (x + i) % gx;
                    let loc = self.arc_loc(x, y, z, y);
                    if z > x {
                        writeln!(
                            out,
                            "a {:>8} {:>8} {:>10} {:>10} {:>10}",
                            self.grid_to_id(x, y),
                            self.grid_to_id(z, y),
                            0,
                            self.capacity[loc],
                            self.cost[loc],
                        )?;
                    } else {
                        if self.grid_to_id(x, y) != self.t {
                            writeln!(
                                out,
                                "a {:>8} {:>8} {:>10} {:>10} {:>10}",
                                self.grid_to_id(x, y),
                                self.t,
                                0,
                                self.capacity[loc],
                                self.cost[loc],
                            )?;
                        }
                        if self.grid_to_id(z, y) != self.s {
                            writeln!(
                                out,
                                "a {:>8} {:>8} {:>10} {:>10} {:>10}",
                                self.s,
                                self.grid_to_id(z, y),
                                0,
                                self.capacity[loc],
                                self.cost[loc],
                            )?;
                        }
                    }
                }
                // y direction
                for i in 1..=ydeg {
                    let z = (y + i) % gy;
                    let loc = self.arc_loc(x, y, x, z);
                    writeln!(
                        out,
                        "a {:>8} {:>8} {:>10} {:>10} {:>10}",
                        self.grid_to_id(x, y),
                        self.grid_to_id(x, z),
                        0,
                        self.capacity[loc],
                        self.cost[loc],
                    )?;
                }
            }
        }

        // Extra nodes or extra arcs
        if self.extra_n > 0 {
            writeln!(
                out,
                "a {:>8} {:>8} {:>10} {:>10} {:>10}",
                self.s,
                gx * gy + 1,
                0,
                self.small_cap,
                self.max_cost / 2,
            )?;
            writeln!(
                out,
                "a {:>8} {:>8} {:>10} {:>10} {:>10}",
                self.n,
                self.t,
                0,
                self.small_cap,
                self.max_cost / 2,
            )?;
            for i in 1..self.extra_n {
                writeln!(
                    out,
                    "a {:>8} {:>8} {:>10} {:>10} {:>10}",
                    gx * gy + i,
                    gx * gy + i + 1,
                    0,
                    self.small_cap,
                    self.max_cost / 2,
                )?;
            }
            for _ in 1..=self.extra_m {
                let rx = self.random_int(2, gx * gy - 1);
                let ry = self.random_int(gx * gy + 1, self.n);
                let cap = self.random_capacity(xdeg);
                let cst = self.random_cost(Direction::Y);
                if self.random_bit() {
                    writeln!(
                        out,
                        "a {:>8} {:>8} {:>10} {:>10} {:>10}",
                        rx, ry, 0, cap, cst,
                    )?;
                } else {
                    writeln!(
                        out,
                        "a {:>8} {:>8} {:>10} {:>10} {:>10}",
                        ry, rx, 0, cap, cst,
                    )?;
                }
            }
        } else {
            for _ in 1..=self.extra_m {
                let rx = self.random_int(1, gx * gy - 1);
                let mut ry = self.random_int(1, gx * gy - 1);
                while rx == ry {
                    ry = self.random_int(1, gx * gy - 1);
                }
                let cap = self.random_capacity(xdeg);
                let cst = self.random_cost(Direction::Y);
                writeln!(
                    out,
                    "a {:>8} {:>8} {:>10} {:>10} {:>10}",
                    rx, ry, 0, cap, cst,
                )?;
            }
        }

        // Return path
        for x in 0..gx {
            for y in 0..gy - 1 {
                writeln!(
                    out,
                    "a {:>8} {:>8} {:>10} {:>10} {:>10}",
                    self.grid_to_id(x, y),
                    self.grid_to_id(x, y + 1),
                    0,
                    self.supply,
                    self.ret_cost,
                )?;
            }
            if x < gx - 1 {
                writeln!(
                    out,
                    "a {:>8} {:>8} {:>10} {:>10} {:>10}",
                    self.grid_to_id(x, gy - 1),
                    self.grid_to_id(x + 1, 0),
                    0,
                    self.supply,
                    self.ret_cost,
                )?;
            }
        }

        Ok(())
    }
}

/// Writes the header lines and delegates to [`GotoGenerator::build`].
fn generate_impl(g: &mut GotoGenerator, out: &mut impl fmt::Write) -> fmt::Result {
    writeln!(
        out,
        "c Grid example: N = {} M = {} MAXCAP = {} MAXCOST = {} SEED = {}",
        g.n, g.m, g.max_cap, g.max_cost, g.seed,
    )?;
    writeln!(out, "c X={} Y={} XDEG={} YDEG={}", g.x, g.y, g.xdeg, g.ydeg,)?;
    writeln!(out, "p min {} {}", g.n, g.m)?;
    g.build(out)
}

/// Generates a GOTO problem, writing DIMACS `.min` format into any [`fmt::Write`] sink.
///
/// This is the core entry point. Use this when you want to control the
/// destination (e.g. writing into a pre-allocated `String`, a test buffer,
/// or an adapter wrapping `io::Write`).
pub fn generate(params: &GotoParams, out: &mut impl fmt::Write) -> Result<(), GotoError> {
    validate(params)?;
    let mut g = GotoGenerator::new(params);
    g.initialize();
    generate_impl(&mut g, out)?;
    Ok(())
}

/// Convenience: generates the problem and returns it as a [`String`].
pub fn generate_to_string(params: &GotoParams) -> Result<String, GotoError> {
    let mut out = String::new();
    generate(params, &mut out)?;
    Ok(out)
}

/// Adapter that bridges [`io::Write`] to [`fmt::Write`].
///
/// Wraps any `io::Write` so it can be used with [`generate`]. Write errors
/// are stored internally and surfaced when [`into_io_result`](IoFmtAdapter::into_io_result)
/// is called.
struct IoFmtAdapter<W: io::Write> {
    inner: W,
    error: Option<io::Error>,
}

impl<W: io::Write> IoFmtAdapter<W> {
    fn new(inner: W) -> Self {
        Self { inner, error: None }
    }

    /// Consumes the adapter and returns an `io::Error` if one occurred, or `Ok(())`.
    fn into_io_result(self) -> io::Result<()> {
        match self.error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

impl<W: io::Write> fmt::Write for IoFmtAdapter<W> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.inner.write_all(s.as_bytes()).map_err(|e| {
            self.error = Some(e);
            fmt::Error
        })
    }
}

/// Generates the problem and writes it to stdout.
pub fn generate_to_stdout(params: &GotoParams) -> Result<(), GotoError> {
    validate(params)?;
    let mut g = GotoGenerator::new(params);
    g.initialize();
    let mut adapter = IoFmtAdapter::new(BufWriter::new(io::stdout().lock()));
    generate_impl(&mut g, &mut adapter).map_err(|_| {
        adapter
            .error
            .take()
            .map_or(GotoError::Fmt(fmt::Error), GotoError::Io)
    })?;
    adapter.into_io_result()?;
    Ok(())
}

/// Generates the problem and writes it to a file at the given path.
pub fn generate_to_file(params: &GotoParams, path: &Path) -> Result<(), GotoError> {
    validate(params)?;
    let mut g = GotoGenerator::new(params);
    g.initialize();
    let file = File::create(path)?;
    let mut adapter = IoFmtAdapter::new(BufWriter::new(file));
    generate_impl(&mut g, &mut adapter).map_err(|_| {
        adapter
            .error
            .take()
            .map_or(GotoError::Fmt(fmt::Error), GotoError::Io)
    })?;
    adapter.into_io_result()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_case(params: GotoParams) -> String {
        generate_to_string(&params).expect("generation should succeed")
    }

    #[test]
    fn case1_n15_m90() {
        let output = run_case(GotoParams {
            n: 15,
            m: 90,
            max_cap: 10,
            max_cost: 10,
            seed: 42,
        });
        let expected = include_str!("../../../testdata/case1.txt");
        assert_eq!(output, expected);
    }

    #[test]
    fn case2_n20_m120() {
        let output = run_case(GotoParams {
            n: 20,
            m: 120,
            max_cap: 16,
            max_cost: 16,
            seed: 99,
        });
        let expected = include_str!("../../../testdata/case2.txt");
        assert_eq!(output, expected);
    }

    #[test]
    fn case3_n50_m300() {
        let output = run_case(GotoParams {
            n: 50,
            m: 300,
            max_cap: 100,
            max_cost: 100,
            seed: 7,
        });
        let expected = include_str!("../../../testdata/case3.txt");
        assert_eq!(output, expected);
    }

    #[test]
    fn invalid_params_too_few_nodes() {
        let result = generate_to_string(&GotoParams {
            n: 10,
            m: 60,
            max_cap: 8,
            max_cost: 8,
            seed: 1,
        });
        assert!(matches!(result, Err(GotoError::InvalidParams(_))));
    }
}

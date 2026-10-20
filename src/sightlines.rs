//! Choose the direction a label's leader should leave a face in (port of `sightlines.py`,
//! itself copied from quid2pmi's `sightlines.py`).
//!
//! A leader is only intuitive if you can see the point it lands on. This picks, for each
//! anchor, a direction whose leader leaves the surface through free space rather than
//! tunnelling through the solid on its way to the label.
//!
//! The test is whether the leader passes through *material*, not whether it touches a face. A
//! leader running straight up the wall of a bore lies in that bore's own cylindrical face the
//! whole way, which a ray/face intersection reports as a hit at every step but which is
//! perfectly readable. Sampling the solid's point classification distinguishes the two: points
//! in free space and points lying on a face are both fine, points strictly inside the material
//! are not.
//!
//! What differs from Python: classification is haecceity's [`Classifier`] (ray parity) in place
//! of OpenCascade's `BRepClass3d_SolidClassifier`. It can also answer `Unknown` (every probe ray
//! was ambiguous); such a point is not shown to be free, so it blocks the leader like `In`.

// Coordinate indexing uses [f64; 3] (or a bounded 0..3 loop), never variable-length JSON arrays.
use haecceity::Part;
use haecceity::classify::{Classifier, State};
use haecceity::geom::V3;

/// The six principal directions, used as fallbacks in a fixed order.
pub const PRINCIPAL: [V3; 6] = [
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
];

/// Samples taken along a candidate leader.
pub const SAMPLES: usize = 24;

/// `a` as a unit vector, or None when it has (almost) no length (Python's `normalise`).
pub fn normalise(a: V3) -> Option<V3> {
    let length = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    (length >= 1e-12).then(|| [a[0] / length, a[1] / length, a[2] / length])
}

/// Tests whether a leader from a point escapes the part without entering it.
pub struct SightTester<'a> {
    classifier: Classifier<'a>,
    reach: f64,
}

impl<'a> SightTester<'a> {
    /// A tester for leaders `reach` long against the whole of `part`.
    pub fn new(part: &'a Part, reach: f64) -> Self {
        SightTester {
            classifier: Classifier::new(part),
            reach,
        }
    }

    /// How far a leader can travel from `origin` before entering material.
    pub fn clear_run(&self, origin: V3, direction: V3) -> f64 {
        let Some(unit) = normalise(direction) else {
            return 0.0;
        };
        let step = self.reach / SAMPLES as f64;
        for index in 1..=SAMPLES {
            let t = index as f64 * step;
            let point = [
                origin[0] + unit[0] * t,
                origin[1] + unit[1] * t,
                origin[2] + unit[2] * t,
            ];
            if matches!(self.classifier.classify(point), State::In | State::Unknown) {
                return (index - 1) as f64 * step;
            }
        }
        self.reach
    }

    pub fn is_clear(&self, origin: V3, direction: V3) -> bool {
        self.clear_run(origin, direction) >= self.reach
    }

    /// The first preferred direction with a clear run, else the least obstructed (the first of
    /// equals). Returns the direction and whether its run is genuinely clear, so the caller can
    /// report how many leaders it could not route cleanly.
    pub fn choose(&self, origin: V3, preferred: &[V3]) -> (V3, bool) {
        let mut candidates: Vec<V3> = Vec::new();
        for &direction in preferred.iter().chain(&PRINCIPAL) {
            if let Some(unit) = normalise(direction)
                && !candidates.contains(&unit)
            {
                candidates.push(unit);
            }
        }
        // PRINCIPAL always supplies six nonzero unit vectors, so candidates is nonempty.
        let mut best = candidates[0];
        let mut best_run = -1.0;
        for &candidate in &candidates {
            let run = self.clear_run(origin, candidate);
            if run >= self.reach {
                return (candidate, true);
            }
            if run > best_run {
                (best, best_run) = (candidate, run);
            }
        }
        (best, false)
    }
}

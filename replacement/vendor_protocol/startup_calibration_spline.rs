//! Cubic calibration solve and evaluation from X-M1 1.50 0x722abc and 0x723234.
//! Pinned source index: 55436fc85c0560dd252b0410af6f2427fe198cd3; solver confirmed against actual image-4 instructions.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SplineError { KnotCount, DescendingKnots, Singular }

pub(crate) struct CubicSpline {
    constant: [f32; 20],
    linear: [f32; 20],
    quadratic: [f32; 20],
    cubic: [f32; 20],
}

impl CubicSpline {
    pub(crate) const fn new() -> Self {
        Self { constant: [0.0; 20], linear: [0.0; 20], quadratic: [0.0; 20], cubic: [0.0; 20] }
    }

    pub(crate) fn solve(&mut self, knots: &[f32], values: &[f32]) -> Result<(), SplineError> {
        let count = knots.len();
        if !(2..=20).contains(&count) || values.len() != count { return Err(SplineError::KnotCount); }
        if knots.windows(2).any(|pair| pair[0] > pair[1]) { return Err(SplineError::DescendingKnots); }
        self.constant[..count].copy_from_slice(values);
        let mut matrix = [0.0f32; 400];
        let mut inverse = [0.0f32; 400];
        let mut rhs = [0.0f32; 20];
        for row in 0..count {
            inverse[row * count + row] = 1.0;
            if row == 0 {
                let span = knots[1] - knots[0];
                matrix[0] = span + span;
                matrix[1] = knots[1] - knots[0];
                rhs[row] = (values[1] - values[0]) * 3.0;
            } else if row == count - 1 {
                let span = knots[row] - knots[row - 1];
                matrix[row * count + row - 1] = span;
                let span = knots[row] - knots[row - 1];
                matrix[row * count + row] = span + span;
                rhs[row] = (values[row] - values[row - 1]) * 3.0;
            } else {
                matrix[row * count + row - 1] = knots[row + 1] - knots[row];
                let next = knots[row + 1] - knots[row];
                let previous = knots[row] - knots[row - 1];
                let diagonal = next + previous;
                matrix[row * count + row] = diagonal + diagonal;
                matrix[row * count + row + 1] = knots[row] - knots[row - 1];
                let first = (previous * (values[row + 1] - values[row])) / next;
                let second = (next * (values[row] - values[row - 1])) / previous;
                rhs[row] = (first + second) * 3.0;
            }
        }
        for pivot in 0..count {
            let diagonal = matrix[pivot * count + pivot];
            if diagonal == 0.0 { return Err(SplineError::Singular); }
            for column in 0..count {
                matrix[pivot * count + column] /= diagonal;
                inverse[pivot * count + column] /= diagonal;
            }
            for row in 0..count {
                if row == pivot { continue; }
                let factor = matrix[row * count + pivot] / matrix[pivot * count + pivot];
                for column in 0..count {
                    matrix[row * count + column] -= factor * matrix[pivot * count + column];
                    inverse[row * count + column] -= factor * inverse[pivot * count + column];
                }
            }
        }
        for row in 0..count {
            self.linear[row] = 0.0;
            for column in 0..count { self.linear[row] += inverse[row * count + column] * rhs[column]; }
        }
        for row in 0..count - 1 {
            let span = knots[row + 1] - knots[row];
            let term = self.linear[row] * -2.0 - self.linear[row + 1];
            let difference = (values[row + 1] - values[row]) * 3.0;
            self.quadratic[row] = (term + difference / span) / span;
            let term = self.linear[row] + self.linear[row + 1];
            let difference = values[row + 1] - values[row];
            self.cubic[row] = (term - (difference + difference) / span) / (span * span);
        }
        Ok(())
    }

    pub(crate) fn evaluate(&self, knots: &[f32], query: f32) -> f32 {
        let mut interval = 0;
        for (index, knot) in knots.iter().enumerate() {
            if *knot < query { interval = index; }
        }
        let delta = query - knots[interval];
        let first = delta * self.linear[interval] + self.constant[interval];
        let second = (delta * self.quadratic[interval]) * delta;
        let third = ((delta * self.cubic[interval]) * delta) * delta;
        (first + second) + third
    }
}

//! Shared dependency-free shapes and complex spectral MAC graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Shape {
    pub rows: usize,
    pub parts: usize,
    pub bins: usize,
}
impl Shape {
    pub fn bounded(rows: usize, parts: usize, bins: usize) -> Option<Self> {
        if rows == 0 || rows > 256 || parts == 0 || parts > 4 || bins < 2 || bins > 4097 {
            return None;
        }
        let rows = rows.max(8).next_power_of_two();
        // One in-flight packet, no PCM cache and no on-disk model cache.
        let floats = rows.checked_mul(bins)?.checked_mul(6 * parts + 8)?;
        (floats <= 32 * 1024 * 1024 / 4).then_some(Self { rows, parts, bins })
    }
    pub fn plane(self) -> usize {
        self.rows * self.bins
    }
}

pub(crate) struct Graph {
    pub inputs: Vec<u32>,
    pub outputs: Vec<u32>,
    pub operations: Vec<(i32, [u32; 3], u32)>,
    pub operands: u32,
}
impl Graph {
    pub fn new(parts: usize) -> Self {
        let mut g = Self {
            inputs: (0..(parts * 6) as u32).collect(),
            outputs: Vec::new(),
            operations: Vec::new(),
            operands: (parts * 6 + 1) as u32,
        };
        let activation = (parts * 6) as u32; // INT32 scalar NONE
        for part in 0..parts {
            let base = (part * 6) as u32;
            let mut sums = Vec::new();
            for ear in 0..2u32 {
                let hr = base + 2 + 2 * ear;
                let hi = hr + 1;
                let ar = g.op(18, base, hr, activation);
                let bi = g.op(18, base + 1, hi, activation);
                let ai = g.op(18, base, hi, activation);
                let br = g.op(18, base + 1, hr, activation);
                sums.push(g.op(36, ar, bi, activation));
                sums.push(g.op(0, ai, br, activation));
            }
            if part == 0 {
                g.outputs = sums;
            } else {
                for (i, value) in sums.into_iter().enumerate() {
                    g.outputs[i] = g.op(0, g.outputs[i], value, activation);
                }
            }
        }
        g
    }
    fn op(&mut self, op: i32, a: u32, b: u32, activation: u32) -> u32 {
        let out = self.operands;
        self.operands += 1;
        self.operations.push((op, [a, b, activation], out));
        out
    }
}

//! Original FlatBuffer encoder for the public LiteRT 2.3.0 TFL3 schema.
//! A graph only: no quantization, constant HRIRs, PCM or learned weights.
use super::{Graph, Shape};
struct Buffer(Vec<u8>);
impl Buffer {
    fn align(&mut self, n: usize) {
        while self.0.len() % n != 0 {
            self.0.push(0);
        }
    }
    fn word(&mut self, v: u32) -> usize {
        let at = self.0.len();
        self.0.extend(v.to_le_bytes());
        at
    }
    fn set(&mut self, at: usize, v: u32) {
        self.0[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn link(&mut self, at: usize, to: usize) {
        assert!(to > at);
        self.set(at, (to - at) as u32);
    }
    fn table(&mut self, count: usize, fields: &[(usize, u32)]) -> usize {
        self.align(2);
        let vt = self.0.len();
        self.0.extend(((4 + 2 * count) as u16).to_le_bytes());
        self.0.extend(((4 + 4 * count) as u16).to_le_bytes());
        for i in 0..count {
            self.0.extend(
                (if fields.iter().any(|(f, _)| *f == i) {
                    (4 + 4 * i) as u16
                } else {
                    0
                })
                .to_le_bytes(),
            );
        }
        self.align(4);
        let at = self.word((self.0.len() - vt) as u32);
        for i in 0..count {
            self.word(
                fields
                    .iter()
                    .find(|(f, _)| *f == i)
                    .map(|(_, v)| *v)
                    .unwrap_or(0),
            );
        }
        at
    }
    fn field(t: usize, f: usize) -> usize {
        t + 4 + 4 * f
    }
    fn vector(&mut self, v: &[u32]) -> usize {
        self.align(4);
        let at = self.word(v.len() as u32);
        for &x in v {
            self.word(x);
        }
        at
    }
    fn text(&mut self, s: &str) -> usize {
        self.align(4);
        let at = self.word(s.len() as u32);
        self.0.extend(s.as_bytes());
        self.0.push(0);
        at
    }
    fn text_field(&mut self, t: usize, f: usize, s: &str) {
        let v = self.text(s);
        self.link(Self::field(t, f), v);
    }
    fn vec_field(&mut self, t: usize, f: usize, v: &[u32]) -> usize {
        let at = self.vector(v);
        self.link(Self::field(t, f), at);
        at
    }
}
pub(super) fn encode(shape: Shape) -> Vec<u8> {
    let g = Graph::new(shape.parts);
    let mut b = Buffer(vec![0, 0, 0, 0, b'T', b'F', b'L', b'3']);
    let model = b.table(8, &[(0, 3), (1, 0), (2, 0), (3, 0), (4, 0), (7, 0)]);
    b.link(0, model);
    b.text_field(model, 3, "SDA original FP32 HRTF complex spectral MAC");
    let codes = b.vec_field(model, 1, &[0; 3]);
    // TFLite ADD=0, MUL=18, SUB=41 (NNAPI SUB=36).
    for (i, code) in [0, 18, 41].into_iter().enumerate() {
        let t = b.table(4, &[(0, code), (2, 1), (3, code)]);
        b.link(codes + 4 + 4 * i, t);
    }
    let graphs = b.vec_field(model, 2, &[0]);
    let sub = b.table(5, &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)]);
    b.link(graphs + 4, sub);
    b.text_field(sub, 4, "sda_spectral");
    // Remove NNAPI's activation scalar from tensor numbering.
    let scalar = (shape.parts * 6) as u32;
    let remap = |n: u32| if n > scalar { n - 1 } else { n };
    b.vec_field(sub, 1, &g.inputs);
    b.vec_field(
        sub,
        2,
        &g.outputs.iter().map(|&n| remap(n)).collect::<Vec<_>>(),
    );
    let tensors = b.vec_field(sub, 0, &vec![0; g.operands as usize - 1]);
    for i in 0..g.operands as usize - 1 {
        let t = b.table(9, &[(0, 0), (1, 0), (2, 0), (3, 0), (8, 1)]);
        b.link(tensors + 4 + i * 4, t);
        b.vec_field(t, 0, &[shape.rows as u32, shape.bins as u32]);
        b.text_field(t, 3, &format!("v{i}"));
    }
    let operations = b.vec_field(sub, 3, &vec![0; g.operations.len()]);
    for (i, (op, args, out)) in g.operations.iter().enumerate() {
        let (index, options) = match op {
            0 => (0, 11),
            18 => (1, 21),
            36 => (2, 28),
            _ => unreachable!(),
        };
        let t = b.table(5, &[(0, index), (1, 0), (2, 0), (3, options), (4, 0)]);
        b.link(operations + 4 + i * 4, t);
        b.vec_field(t, 1, &[remap(args[0]), remap(args[1])]);
        b.vec_field(t, 2, &[remap(*out)]);
        let opts = b.table(1, &[(0, 0)]);
        b.link(Buffer::field(t, 4), opts);
    }
    let buffers = b.vec_field(model, 4, &[0]);
    let empty = b.table(0, &[]);
    b.link(buffers + 4, empty);
    let signatures = b.vec_field(model, 7, &[0]);
    let sig = b.table(5, &[(0, 0), (1, 0), (2, 0), (4, 0)]);
    b.link(signatures + 4, sig);
    b.text_field(sig, 2, "serving_default");
    for (field, indices) in [
        (0, g.inputs.clone()),
        (1, g.outputs.iter().map(|&n| remap(n)).collect()),
    ] {
        let maps = b.vec_field(sig, field, &vec![0; indices.len()]);
        for (i, &index) in indices.iter().enumerate() {
            let map = b.table(2, &[(0, 0), (1, index)]);
            b.link(maps + 4 + i * 4, map);
            b.text_field(
                map,
                0,
                &format!("{}{i}", if field == 0 { "input" } else { "output" }),
            );
        }
    }
    b.0
}
#[cfg(test)]
mod tests {
    use super::*;
    fn u32at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }
    fn field(b: &[u8], t: usize, f: usize) -> usize {
        let vt = (t as i64 - i32::from_le_bytes(b[t..t + 4].try_into().unwrap()) as i64) as usize;
        let o = u16::from_le_bytes(b[vt + 4 + f * 2..vt + 6 + f * 2].try_into().unwrap());
        assert_ne!(o, 0);
        t + o as usize
    }
    fn target(b: &[u8], at: usize) -> usize {
        at + u32at(b, at) as usize
    }
    #[test]
    fn public_schema_preserves_float_graph_partition_order_and_no_constants() {
        for p in 1..=4 {
            let shape = Shape::bounded(32, p, 1025).unwrap();
            let bytes = encode(shape);
            let b = &bytes[..];
            assert_eq!(&b[4..8], b"TFL3");
            let m = target(b, 0);
            assert_eq!(u32at(b, field(b, m, 0)), 3);
            let buffers = target(b, field(b, m, 4));
            assert_eq!(u32at(b, buffers), 1);
            let sg = target(b, field(b, m, 2));
            assert_eq!(u32at(b, sg), 1);
            let sub = target(b, sg + 4);
            let ins = target(b, field(b, sub, 1));
            assert_eq!(u32at(b, ins), 6 * p as u32);
            let tensors = target(b, field(b, sub, 0));
            let g = Graph::new(p);
            assert_eq!(u32at(b, tensors), g.operands - 1);
            for i in 0..g.operands as usize - 1 {
                let t = target(b, tensors + 4 + i * 4);
                assert_eq!(b[field(b, t, 1)], 0);
                let s = target(b, field(b, t, 0));
                assert_eq!(u32at(b, s), 2);
                assert_eq!(u32at(b, s + 4), 32);
                assert_eq!(u32at(b, s + 8), 1025);
            }
            let ops = target(b, field(b, sub, 3));
            assert_eq!(u32at(b, ops), g.operations.len() as u32);
            for (i, (op, args, out)) in g.operations.iter().enumerate() {
                let t = target(b, ops + 4 + i * 4);
                let expected = match op {
                    0 => 0,
                    18 => 1,
                    36 => 2,
                    _ => unreachable!(),
                };
                assert_eq!(u32at(b, field(b, t, 0)), expected);
                let remap = |n: u32| if n > (6 * p) as u32 { n - 1 } else { n };
                let iv = target(b, field(b, t, 1));
                assert_eq!(u32at(b, iv), 2);
                assert_eq!(u32at(b, iv + 4), remap(args[0]));
                assert_eq!(u32at(b, iv + 8), remap(args[1]));
                let ov = target(b, field(b, t, 2));
                assert_eq!(u32at(b, ov + 4), remap(*out));
            }
        }
    }
}

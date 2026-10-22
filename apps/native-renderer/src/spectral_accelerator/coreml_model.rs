//! Original deterministic graph, encoded using Apple's public Core ML protobuf
//! schema (coremltools 9.0 mlmodel/format). No trained weights or audio in files.
use super::{Graph, Shape};
fn varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 128 {
        out.push(value as u8 | 128);
        value >>= 7;
    }
    out.push(value as u8);
}
fn scalar(out: &mut Vec<u8>, field: u64, value: u64) {
    varint(out, field << 3);
    varint(out, value);
}
fn message(out: &mut Vec<u8>, field: u64, data: &[u8]) {
    varint(out, field << 3 | 2);
    varint(out, data.len() as u64);
    out.extend_from_slice(data);
}
fn text(out: &mut Vec<u8>, field: u64, value: &str) {
    message(out, field, value.as_bytes());
}
fn feature(name: &str, shape: Shape) -> Vec<u8> {
    let mut dimensions = Vec::new();
    varint(&mut dimensions, shape.rows as u64);
    varint(&mut dimensions, shape.bins as u64);
    let mut array = Vec::new();
    message(&mut array, 1, &dimensions);
    scalar(&mut array, 2, 65568); // FLOAT32
    let mut kind = Vec::new();
    message(&mut kind, 5, &array);
    let mut f = Vec::new();
    text(&mut f, 1, name);
    message(&mut f, 3, &kind);
    f
}
pub(crate) fn encode(shape: Shape) -> Vec<u8> {
    let graph = Graph::new(shape.parts);
    let name = |id: u32| {
        graph
            .outputs
            .iter()
            .position(|i| *i == id)
            .map_or_else(|| format!("v{id}"), |i| format!("o{i}"))
    };
    let mut description = Vec::new();
    for &id in &graph.inputs {
        message(&mut description, 1, &feature(&name(id), shape));
    }
    for &id in &graph.outputs {
        message(&mut description, 10, &feature(&name(id), shape));
    }
    let mut network = Vec::new();
    scalar(&mut network, 5, 1); // EXACT_ARRAY_MAPPING (iOS 13+)
    for (index, (op, args, out)) in graph.operations.iter().enumerate() {
        let mut layer = Vec::new();
        text(&mut layer, 1, &format!("spectral_{index}"));
        text(&mut layer, 2, &name(args[0]));
        text(&mut layer, 2, &name(args[1]));
        text(&mut layer, 3, &name(*out));
        let field = match op {
            0 => 880,
            18 => 900,
            36 => 905,
            _ => unreachable!("unsupported spectral operation"),
        };
        message(&mut layer, field, &[]);
        message(&mut network, 1, &layer);
    }
    let mut model = Vec::new();
    scalar(&mut model, 1, 4);
    message(&mut model, 2, &description);
    message(&mut model, 500, &network);
    model
}
#[cfg(test)]
mod tests {
    use super::*;
    // Decode the emitted wire format separately to catch tags, tensor names,
    // bounds and input/output contracts without requiring an Apple host.
    fn read(data: &[u8], at: &mut usize) -> u64 {
        let mut x = 0;
        let mut shift = 0;
        loop {
            let b = data[*at];
            *at += 1;
            x |= ((b & 127) as u64) << shift;
            if b < 128 {
                return x;
            }
            shift += 7;
            assert!(shift < 64);
        }
    }
    fn fields(data: &[u8]) -> Vec<(u64, u64, Vec<u8>)> {
        let mut at = 0;
        let mut out = Vec::new();
        while at < data.len() {
            let key = read(data, &mut at);
            if key & 7 == 0 {
                let v = read(data, &mut at);
                out.push((key >> 3, v, vec![]));
            } else {
                assert_eq!(key & 7, 2);
                let n = read(data, &mut at) as usize;
                out.push((key >> 3, 0, data[at..at + n].to_vec()));
                at += n;
            }
        }
        out
    }
    #[test]
    fn public_model_schema_and_graph_have_exact_shape_contract() {
        for parts in 1..=4 {
            let shape = Shape::bounded(16, parts, 1025).unwrap();
            let bytes = encode(shape);
            assert!(bytes.len() < 65536);
            let root = fields(&bytes);
            assert_eq!(root[0].0, 1);
            assert_eq!(root[0].1, 4);
            let desc = fields(&root.iter().find(|x| x.0 == 2).unwrap().2);
            assert_eq!(desc.iter().filter(|x| x.0 == 1).count(), parts * 6);
            assert_eq!(desc.iter().filter(|x| x.0 == 10).count(), 4);
            let network = fields(&root.iter().find(|x| x.0 == 500).unwrap().2);
            assert_eq!(
                network.iter().filter(|x| x.0 == 1).count(),
                Graph::new(parts).operations.len()
            );
            assert!(network.iter().any(|x| x.0 == 5 && x.1 == 1));
            for layer in network.iter().filter(|x| x.0 == 1) {
                let f = fields(&layer.2);
                assert_eq!(f.iter().filter(|x| x.0 == 2).count(), 2);
                assert!(f.iter().any(|x| matches!(x.0, 880 | 900 | 905)));
            }
        }
    }
}

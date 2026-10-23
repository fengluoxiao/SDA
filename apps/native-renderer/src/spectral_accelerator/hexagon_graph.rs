//! Original FP32 graph for the public Hexagon NN 1.20 interface.
//! This bypasses TFLite's quantized-only delegate, not its precision safeguards.
use super::{Graph, Shape};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub(super) struct Input {
    pub node: u32,
    pub output: u32,
}
pub(super) struct Node {
    pub id: u32,
    pub operation: u32,
    pub inputs: Vec<Input>,
    pub outputs: usize,
}
pub(super) fn nodes(shape: Shape) -> Vec<Node> {
    let graph = Graph::new(shape.parts);
    let mut values = vec![Input { node: 0, output: 0 }; graph.operands as usize];
    for &i in &graph.inputs {
        values[i as usize] = Input { node: 1, output: i };
    }
    let mut nodes = vec![Node {
        id: 1,
        operation: 0,
        inputs: vec![],
        outputs: graph.inputs.len(),
    }];
    for (i, (op, args, out)) in graph.operations.iter().enumerate() {
        let id = 2 + i as u32;
        nodes.push(Node {
            id,
            operation: match op {
                0 => 66,
                18 => 67,
                36 => 75,
                _ => unreachable!(),
            },
            inputs: vec![values[args[0] as usize], values[args[1] as usize]],
            outputs: 1,
        });
        values[*out as usize] = Input {
            node: id,
            output: 0,
        };
    }
    nodes.push(Node {
        id: nodes.len() as u32 + 1,
        operation: 1,
        inputs: graph.outputs.iter().map(|i| values[*i as usize]).collect(),
        outputs: 0,
    });
    nodes
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_float_graph_preserves_both_ears_partition_order_and_input_ports() {
        for parts in 1..=4 {
            let shape = Shape::bounded(8, parts, 17).unwrap();
            let g = nodes(shape);
            let n = shape.plane();
            let mut values = std::collections::HashMap::new();
            for i in 0..6 * parts {
                values.insert(
                    (1, i as u32),
                    (0..n)
                        .map(|j| ((i * n + j) * 7919 % 1021) as f32 / 511. - 1.)
                        .collect::<Vec<_>>(),
                );
            }
            let inputs = values.clone();
            for node in &g[1..g.len() - 1] {
                let a = &values[&(node.inputs[0].node, node.inputs[0].output)];
                let b = &values[&(node.inputs[1].node, node.inputs[1].output)];
                let v = a
                    .iter()
                    .zip(b)
                    .map(|(&a, &b)| match node.operation {
                        66 => a + b,
                        67 => a * b,
                        75 => a - b,
                        _ => panic!(),
                    })
                    .collect();
                values.insert((node.id, 0), v);
            }
            for (ear, out) in g.last().unwrap().inputs.chunks_exact(2).enumerate() {
                for j in 0..n {
                    let mut r = 0.;
                    let mut im = 0.;
                    for p in 0..parts {
                        let a = inputs[&(1, (6 * p) as u32)][j];
                        let b = inputs[&(1, (6 * p + 1) as u32)][j];
                        let h = inputs[&(1, (6 * p + 2 + 2 * ear) as u32)][j];
                        let k = inputs[&(1, (6 * p + 3 + 2 * ear) as u32)][j];
                        r += a * h - b * k;
                        im += a * k + b * h;
                    }
                    assert_eq!(values[&(out[0].node, out[0].output)][j], r);
                    assert_eq!(values[&(out[1].node, out[1].output)][j], im);
                }
            }
            assert_eq!(g[0].outputs, 6 * parts);
            assert_eq!(g.last().unwrap().inputs.len(), 4);
        }
    }
}

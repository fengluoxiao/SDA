#[path = "../apps/native-renderer/src/spectral_accelerator/model_graph.rs"]
mod model_graph;
use model_graph::{Graph, Shape};
#[path = "../apps/native-renderer/src/spectral_accelerator/coreml_model.rs"]
mod coreml_model;
fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("output directory"));
    std::fs::create_dir_all(&out).unwrap();
    for parts in 1..=4 {
        let shape = Shape::bounded(8, parts, 17).unwrap();
        std::fs::write(
            out.join(format!("spectral-{parts}.mlmodel")),
            coreml_model::encode(shape),
        )
        .unwrap();
    }
    let shape = Shape::bounded(32, 2, 1025).unwrap();
    std::fs::write(
        out.join("spectral-live-shape.mlmodel"),
        coreml_model::encode(shape),
    )
    .unwrap();
}

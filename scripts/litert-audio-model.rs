//! Export the exact production graphs for independent runtime validation.
#[path = "../apps/native-renderer/src/spectral_accelerator/model_graph.rs"]
mod model_graph;
use model_graph::{Graph, Shape};
#[path = "../apps/native-renderer/src/spectral_accelerator/hexagon_graph.rs"]
mod hexagon_graph;
#[path = "../apps/native-renderer/src/spectral_accelerator/litert_model.rs"]
mod litert_model;
fn main() {
    let directory = std::path::PathBuf::from(std::env::args().nth(1).expect("output directory"));
    std::fs::create_dir_all(&directory).unwrap();
    for p in 1..=4 {
        std::fs::write(
            directory.join(format!("spectral-{p}.tflite")),
            litert_model::encode(Shape::bounded(8, p, 17).unwrap()),
        )
        .unwrap();
    }
    std::fs::write(
        directory.join("spectral-live.tflite"),
        litert_model::encode(Shape::bounded(32, 2, 1025).unwrap()),
    )
    .unwrap();
}

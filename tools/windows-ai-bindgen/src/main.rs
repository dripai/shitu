use std::{env, path::PathBuf};

fn main() {
    let runtime = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .expect("usage: gridstart-windows-ai-bindgen <Windows App Runtime directory>");
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("src")
        .join("platform")
        .join("windows")
        .join("windows_ai_bindings.rs");

    let inputs = [
        runtime.join("Microsoft.Windows.AI.winmd"),
        runtime.join("Microsoft.Windows.AI.Imaging.winmd"),
        runtime.join("Microsoft.Graphics.Imaging.winmd"),
    ];
    for input in &inputs {
        assert!(input.is_file(), "missing metadata: {}", input.display());
    }

    let arguments = vec![
        "--in".to_owned(),
        "default".to_owned(),
        inputs[0].to_string_lossy().into_owned(),
        inputs[1].to_string_lossy().into_owned(),
        inputs[2].to_string_lossy().into_owned(),
        "--out".to_owned(),
        output.to_string_lossy().into_owned(),
        "--reference".to_owned(),
        "windows".to_owned(),
        "--filter".to_owned(),
        "Microsoft.Windows.AI.AIFeatureReadyState".to_owned(),
        "Microsoft.Windows.AI.AIFeatureReadyResult".to_owned(),
        "Microsoft.Windows.AI.AIFeatureReadyResultState".to_owned(),
        "Microsoft.Windows.AI.Imaging.TextRecognizer".to_owned(),
        "Microsoft.Windows.AI.Imaging.RecognizedText".to_owned(),
        "Microsoft.Windows.AI.Imaging.RecognizedLine".to_owned(),
        "Microsoft.Windows.AI.Imaging.RecognizedWord".to_owned(),
        "Microsoft.Windows.AI.Imaging.RecognizedTextBoundingBox".to_owned(),
        "Microsoft.Windows.AI.Imaging.RecognizedLineStyle".to_owned(),
        "Microsoft.Graphics.Imaging.ImageBuffer".to_owned(),
        "Microsoft.Graphics.Imaging.ImageBufferPixelFormat".to_owned(),
    ];

    let warnings = windows_bindgen::bindgen(arguments);
    assert!(warnings.is_empty(), "binding warnings: {warnings}");
}

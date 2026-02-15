import torch
import timm
import json
import os
import urllib.request

# Based on tutorial: https://huggingface.co/docs/timm/models/mobilenet-v3

def export_model():
    # 0. Setup output directory
    output_dir = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "models")
    os.makedirs(output_dir, exist_ok=True)
    
    # 1. Load MobileNet V3 Large (pretrained on ImageNet)
    # num_classes=0 removes the classifier head, returning the 1280-dim feature vector.
    model_name = 'mobilenetv3_large_100'
    print(f"Loading {model_name} (Feature Extractor)...")
    model = timm.create_model(model_name, pretrained=True, num_classes=0)
    model.eval()

    # 2. Export to ONNX
    output_onnx = os.path.join(output_dir, "mobilenetv3.onnx")
    print(f"Exporting model to {output_onnx}...")

    # Create dummy input (1 image, 3 channels, 224x224 resolution)
    dummy_input = torch.randn(1, 3, 224, 224)

    torch.onnx.export(
        model,
        dummy_input,
        output_onnx,
        export_params=True,
        opset_version=17,
        do_constant_folding=True,
        input_names=['input'],
        output_names=['output'],
        dynamic_axes={'input': {0: 'batch_size'}, 'output': {0: 'batch_size'}}
    )

    # 2.5. Ensure single file (inline weights)
    import onnx
    print("Ensuring model is self-contained...")
    onnx_model = onnx.load(output_onnx)
    
    # onnx.save will inline data for small models by default
    onnx.save(onnx_model, output_onnx)

    # Remove the .data file if it exists
    data_file = output_onnx + ".data"
    if os.path.exists(data_file):
        print(f"Removing external data file: {data_file}")
        os.remove(data_file)

    print("Done!")
    print(f"Model saved to: {os.path.abspath(output_onnx)}")

if __name__ == "__main__":
    export_model()

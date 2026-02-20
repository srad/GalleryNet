import onnxruntime as ort
import numpy as np

def check_model(path):
    try:
        sess = ort.InferenceSession(path)
        print(f"Model: {path}")
        for i, input in enumerate(sess.get_inputs()):
            print(f"  Input {i}: {input.name}, {input.shape}, {input.type}")
        for i, output in enumerate(sess.get_outputs()):
            print(f"  Output {i}: {output.name}, {output.shape}, {output.type}")
    except Exception as e:
        print(f"Error loading {path}: {e}")

if __name__ == "__main__":
    check_model("assets/models/version-slim-320.onnx")

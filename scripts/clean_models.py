import onnx
import sys

def clean_model(model_path):
    """
    Cleans an ONNX model by:
    1. Removing initializers from graph inputs (silences 'appears in graph inputs' warnings)
    2. Removing unused initializers (silences 'not used by any node' warnings)
    """
    try:
        model = onnx.load(model_path)
        
        # Get set of all initializers
        initializers = {i.name for i in model.graph.initializer}
        
        # 1. Fix overlapping inputs/initializers
        new_inputs = [i for i in model.graph.input if i.name not in initializers]
        while len(model.graph.input) > 0:
            model.graph.input.pop()
        model.graph.input.extend(new_inputs)
        
        # 2. Identify used inputs by nodes and outputs
        used_names = set()
        for node in model.graph.node:
            for input_name in node.input:
                used_names.add(input_name)
        for output in model.graph.output:
            used_names.add(output.name)
            
        # Remove initializers that aren't used anywhere
        new_initializers = [i for i in model.graph.initializer if i.name in used_names]
        
        if len(new_initializers) < len(model.graph.initializer):
            removed = len(model.graph.initializer) - len(new_initializers)
            print(f"Removing {removed} unused initializers from {model_path}")
            
        while len(model.graph.initializer) > 0:
            model.graph.initializer.pop()
        model.graph.initializer.extend(new_initializers)
        
        onnx.save(model, model_path)
        print(f"Successfully cleaned: {model_path}")
    except Exception as e:
        print(f"Failed to clean {model_path}: {e}")

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python clean_models.py <model1.onnx> <model2.onnx> ...")
    else:
        for path in sys.argv[1:]:
            clean_model(path)

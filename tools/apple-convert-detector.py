"""Convert the raw detector trace to upstream-compatible Core ML functions on a Mac."""
import argparse
from pathlib import Path
import tempfile

import coremltools as ct
import torch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    network = torch.jit.load(str(args.directory / "detector-raw.torchscript")).eval()
    converted = ct.convert(network, inputs=[ct.TensorType(name="x",shape=(1,3,640,640))],
        outputs=[ct.TensorType(name="boxes"),ct.TensorType(name="scores")], convert_to="mlprogram",
        compute_precision=ct.precision.FLOAT16, minimum_deployment_target=ct.target.macOS15)
    with tempfile.TemporaryDirectory() as temporary:
        single = str(Path(temporary) / "single.mlpackage")
        converted.save(single)
        functions = ct.utils.MultiFunctionDescriptor()
        functions.add_function(single, src_function_name="main", target_function_name="s640x640")
        functions.default_function_name = "s640x640"
        ct.utils.save_multifunction(functions, str(args.directory / "detector-640.mlpackage"))


if __name__ == "__main__":
    main()

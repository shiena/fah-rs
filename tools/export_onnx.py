"""Export fixed-shape FP32 models for the Windows ONNX Runtime backends."""
import torch
import onnx
from export import ROOT, Pose, upstream_modules


def main():
    torch.set_num_threads(8)
    torch.manual_seed(42)
    directory = ROOT / "models"
    pose = Pose(directory / "model.pt").eval().requires_grad_(False)
    inputs = (torch.rand(1, 3, 320, 320), torch.rand(1, 400, 99), torch.rand(1, 3))
    torch.onnx.export(pose, inputs, directory / "pose.onnx", dynamo=False,
                      opset_version=17, input_names=["img", "rays", "cond"],
                      output_names=[f"out{i}" for i in range(6)])
    upstream_modules()
    from fah.yolo import load_yolo

    class Detector(torch.nn.Module):
        def __init__(self):
            super().__init__()
            self.net = load_yolo(directory / "detector.pt")

        def forward(self, image):
            result = self.net(image)
            return result[0][:, :4 + self.net.head.classes] if isinstance(result, tuple) else result

    detector = Detector().eval().requires_grad_(False)
    torch.onnx.export(detector, (torch.zeros(1, 3, 640, 640),), directory / "detector.onnx",
                      dynamo=False, opset_version=17, input_names=["img"], output_names=["out0"])
    for name in ["pose", "detector"]:
        model = onnx.load(directory / f"{name}.onnx")
        onnx.checker.check_model(model)
        print(name, [(v.name, [d.dim_value for d in v.type.tensor_type.shape.dim]) for v in model.graph.output])


if __name__ == "__main__":
    main()

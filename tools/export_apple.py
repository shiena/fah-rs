"""Export existing FAH checkpoints for the Apple adapter without loading MANO."""
import argparse
import importlib.util
from pathlib import Path
from types import SimpleNamespace as N

import torch
import export as shared


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--models", type=Path, default=shared.ROOT / "models")
    parser.add_argument("--out", type=Path, default=shared.ROOT / "models/apple-export")
    args = parser.parse_args()
    torch.set_num_threads(8)
    source = shared.ROOT / "apple/Vendor/fah-metal/tools/export.py"
    spec = importlib.util.spec_from_file_location("fah_metal_export", source)
    upstream = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(upstream)
    pose = shared.Pose(args.models / "model.pt").eval()
    state = torch.load(args.models / "model.pt", map_location="cpu", weights_only=True)["trainable"]
    camera = N(conv=N(weight=state["model.ray_cond_emb_hand.conv.weight"]), norm=pose.norm,
               patch_size=16, camera=N(num_bands=16, max_resolution=[64,64,64]))
    model = N(keypoint_head=pose.head, verify_head=pose.verify,
              image_mean=pose.encoder.mean, image_std=pose.encoder.std, ray_cond_emb_hand=camera,
              cfg=N(MODEL=N(PROMPT_ENCODER={"MASK_EMBED_TYPE": "mask"})),
              prompt_encoder=N(no_mask_embed=N(weight=state["model.prompt_encoder.no_mask_embed.weight"])))
    args.out.mkdir(parents=True, exist_ok=True)
    upstream.export_pose(N(student=pose.encoder, model=model), str(args.out / "fah-s-pose"))
    from fah.yolo import load_yolo

    class Detector(torch.nn.Module):
        def __init__(self):
            super().__init__()
            self.net = load_yolo(args.models / "detector.pt")

        def forward(self, image):
            output = self.net(image, raw=True)
            return output["boxes"], output["scores"]

    with torch.no_grad():
        traced = torch.jit.trace(Detector().eval(), torch.zeros(1,3,640,640))
        traced.save(str(args.out / "detector-raw.torchscript"))


if __name__ == "__main__":
    main()

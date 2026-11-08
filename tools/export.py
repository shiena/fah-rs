"""Export the FAH-S pose and detector networks without loading MANO or MHR."""
import argparse
import importlib
import json
import math
from pathlib import Path
import sys
import types

import numpy as np
import torch
from torch import nn
from torch.nn import functional as F

ROOT = Path(__file__).resolve().parents[1]
UPSTREAM = ROOT / "third_party/FAH"


def upstream_modules():
    # Load inference modules without FAH's full application initialization.
    for name in ["fah", "fah.models", "fah.models.heads"]:
        package = types.ModuleType(name)
        package.__path__ = [str(UPSTREAM / name.replace(".", "/"))]
        sys.modules[name] = package
    from fah.student import StudentEncoder
    from fah.models.heads.keypoint_head import HandKeypointHead
    from fah.models.heads.mask_head import HandVerifyHead
    return StudentEncoder, HandKeypointHead, HandVerifyHead


def subset(state, prefix):
    return {k[len(prefix):]: v.float() if v.is_floating_point() else v
            for k, v in state.items() if k.startswith(prefix)}


class ChannelNorm(nn.Module):
    def __init__(self, source):
        super().__init__()
        self.weight = source.weight
        self.bias = source.bias
        self.eps = source.eps

    def forward(self, x):
        return F.layer_norm(x.permute(0, 2, 3, 1), (x.shape[1],),
                            self.weight, self.bias, self.eps).permute(0, 3, 1, 2)


def replace_norms(module):
    for name, child in list(module.named_children()):
        if type(child).__name__ == "LayerNorm2d":
            setattr(module, name, ChannelNorm(child))
        else:
            replace_norms(child)


class Pose(nn.Module):
    def __init__(self, checkpoint):
        super().__init__()
        Student, Head, Verify = upstream_modules()
        ck = torch.load(checkpoint, map_location="cpu", weights_only=True)
        sd = ck["trainable"]
        self.encoder = Student(ck["student_name"], pretrained=False, size=320)
        self.encoder.load_state_dict(subset(sd, "encoder."), strict=True)
        v = self.encoder.vit
        with torch.no_grad():
            _, rope = v._pos_embed(v.patch_embed(torch.zeros(1, 3, 320, 320)))
        sin, cos = rope.chunk(2, -1)
        self.register_buffer("rope_sin", sin.reshape(1, 1, 400, 64))
        self.register_buffer("rope_cos", cos.reshape(1, 1, 400, 64))
        self.register_buffer("prefix", torch.cat([v.cls_token, v.reg_token], 1).detach().clone())
        hs = subset(sd, "model.keypoint_head.")
        self.head = Head(dim=64, pool=True, depth=True, vis=True,
                         dense=hs["dense"].tolist(), dense_vidx=hs["dense_vidx"].tolist())
        required = self.head.state_dict()
        self.head.load_state_dict({k: hs[k] for k in required}, strict=True)
        self.verify = Verify()
        self.verify.load_state_dict(subset(sd, "model.verify_head."), strict=True)
        ad = self.encoder.adapter
        w = sd["model.ray_cond_emb_hand.conv.weight"][:, :, 0, 0].double()
        wi, wr = w[:, :1280], w[:, 1280:]
        nm = sd["model.prompt_encoder.no_mask_embed.weight"].reshape(-1).double()
        self.fold = nn.Linear(1280, 1280)
        self.fold.weight = nn.Parameter((wi @ ad[3].weight.double()).float())
        self.fold.bias = nn.Parameter((wi @ (ad[3].bias.double() + nm)).float())
        self.ray = nn.Linear(99, 1280, bias=False)
        self.ray.weight = nn.Parameter(wr.float())
        self.norm = nn.LayerNorm(1280, eps=1e-6)
        self.norm.weight = nn.Parameter(sd["model.ray_cond_emb_hand.norm.weight"].float())
        self.norm.bias = nn.Parameter(sd["model.ray_cond_emb_hand.norm.bias"].float())
        self.register_buffer("xs", ((torch.arange(80) + .5) / 80).repeat(80))
        self.register_buffer("ys", ((torch.arange(80) + .5) / 80).repeat_interleave(80))
        replace_norms(self.head)

    def reference(self, img, rays, cond):
        e, h = self.encoder, self.head
        x = (img - e.mean) / e.std
        t = e.vit.forward_features(x)[:, e.num_prefix:]
        a = e.adapter[2](e.adapter[1](e.adapter[0](t)))
        tok = self.norm(self.fold(a) + self.ray(rays))
        pose = h.pool(torch.cat([tok.mean(1), cond], -1))
        uv, xyz, _ = h(tok, (20, 20), pose, x, cond=cond, return_conf=True)
        return uv, xyz, h.last_conf, h.last_vis, h.depth(pose), self.verify(pose)

    def forward(self, img, rays, cond):
        e, h = self.encoder, self.head
        x = (img - e.mean) / e.std
        v = e.vit
        t = torch.cat([self.prefix, v.patch_embed.proj(x).flatten(2).transpose(1, 2)], 1)
        for block in v.blocks:
            z = F.layer_norm(t, (384,), block.norm1.weight, block.norm1.bias, block.norm1.eps)
            qkv = block.attn.qkv(z)
            q = qkv[:, :, :384].reshape(1, 405, 6, 64).permute(0, 2, 1, 3)
            k = qkv[:, :, 384:768].reshape(1, 405, 6, 64).permute(0, 2, 1, 3)
            val = qkv[:, :, 768:].reshape(1, 405, 6, 64).permute(0, 2, 1, 3)
            def rotate(a):
                patches = a[:, :, 5:, :]
                turned = torch.cat([-patches[:, :, :, 32:], patches[:, :, :, :32]], -1)
                return torch.cat([a[:, :, :5, :], patches * self.rope_cos + turned * self.rope_sin], 2)
            q, k = rotate(q), rotate(k)
            att = (q @ k.transpose(-1, -2) * .125).softmax(-1)
            z = (att @ val).transpose(1, 2).reshape(1, 405, 384)
            t = t + block.attn.proj(z) * block.gamma_1
            z = F.layer_norm(t, (384,), block.norm2.weight, block.norm2.bias, block.norm2.eps)
            t = t + block.mlp.fc2(F.gelu(block.mlp.fc1(z))) * block.gamma_2
        t = F.layer_norm(t, (384,), v.norm.weight, v.norm.bias, v.norm.eps)[:, 5:]
        a = e.adapter[2](e.adapter[1](e.adapter[0](t)))
        tok = self.norm(self.fold(a) + self.ray(rays))
        pose = h.pool(torch.cat([tok.mean(1), cond], -1))
        f = h.fuse(torch.cat([h.up(h.img_proj(tok.transpose(1, 2).reshape(1, 1280, 20, 20))), h.stem(x)], 1))
        q = h.qmlp(h.query(pose).unsqueeze(1) + h.point.unsqueeze(0))
        flat = f.flatten(2)
        p = (torch.bmm(q, flat) * (h.log_temp.exp() / 8)).softmax(-1)
        uv = torch.stack([(p * self.xs).sum(-1), (p * self.ys).sum(-1)], -1)
        pooled = torch.bmm(p, flat.transpose(1, 2))
        pq = torch.cat([pooled, q], -1)
        w = h.xyz[0].weight
        hidden = F.linear(pq, w[:, :128]) + F.linear(pose, w[:, 128:], h.xyz[0].bias).unsqueeze(1)
        xyz = h.xyz[2](h.xyz[1](hidden))
        return uv, xyz, p.amax(-1), h.vis(pq).squeeze(-1), h.depth(pose), self.verify.mlp(pose)


def camera_features(ray):
    yy, xx = torch.meshgrid(torch.arange(320).float(), torch.arange(320).float(), indexing="ij")
    grid = F.interpolate(torch.stack([xx, yy])[None], (20, 20), mode="bilinear", align_corners=False, antialias=True)
    grid = grid[0].permute(1, 2, 0).reshape(400, 2)
    grid.numpy().tofile(ROOT / "models/uv_grid.f32")
    xy = grid * torch.tensor([ray[0], ray[2]]) + torch.tensor([ray[1], ray[3]])
    pos = torch.cat([xy, torch.ones(400, 1)], -1)
    phase = (pos[:, :, None] * torch.linspace(1, 32, 16)).reshape(400, 48) * math.pi
    return torch.cat([pos, phase.sin(), phase.cos()], -1)[None]


def convert(model, inputs, stem):
    import pnnx
    model.eval().requires_grad_(False)
    with torch.no_grad():
        traced = torch.jit.trace(model, inputs, check_trace=False)
        traced.save(str(stem) + ".torchscript")
    pnnx.convert(stem.as_posix() + ".torchscript", inputs=inputs, fp16=False)
    if stem.name == "pose":
        path = Path(str(stem) + ".ncnn.param")
        lines = path.read_text().splitlines()
        expanded = 0
        squeezed = 0
        for i, line in enumerate(lines):
            fields = line.split()
            if fields and fields[0] == "ExpandDims":
                # These two static vector-to-row views need no host round trip.
                width = [64, 256][expanded]
                lines[i] = " ".join(["Reshape", *fields[1:6], f"0={width}", "1=1"])
                expanded += 1
            elif fields and fields[0] == "Squeeze":
                lines[i] = " ".join(["Reshape", *fields[1:6], "0=461"])
                squeezed += 1
        assert expanded == 2 and squeezed == 1
        path.write_text("\n".join(lines) + "\n")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("kind", choices=["pose", "detector"])
    args = ap.parse_args()
    torch.set_num_threads(8)
    torch.manual_seed(42)
    out = ROOT / "models"
    if args.kind == "pose":
        net = Pose(out / "model.pt").eval()
        img = torch.rand(1, 3, 320, 320)
        rays = camera_features([.002, -.32, .002, -.32])
        cond = torch.tensor([[0., 0., .64]])
        inputs = (img, rays, cond)
        with torch.no_grad():
            outputs = net(*inputs)
            reference = net.reference(*inputs)
        parity = [float((a-b).abs().max()) for a, b in zip(outputs, reference)]
        print("Upstream forward max differences:", parity, flush=True)
        assert parity[0] * 320 < .05 and parity[1] * 1000 < .05
        (ROOT / "artifacts/export-parity.json").write_text(json.dumps(parity))
        for name, value in zip(["img", "rays", "cond"], inputs):
            value.numpy().tofile(out / (name + ".f32"))
        for i, value in enumerate(reference):
            value.numpy().tofile(out / f"reference{i}.f32")
        print("Reference outputs:", [tuple(x.shape) for x in outputs], flush=True)
        convert(net, inputs, out / "pose")
    else:
        upstream_modules()
        from fah.yolo import load_yolo
        class Detector(nn.Module):
            def __init__(self):
                super().__init__()
                self.net = load_yolo(out / "detector.pt")
                print("Detector classes:", self.net.names, flush=True)
            def forward(self, x):
                y = self.net(x)
                return y[0][:, :4 + self.net.head.classes] if isinstance(y, tuple) else y
        net = Detector().eval()
        convert(net, (torch.rand(1, 3, 640, 640),), out / "detector")


if __name__ == "__main__":
    main()

"""Compare exported ncnn pose outputs against the PyTorch reference."""
from pathlib import Path
import json
import time
import ncnn
import numpy as np

ROOT = Path(__file__).resolve().parents[1]
model = ROOT / "models"
with ncnn.Net() as net:
    net.opt.num_threads = 8
    net.opt.use_vulkan_compute = False
    assert net.load_param(str(model / "pose.ncnn.param")) == 0
    assert net.load_model(str(model / "pose.ncnn.bin")) == 0
    start = time.perf_counter()
    ex = net.create_extractor()
    for i, (name, shape) in enumerate([("img", (3, 320, 320)), ("rays", (400, 99)), ("cond", (3,))]):
        a = np.fromfile(model / (name + ".f32"), np.float32).reshape(shape)
        assert ex.input(f"in{i}", ncnn.Mat(a).clone()) == 0
    results = []
    for i in range(6):
        status, out = ex.extract(f"out{i}")
        assert status == 0
        a = np.asarray(out).reshape(-1).copy()
        ref = np.fromfile(model / f"reference{i}.f32", np.float32)
        assert a.size == ref.size and np.isfinite(a).all()
        error = np.abs(a - ref)
        results.append(dict(output=i, count=int(a.size), max=float(error.max()), mean=float(error.mean())))
    report = dict(seconds=time.perf_counter()-start, outputs=results)
    print(json.dumps(report, indent=2))
    (ROOT / "artifacts/host-parity.json").write_text(json.dumps(report, indent=2))
    assert results[0]["max"] * 320 < 0.5, "UV parity exceeds 0.5 crop pixels"
    assert results[1]["max"] * 1000 < 1.0, "XYZ parity exceeds 1 mm"
    del out, ex

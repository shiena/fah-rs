"""Summarize measured device results; fail if parity or the tracking smoke test fails."""
import json
from pathlib import Path
import statistics

root = Path(__file__).resolve().parents[1]
artifacts=root / "artifacts"
def rows(path):
    data=path.read_bytes()
    text=data.decode("utf-16") if data.startswith((b'\xff\xfe',b'\xfe\xff')) else data.decode("utf-8-sig",errors="replace")
    return [json.loads(line) for line in text.splitlines() if line.startswith('{')]

report={}
for backend in ("cpu","gpu"):
    tests=[r for r in rows(artifacts / f"device-{backend}.log") if "passed" in r]
    assert len(tests)==1 and tests[0]["passed"], backend
    report[backend]=tests[0]
report["fixtures"]={}
for name in ("hand_right","hand_mirrored","hand_lower","wide_crop"):
    tests=[r for r in rows(artifacts / f"fixture-{name}.log") if "passed" in r]
    assert len(tests)==1 and tests[0]["passed"], name
    report["fixtures"][name]=tests[0]
frames=[r for r in rows(artifacts / "device-image.log") if "hands" in r]
assert len(frames)>=12 and all(len(f["hands"])==2 for f in frames)
assert all(f.get("backend")=="vulkan" and not f.get("cpu_fallback") for f in frames), "GPU pipeline fell back to CPU"
assert frames[0]["detected"] and not frames[1]["detected"] and frames[10]["detected"]
report["pipeline"]={"input":"FAH published selfie example; same frame repeated", "frames":len(frames),
    "hands":2,"median_ms":statistics.median(f["total_ms"] for f in frames),
    "tracking_median_ms":statistics.median(f["total_ms"] for f in frames if not f["detected"]),
    "detection_frames":[f["frame"] for f in frames if f["detected"]]}
(artifacts / "validation.json").write_text(json.dumps(report,indent=2))
print(json.dumps(report,indent=2))

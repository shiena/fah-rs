"""Create deterministic, local parity fixtures from FAH's published example."""
from pathlib import Path
import json
import numpy as np
from PIL import Image, ImageOps
import torch
from export import Pose, camera_features, ROOT

torch.set_num_threads(8)
net=Pose(ROOT / "models/model.pt").eval()
source=Image.open(ROOT / "third_party/FAH/docs/assets/videos/test_video_selfie_poster.jpg").convert("RGB")
sample=source.crop((0,0,640,426))
sample.save(ROOT / "models/sample.png")
np.asarray(sample.convert("RGBA")).tofile(ROOT / "models/sample.rgba")
cases=[("hand_right",(470,110,680,390),False),
       ("hand_mirrored",(470,110,680,390),True),
       ("hand_lower",(180,290,430,540),False),
       ("wide_crop",(400,80,720,400),False)]
report=[]
for name,box,mirror in cases:
    crop=sample.crop(box).resize((320,320),Image.Resampling.BILINEAR)
    if mirror: crop=ImageOps.mirror(crop)
    img=torch.from_numpy(np.asarray(crop).copy()).permute(2,0,1).float()[None]/255
    rays=camera_features([.0018,.12,.0022,-.18])
    cond=torch.tensor([[.4,.1,.65]])
    inputs=(img,rays,cond)
    with torch.no_grad():
        outputs=net.reference(*inputs)
        port=net(*inputs)
    error=[float((a-b).abs().max()) for a,b in zip(outputs,port)]
    assert error[0]*320<.05 and error[1]*1000<.05
    dest=ROOT / "models/fixtures" / name
    dest.mkdir(parents=True,exist_ok=True)
    crop.save(dest / "crop.png")
    for key,tensor in zip(["img","rays","cond"],inputs): tensor.numpy().tofile(dest / f"{key}.f32")
    for i,tensor in enumerate(outputs): tensor.numpy().tofile(dest / f"reference{i}.f32")
    report.append(dict(name=name,reference_rewrite_max=error))
(ROOT / "artifacts/fixtures.json").write_text(json.dumps(report,indent=2))
print(json.dumps(report,indent=2))

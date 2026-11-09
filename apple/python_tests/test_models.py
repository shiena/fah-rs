import importlib.util
import json
from pathlib import Path
import struct
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("apple_models", ROOT / "tools/apple-models.py")
MODELS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODELS)


class ModelPreparationTests(unittest.TestCase):
    def test_extracts_exact_grid_from_weight_blob(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "source"
            source.mkdir()
            grid = struct.pack("<800f", *[i / 8 for i in range(800)])
            (source / "fah-s-pose.bin").write_bytes(bytes(256) + grid + b"suffix")
            metadata = {"tensors": {"uv_ds": {"offset": 256, "dtype": "f32", "shape": [400, 2]}}}
            (source / "fah-s-pose.json").write_text(json.dumps(metadata))
            output = Path(temporary) / "output"
            MODELS.prepare(source, output, False)
            self.assertEqual((output / "uv_grid.f32").read_bytes(), grid)
            self.assertEqual((output / "fah-s-pose.bin").read_bytes(), (source / "fah-s-pose.bin").read_bytes())
            metadata["tensors"]["uv_ds"]["offset"] = -1
            (source / "fah-s-pose.json").write_text(json.dumps(metadata))
            with self.assertRaisesRegex(ValueError, "offset"):
                MODELS.prepare(source, output, False)
            with self.assertRaisesRegex(ValueError, "separate"):
                MODELS.prepare(source, source, False)


if __name__ == "__main__":
    unittest.main()

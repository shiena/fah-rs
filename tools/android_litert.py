"""Collect the pinned Android arm64 LiteRT C runtime from Google's Maven AAR."""

import hashlib
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VERSION = "2.3.0"
SHA256 = "9508c6e9142646f0f2bc1fe68b3925fb470ee7ddd29114540339a04a8ab189dc"
URL = (
    "https://dl.google.com/dl/android/maven2/com/google/ai/edge/litert/"
    f"litert/{VERSION}/litert-{VERSION}.aar"
)


def collect(destination: Path) -> None:
    cache = ROOT / "third_party" / f"litert-android-{VERSION}.aar"
    cache.parent.mkdir(parents=True, exist_ok=True)
    if not cache.exists():
        urllib.request.urlretrieve(URL, cache)
    if hashlib.sha256(cache.read_bytes()).hexdigest() != SHA256:
        raise ValueError(f"LiteRT AAR checksum mismatch: {cache}")
    with zipfile.ZipFile(cache) as archive:
        destination.mkdir(parents=True, exist_ok=True)
        (destination / "libLiteRt.so").write_bytes(
            archive.read("jni/arm64-v8a/libLiteRt.so")
        )
        (destination / "LITERT-LICENSE.txt").write_bytes(archive.read("LICENSE"))


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    collect(parser.parse_args().destination)

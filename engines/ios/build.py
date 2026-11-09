"""Build the iOS static C ABI and a signed on-device validation app."""

import argparse
import os
import pathlib
import platform
import plistlib
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
OUT = ROOT / "engines/build/ios"


def run(args, env=None):
    subprocess.run([str(a) for a in args], cwd=ROOT, env=env, check=True)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--team")
    p.add_argument("--models", type=pathlib.Path, default=ROOT / "models/apple")
    p.add_argument(
        "--model-package",
        type=pathlib.Path,
        default=ROOT / "models/apple-export/detector-640.mlpackage",
    )
    a = p.parse_args()
    if platform.system() != "Darwin":
        p.error("Build iOS on a Mac with Xcode")
    OUT.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["IPHONEOS_DEPLOYMENT_TARGET"] = "18.0"
    env["CARGO_PROFILE_RELEASE_STRIP"] = "none"
    env["SDKROOT"] = subprocess.check_output(
        ["xcrun", "--sdk", "iphoneos", "--show-sdk-path"], text=True
    ).strip()
    run(["rustup", "target", "add", "aarch64-apple-ios"])
    run(
        [
            "cargo",
            "rustc",
            "-p",
            "fah-ffi",
            "--lib",
            "--crate-type",
            "staticlib",
            "--release",
            "--locked",
            "--target",
            "aarch64-apple-ios",
            "--target-dir",
            OUT / "rust",
        ],
        env,
    )
    shutil.copy2(OUT / "rust/aarch64-apple-ios/release/libfah_engine.a", OUT)
    if not a.team:
        print(OUT / "libfah_engine.a")
        return
    if not (ROOT / "apple/Artifacts/FAHCore.xcframework").exists():
        run([sys.executable, ROOT / "apple/build.py"])
    models = OUT / "models"
    models.mkdir(exist_ok=True)
    for name in ["fah-s-pose.bin", "fah-s-pose.json", "uv_grid.f32", "sample.rgba"]:
        source = a.models / name
        if not source.exists() and name == "sample.rgba":
            source = ROOT / "models/sample.rgba"
        shutil.copy2(source, models / name)
    shutil.copytree(
        a.model_package, models / "detector-640.mlpackage", dirs_exist_ok=True
    )
    objects = {}

    def obj(isa, **kwargs):
        key = f"{len(objects) + 1:024X}"
        objects[key] = {"isa": isa, **kwargs}
        return key

    source = obj(
        "PBXFileReference",
        lastKnownFileType="sourcecode.swift",
        path="CheckApp.swift",
        sourceTree="<group>",
    )
    folder = obj(
        "PBXFileReference",
        lastKnownFileType="folder",
        path="../build/ios/models",
        name="models",
        sourceTree="<group>",
    )
    product = obj(
        "PBXFileReference",
        explicitFileType="wrapper.application",
        path="FAHEngineCheck.app",
        sourceTree="BUILT_PRODUCTS_DIR",
    )
    group = obj("PBXGroup", children=[source, folder, product], sourceTree="<group>")
    package = obj("XCLocalSwiftPackageReference", relativePath="../../apple")
    dep = obj(
        "XCSwiftPackageProductDependency", package=package, productName="FAHAppleNative"
    )
    sources = obj(
        "PBXSourcesBuildPhase",
        buildActionMask=2147483647,
        files=[obj("PBXBuildFile", fileRef=source)],
        runOnlyForDeploymentPostprocessing=0,
    )
    resources = obj(
        "PBXResourcesBuildPhase",
        buildActionMask=2147483647,
        files=[obj("PBXBuildFile", fileRef=folder)],
        runOnlyForDeploymentPostprocessing=0,
    )
    frameworks = obj(
        "PBXFrameworksBuildPhase",
        buildActionMask=2147483647,
        files=[obj("PBXBuildFile", productRef=dep)],
        runOnlyForDeploymentPostprocessing=0,
    )
    embed = obj(
        "PBXCopyFilesBuildPhase",
        buildActionMask=2147483647,
        dstPath="",
        dstSubfolderSpec=10,
        name="Embed Frameworks",
        files=[
            obj(
                "PBXBuildFile",
                productRef=dep,
                settings={"ATTRIBUTES": ["CodeSignOnCopy", "RemoveHeadersOnCopy"]},
            )
        ],
        runOnlyForDeploymentPostprocessing=0,
    )

    def configs(settings):
        cs = [
            obj("XCBuildConfiguration", name=n, buildSettings=settings)
            for n in ["Debug", "Release"]
        ]
        return obj(
            "XCConfigurationList",
            buildConfigurations=cs,
            defaultConfigurationIsVisible=0,
            defaultConfigurationName="Release",
        )

    settings = {
        "PRODUCT_BUNDLE_IDENTIFIER": "dev.fah.enginecheck",
        "PRODUCT_NAME": "FAHEngineCheck",
        "SWIFT_VERSION": "5.0",
        "IPHONEOS_DEPLOYMENT_TARGET": "18.0",
        "SDKROOT": "iphoneos",
        "TARGETED_DEVICE_FAMILY": "1,2",
        "GENERATE_INFOPLIST_FILE": "YES",
        "INFOPLIST_KEY_UILaunchScreen_Generation": "YES",
        "CODE_SIGN_STYLE": "Automatic",
        "DEVELOPMENT_TEAM": a.team,
        "SWIFT_OBJC_BRIDGING_HEADER": "../include/fah_engine.h",
        "OTHER_LDFLAGS": [
            "$(inherited)",
            str(OUT / "libfah_engine.a"),
            "-lresolv",
            "-liconv",
            "-framework",
            "Security",
        ],
        "LD_RUNPATH_SEARCH_PATHS": ["$(inherited)", "@executable_path/Frameworks"],
        "SWIFT_OPTIMIZATION_LEVEL": "-O",
        "ENABLE_USER_SCRIPT_SANDBOXING": "NO",
        "ALWAYS_SEARCH_USER_PATHS": "NO",
    }
    target = obj(
        "PBXNativeTarget",
        name="FAHEngineCheck",
        productName="FAHEngineCheck",
        productReference=product,
        productType="com.apple.product-type.application",
        buildConfigurationList=configs(settings),
        buildPhases=[sources, frameworks, resources, embed],
        buildRules=[],
        dependencies=[],
        packageProductDependencies=[dep],
    )
    project = obj(
        "PBXProject",
        attributes={"LastUpgradeCheck": "2700"},
        buildConfigurationList=configs({}),
        compatibilityVersion="Xcode 14.0",
        developmentRegion="en",
        knownRegions=["en", "Base"],
        mainGroup=group,
        productRefGroup=group,
        projectDirPath="",
        projectRoot="",
        targets=[target],
        packageReferences=[package],
    )
    path = ROOT / "engines/ios/FAHEngineCheck.xcodeproj"
    path.mkdir(exist_ok=True)
    (path / "project.pbxproj").write_bytes(
        plistlib.dumps(
            {
                "archiveVersion": "1",
                "classes": {},
                "objectVersion": "56",
                "objects": objects,
                "rootObject": project,
            }
        )
    )
    run(
        [
            "xcodebuild",
            "-project",
            path,
            "-scheme",
            "FAHEngineCheck",
            "-configuration",
            "Release",
            "-destination",
            "generic/platform=iOS",
            "-derivedDataPath",
            OUT / "app",
            "-allowProvisioningUpdates",
            "build",
        ]
    )


if __name__ == "__main__":
    main()

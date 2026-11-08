#pragma once
#include <array>
#include <string>
#include <vector>
#include <net.h>

namespace fah {
using Outputs = std::array<ncnn::Mat, 6>;
std::string gpuInfo();
class Runtime {
public:
    Runtime(const std::string& directory, bool gpu, bool half = false, bool detector = false);
    Outputs pose(const ncnn::Mat& image, const ncnn::Mat& rays, const ncnn::Mat& condition);
    ncnn::Mat detect(const ncnn::Mat& image);
    std::string coverage() const;
private:
    ncnn::Net poseNet, detectorNet;
    std::vector<std::string> checkpoints;
};
}

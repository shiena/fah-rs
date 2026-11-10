#include "runtime.h"
#include "vulkan_ops.h"
#include <gpu.h>
#include <layer.h>
#include <fstream>
#include <sstream>
#include <stdexcept>
#include <cstdlib>
#include <cmath>
#include <filesystem>

namespace fah {
std::string gpuInfo() {
    if (ncnn::get_gpu_count() == 0) return "{\"available\":false}";
    auto& g = ncnn::get_gpu_info(ncnn::get_default_gpu_index());
    std::ostringstream s;
    s << "{\"available\":true,\"name\":\"" << g.device_name()
      << "\",\"api_version\":" << g.api_version()
      << ",\"driver_version\":" << g.driver_version()
      << ",\"fp16_storage\":" << g.support_fp16_storage()
      << ",\"fp16_arithmetic\":" << g.support_fp16_arithmetic()
      << ",\"cooperative_matrix\":" << g.support_cooperative_matrix() << "}";
    return s.str();
}
Runtime::Runtime(const std::string& directory, bool gpu, bool half, bool detector, bool pose) {
    if (gpu && ncnn::get_gpu_count() == 0) throw std::runtime_error("Vulkan GPU unavailable");
    auto load = [&](ncnn::Net& net, const char* stem) {
        net.opt.num_threads = 4;
        net.opt.use_vulkan_compute = gpu;
#ifdef _WIN32
        // Retaining GPU intermediates avoids incorrect FP32 results with buffer reuse on RTX 5070.
        if (gpu) net.opt.lightmode = false;
#endif
        net.opt.use_fp16_packed = half;
        net.opt.use_fp16_storage = half;
        net.opt.use_fp16_arithmetic = false;
        net.opt.use_bf16_storage = false;
        if(gpu && std::string(stem)=="pose") registerVulkanOps(net);
        const auto base = std::filesystem::u8path(directory);
        if (net.load_param((base / (std::string(stem) + ".ncnn.param")).c_str()) ||
            net.load_model((base / (std::string(stem) + ".ncnn.bin")).c_str()))
            throw std::runtime_error(std::string("Failed to load ") + stem);
    };
    if (pose) load(poseNet, "pose");
    if(gpu && pose) {
        // Bound command-buffer duration on older Adreno drivers while the camera renders.
        for(int i=0;i<12;++i) {
            const std::string name="add_"+std::to_string(14+13*i);
            for(const auto* layer:poseNet.layers()) if(layer->name==name && layer->tops.size()==1)
                checkpoints.push_back(poseNet.blobs()[layer->tops[0]].name);
        }
        if(checkpoints.size()!=12) throw std::runtime_error("Unexpected FAH encoder graph");
    }
    if (detector) load(detectorNet, "detector");
}
Outputs Runtime::pose(const ncnn::Mat& image, const ncnn::Mat& rays, const ncnn::Mat& condition) {
    auto ex = poseNet.create_extractor();
    const char* trace=std::getenv("FAH_TRACE");
    if(trace) ex.set_light_mode(false);
    if (ex.input("in0", image) || ex.input("in1", rays) || ex.input("in2", condition))
        throw std::runtime_error("Invalid pose input");
    Outputs outputs;
    for(const auto& checkpoint:checkpoints) {
        ncnn::Mat boundary;
        if(ex.extract(checkpoint.c_str(),boundary)||boundary.empty())
            throw std::runtime_error("Encoder checkpoint failed: "+checkpoint);
    }
    for (int i = 0; i < 6; ++i) {
        if (ex.extract(("out" + std::to_string(i)).c_str(), outputs[i]) || outputs[i].empty())
            throw std::runtime_error("Failed to extract pose output " + std::to_string(i));
        outputs[i] = outputs[i].clone();
    }
    if(trace) {
        std::ofstream file(trace);
        for(const auto& blob:poseNet.blobs()) {
            ncnn::Mat m;
            if(ex.extract(blob.name.c_str(),m)||m.empty()) continue;
            double sum=0,squares=0;float lo=INFINITY,hi=-INFINITY;
            for(int c=0;c<m.c;++c) {
                const float* p=m.channel(c);
                for(int j=0;j<m.w*m.h*m.d;++j) {float x=p[j];sum+=x;squares+=double(x)*x;lo=std::min(lo,x);hi=std::max(hi,x);}
            }
            size_t n=size_t(m.w)*m.h*m.d*m.c;
            file<<blob.name<<' '<<m.dims<<' '<<m.w<<' '<<m.h<<' '<<m.c<<' '<<sum/n<<' '<<std::sqrt(squares/n)<<' '<<lo<<' '<<hi<<'\n';
        }
    }
    return outputs;
}
ncnn::Mat Runtime::detect(const ncnn::Mat& image) {
    auto ex = detectorNet.create_extractor();
    ncnn::Mat out;
    if (ex.input("in0", image) || ex.extract("out0", out) || out.empty())
        throw std::runtime_error("Detector inference failed");
    return out.clone();
}
std::string Runtime::coverage() const {
    size_t gpu = 0;
    std::ostringstream unsupported;
    for (auto* layer : poseNet.layers()) {
        if (layer->support_vulkan) ++gpu;
        else if (layer->type != "Input" && layer->type != "Split")
            unsupported << layer->type << ":" << layer->name << " ";
    }
    return "{\"layers\":" + std::to_string(poseNet.layers().size()) + ",\"vulkan_capable\":" +
        std::to_string(gpu) + ",\"cpu_layers\":\"" + unsupported.str() + "\"}";
}
}

#include "runtime.h"
#include <gpu.h>
#include <exception>
#include <memory>

#ifdef _WIN32
#define FAH_EXPORT __declspec(dllexport)
#else
#define FAH_EXPORT
#endif

namespace {
thread_local std::string lastError, text;
struct Runtime {
    fah::Runtime net;
    std::vector<float> buffers[6];
    Runtime(const char* path, bool gpu, bool half, bool detector): net(path,gpu,half,detector) {}
};
}
extern "C" {
struct FahView { const float* data; int w,h,c; };
// Shut Vulkan down before Windows enters DLL process-detach callbacks.
FAH_EXPORT void fah_shutdown() noexcept { ncnn::destroy_gpu_instance(); }
FAH_EXPORT const char* fah_error() noexcept { return lastError.c_str(); }
FAH_EXPORT void* fah_runtime_create(const char* path,bool gpu,bool half,bool detector) noexcept {
    try { return new Runtime(path,gpu,half,detector); }
    catch(const std::exception& e) { lastError=e.what();return nullptr; }
    catch(...) { lastError="Unknown ncnn error";return nullptr; }
}
FAH_EXPORT void fah_runtime_destroy(void* p) noexcept { delete static_cast<Runtime*>(p); }
FAH_EXPORT const char* fah_gpu_info() noexcept {
    try { text=fah::gpuInfo(); } catch(...) { text="{\"available\":false}"; }
    return text.c_str();
}
FAH_EXPORT const char* fah_coverage(void* p) noexcept {
    try { text=static_cast<Runtime*>(p)->net.coverage(); } catch(...) { text="{}"; }
    return text.c_str();
}
static void copy(const ncnn::Mat& mat,std::vector<float>& buffer,FahView& view) {
    buffer.clear();
    for(int c=0;c<mat.c;++c) {
        const float* channel=mat.channel(c);
        buffer.insert(buffer.end(),channel,channel+mat.w*mat.h*mat.d);
    }
    view={buffer.data(),mat.w,mat.h*mat.d,mat.c};
}
FAH_EXPORT bool fah_pose(void* p,const float* image,const float* rays,const float* condition,FahView* output) noexcept {
    try {
        auto& r=*static_cast<Runtime*>(p);
        auto out=r.net.pose(ncnn::Mat(320,320,3,const_cast<float*>(image)),ncnn::Mat(99,400,const_cast<float*>(rays)),ncnn::Mat(3,const_cast<float*>(condition)));
        for(int i=0;i<6;++i) copy(out[i],r.buffers[i],output[i]);
        return true;
    } catch(const std::exception& e) { lastError=e.what();return false; }
    catch(...) { lastError="Unknown ncnn error";return false; }
}
FAH_EXPORT bool fah_detect(void* p,const float* image,FahView* output) noexcept {
    try {
        auto& r=*static_cast<Runtime*>(p);
        copy(r.net.detect(ncnn::Mat(640,640,3,const_cast<float*>(image))),r.buffers[0],*output);
        return true;
    } catch(const std::exception& e) { lastError=e.what();return false; }
    catch(...) { lastError="Unknown ncnn error";return false; }
}
}

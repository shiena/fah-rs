#pragma once
#include "runtime.h"
#include <cstdint>
#include <memory>

namespace fah {
struct Box { float x0,y0,x1,y1,score; bool right; };
class Pipeline {
public:
    Pipeline(const std::string& dir, bool gpu, bool half=false);
    std::string process(const uint8_t* rgba, int width, int height, int stride, bool reset=false);
private:
    std::unique_ptr<Runtime> runtime;
    std::string directory;
    bool gpuActive;
    bool cpuFallback=false;
    double recoveryMs=0;
    std::string processFrame(const uint8_t* rgba,int width,int height,int stride,bool reset);
    std::vector<float> grid;
    std::vector<Box> previous;
    unsigned frame=0;
    int lastWidth=0,lastHeight=0;
};
}

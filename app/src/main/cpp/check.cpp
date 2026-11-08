#include "runtime.h"
#include "pipeline.h"
#include <gpu.h>
#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstring>
#include <iostream>
#include <fstream>
#include <stdexcept>

int main(int argc, char** argv) {
    try {
        if (argc < 2) throw std::runtime_error("Usage: fah-check MODELS [cpu|gpu|half] [runs]");
        std::string dir = argv[1], backend = argc > 2 ? argv[2] : "gpu";
        if(backend=="image") {
            if(argc<6) throw std::runtime_error("Usage: fah-check MODELS image RGBA WIDTH HEIGHT [runs]");
            int w=std::stoi(argv[4]),h=std::stoi(argv[5]);
            if(w<1||h<1||w>8192||h>8192) throw std::runtime_error("Invalid image dimensions");
            std::vector<uint8_t> rgba(size_t(w)*h*4);
            std::ifstream input(argv[3],std::ios::binary);
            if(!input.read(reinterpret_cast<char*>(rgba.data()),rgba.size())) throw std::runtime_error("Invalid RGBA file");
            fah::Pipeline pipeline(dir,true);
            int count=argc>6?std::stoi(argv[6]):1;
            for(int i=0;i<count;++i) std::cout<<pipeline.process(rgba.data(),w,h,w*4)<<std::endl;
            return 0;
        }
        int runs = argc > 3 ? std::stoi(argv[3]) : 10;
        std::string fixture=argc>4?argv[4]:dir;
        if (runs < 1 || runs > 1000) throw std::runtime_error("runs must be 1..1000");
        std::cerr << "GPU " << fah::gpuInfo() << std::endl;
        fah::Runtime net(dir, backend != "cpu", backend == "half");
        std::cerr << "COVERAGE " << net.coverage() << std::endl;
        auto img = fah::readFloats(fixture + "/img.f32");
        auto ray = fah::readFloats(fixture + "/rays.f32");
        auto cond = fah::readFloats(fixture + "/cond.f32");
        if (img.size()!=3*320*320 || ray.size()!=400*99 || cond.size()!=3)
            throw std::runtime_error("Incorrect fixture size");
        ncnn::Mat im(320, 320, 3, img.data()), ra(99, 400, ray.data()), co(3, cond.data());
        std::vector<double> times;
        fah::Outputs outputs;
        for (int i = std::getenv("FAH_TRACE")?0:-2; i < runs; ++i) {
            auto start = std::chrono::steady_clock::now();
            outputs = net.pose(im, ra, co);
            double ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now()-start).count();
            std::cerr << "run " << i << " " << ms << " ms" << std::endl;
            if (i >= 0) times.push_back(ms);
        }
        std::sort(times.begin(), times.end());
        double median = (times[(runs-1)/2] + times[runs/2])*.5;
        bool passed = true;
        std::cout << "{\"backend\":\"" << backend << "\",\"runs\":" << runs
                  << ",\"median_ms\":" << median << ",\"fps\":" << 1000/median << ",\"outputs\":[";
        for (int i = 0; i < 6; ++i) {
            auto ref = fah::readFloats(fixture + "/reference" + std::to_string(i) + ".f32");
            auto& out = outputs[i];
            if (ref.size() != size_t(out.w*out.h*out.d*out.c)) throw std::runtime_error("Output size mismatch");
            double mx=0, mean=0;
            for (size_t j=0; j<ref.size(); ++j) {
                float actual = reinterpret_cast<const float*>(out.data)[j];
                if (!std::isfinite(actual)) throw std::runtime_error("Non-finite output");
                double d = std::abs(actual-ref[j]); mx=std::max(mx,d); mean+=d;
            }
            if ((i==0 && mx*320 > .5) || (i==1 && mx*1000 > 1)) passed=false;
            if (i) std::cout << ',';
            std::cout << "{\"index\":" << i << ",\"max\":" << mx << ",\"mean\":" << mean/ref.size() << "}";
            fah::writeFloats(fixture + "/actual" + std::to_string(i) + ".f32", out);
        }
        std::cout << "],\"passed\":" << (passed?"true":"false") << "}" << std::endl;
        return passed ? 0 : 2;
    } catch (const std::exception& e) { std::cerr << e.what() << std::endl; return 1; }
}

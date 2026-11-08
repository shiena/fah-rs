#include "pipeline.h"
#include <algorithm>
#include <chrono>
#include <cmath>
#include <sstream>
#include <stdexcept>

namespace fah {
namespace {
using Clock=std::chrono::steady_clock;
double elapsed(Clock::time_point t) { return std::chrono::duration<double,std::milli>(Clock::now()-t).count(); }
float sigmoid(float x) { return 1.f/(1.f+std::exp(-x)); }
float iou(const Box& a,const Box& b) {
    float area=std::max(0.f,std::min(a.x1,b.x1)-std::max(a.x0,b.x0))*std::max(0.f,std::min(a.y1,b.y1)-std::max(a.y0,b.y0));
    return area/std::max(1.f,(a.x1-a.x0)*(a.y1-a.y0)+(b.x1-b.x0)*(b.y1-b.y0)-area);
}
ncnn::Mat warp(const uint8_t* src,int width,int height,int stride,int size,
               float sx,float ox,float sy,float oy,float border=0.f) {
    ncnn::Mat dst(size,size,3);
    for (int y=0;y<size;++y) for (int x=0;x<size;++x) {
        float px=sx*x+ox,py=sy*y+oy;
        int ix=int(std::floor(px)),iy=int(std::floor(py));
        float dx=px-ix,dy=py-iy;
        for(int c=0;c<3;++c) {
            float v=0;
            for(int j=0;j<2;++j) for(int k=0;k<2;++k) {
                int xx=ix+k,yy=iy+j;
                float sample=(xx>=0&&xx<width&&yy>=0&&yy<height)?src[yy*stride+xx*4+c]:border;
                v+=sample*(k?dx:1-dx)*(j?dy:1-dy);
            }
            dst.channel(c).row(y)[x]=v/255.f;
        }
    }
    return dst;
}
}
Pipeline::Pipeline(const std::string& dir,bool gpu,bool half):runtime(std::make_unique<Runtime>(dir,gpu,half,true)),directory(dir),gpuActive(gpu),grid(readFloats(dir+"/uv_grid.f32")) {
    if(grid.size()!=800) throw std::runtime_error("Invalid camera grid");
}
std::string Pipeline::process(const uint8_t* rgba,int width,int height,int stride,bool reset) {
    const auto start=Clock::now();
    recoveryMs=0;
    try { return processFrame(rgba,width,height,stride,reset); }
    catch(const std::runtime_error&) {
        if(!gpuActive) throw;
        // Keep camera inference available when the device's GPU driver fails.
        runtime=std::make_unique<Runtime>(directory,false,false,true);
        gpuActive=false;cpuFallback=true;
        recoveryMs=elapsed(start);
        return processFrame(rgba,width,height,stride,true);
    }
}
std::string Pipeline::processFrame(const uint8_t* rgba,int width,int height,int stride,bool reset) {
    const auto start=Clock::now();
    if(reset||width!=lastWidth||height!=lastHeight) { previous.clear(); frame=0; }
    lastWidth=width;lastHeight=height;
    bool detection=previous.empty()||frame%10==0;
    ++frame;
    std::vector<Box> boxes;
    double detMs=0,poseMs=0;
    if(detection) {
        auto t=Clock::now();
        float scale=640.f/std::max(width,height);
        int nw=std::round(width*scale),nh=std::round(height*scale);
        int padX=(640-nw)/2,padY=(640-nh)/2;
        auto input=warp(rgba,width,height,stride,640,1/scale,(.5f-padX)/scale-.5f,1/scale,(.5f-padY)/scale-.5f,114);
        auto out=runtime->detect(input);
        if(out.dims!=2||out.h!=6) throw std::runtime_error("Unexpected detector output shape");
        std::vector<Box> candidates;
        for(int i=0;i<out.w;++i) {
            float left=out.row(4)[i],right=out.row(5)[i],score=std::max(left,right);
            if(score<.05f) continue;
            float cx=(out.row(0)[i]-padX)/scale,cy=(out.row(1)[i]-padY)/scale;
            float bw=out.row(2)[i]/scale,bh=out.row(3)[i]/scale;
            Box b{std::clamp(cx-bw*.5f,0.f,float(width-1)),std::clamp(cy-bh*.5f,0.f,float(height-1)),
                  std::clamp(cx+bw*.5f,0.f,float(width-1)),std::clamp(cy+bh*.5f,0.f,float(height-1)),score,right>left};
            if(b.x1-b.x0>4&&b.y1-b.y0>4) candidates.push_back(b);
        }
        std::stable_sort(candidates.begin(),candidates.end(),[](const Box&a,const Box&b){return a.score>b.score;});
        for(auto b:candidates) {
            bool overlap=false;
            for(auto kept:boxes) if(iou(b,kept)>.45f) overlap=true;
            if(!overlap) boxes.push_back(b);
            if(boxes.size()==2) break;
        }
        detMs=elapsed(t);
    } else boxes=previous;
    previous.clear();
    std::ostringstream hands;
    int count=0;
    // Intrinsics are estimated until calibrated camera intrinsics are supplied.
    const float focal=std::sqrt(float(width*width+height*height));
    const float kx=width*.5f,ky=height*.5f;
    for(auto b:boxes) {
        auto t=Clock::now();
        float cx=(b.x0+b.x1)*.5f,cy=(b.y0+b.y1)*.5f;
        float bw=b.x1-b.x0,bh=b.y1-b.y0;
        if(bh>bw) bw=std::max(bw,1.1f*bh/(4.f/3*.9f));
        float size=std::max(bw*.9f/.75f,bh*.9f);
        size=std::max(size,8.f);
        float cxMirror=b.right?cx:width-1-cx;
        float kxMirror=b.right?kx:width-1-kx;
        float left=cxMirror-size*.5f,top=cy-size*.5f;
        float step=size/320;
        auto image=warp(rgba,width,height,stride,320,b.right?step:-step,
                        b.right?left:width-1-left,step,top);
        ncnn::Mat rays(99,400),condition(3);
        for(int p=0;p<400;++p) {
            float pos[3]={(grid[2*p]*step+left-kxMirror)/focal,(grid[2*p+1]*step+top-ky)/focal,1};
            float* row=rays.row(p);
            for(int c=0;c<3;++c) {
                row[c]=pos[c];
                for(int j=0;j<16;++j) {
                    float phase=pos[c]*(1+31.f*j/15)*3.14159265358979323846f;
                    row[3+c*16+j]=std::sin(phase);row[51+c*16+j]=std::cos(phase);
                }
            }
        }
        condition[0]=(cxMirror-kxMirror)/focal;condition[1]=(cy-ky)/focal;condition[2]=size/focal;
        auto out=runtime->pose(image,rays,condition);
        poseMs+=elapsed(t);
        for(const auto& mat:out) for(int c=0;c<mat.c;++c) {
            const float* values=mat.channel(c);
            for(int i=0;i<mat.w*mat.h*mat.d;++i)
                if(!std::isfinite(values[i])) {
                    writeFloats(directory+"/failure-img.f32",image);
                    writeFloats(directory+"/failure-rays.f32",rays);
                    writeFloats(directory+"/failure-cond.f32",condition);
                    throw std::runtime_error("Non-finite pose result at "+std::to_string(i));
                }
        }
        float presence=sigmoid(out[5][0]),side=sigmoid(out[5][1]);
        float score=std::sqrt(b.score*presence);
        if(presence<.5f||score<.15f) continue;
        std::vector<float> uv(922),xyz(1383);
        float tz=std::exp(out[4][0])*focal/size,tx=0,ty=0;
        for(int p=0;p<461;++p) {
            float x=out[0].row(p)[0]*size+left;
            uv[2*p]=b.right?x:width-1-x;uv[2*p+1]=out[0].row(p)[1]*size+top;
            xyz[3*p]=out[1].row(p)[0]*(b.right?1:-1);
            xyz[3*p+1]=out[1].row(p)[1];xyz[3*p+2]=out[1].row(p)[2];
            tx+=(uv[2*p]-kx)*(xyz[3*p+2]+tz)/focal-xyz[3*p];
            ty+=(uv[2*p+1]-ky)*(xyz[3*p+2]+tz)/focal-xyz[3*p+1];
        }
        tx/=461;ty/=461;
        Box next{float(width),float(height),0,0,b.score,b.right};
        for(int p=0;p<461;++p) {
            next.x0=std::min(next.x0,uv[2*p]);next.y0=std::min(next.y0,uv[2*p+1]);
            next.x1=std::max(next.x1,uv[2*p]);next.y1=std::max(next.y1,uv[2*p+1]);
        }
        next.x0=std::clamp(next.x0,0.f,float(width-1));next.x1=std::clamp(next.x1,0.f,float(width-1));
        next.y0=std::clamp(next.y0,0.f,float(height-1));next.y1=std::clamp(next.y1,0.f,float(height-1));
        if(next.x1-next.x0>4&&next.y1-next.y0>4) previous.push_back(next);
        if(count++) hands<<',';
        hands<<"{\"right\":"<<(b.right?"true":"false")<<",\"score\":"<<score<<",\"presence\":"<<presence
             <<",\"side\":"<<side<<",\"joints2d\":[";
        for(int p=0;p<21;++p) { if(p) hands<<',';hands<<'['<<uv[2*p]<<','<<uv[2*p+1]<<']'; }
        hands<<"],\"joints3d\":[";
        for(int p=0;p<21;++p) { if(p) hands<<',';hands<<'['<<xyz[3*p]+tx<<','<<xyz[3*p+1]+ty<<','<<xyz[3*p+2]+tz<<']'; }
        hands<<"]}";
    }
    std::ostringstream result;
    result<<"{\"backend\":\""<<(gpuActive?"vulkan":"cpu")<<"\",\"cpu_fallback\":"<<(cpuFallback?"true":"false")
          <<",\"width\":"<<width<<",\"height\":"<<height<<",\"frame\":"<<frame<<",\"detected\":"<<(detection?"true":"false")
          <<",\"detector_ms\":"<<detMs<<",\"pose_ms\":"<<poseMs<<",\"recovery_ms\":"<<recoveryMs
          <<",\"total_ms\":"<<elapsed(start)+recoveryMs<<",\"hands\":["<<hands.str()<<"]}";
    return result.str();
}
}
